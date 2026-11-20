#![no_std]

mod test;

use soroban_sdk::{
    contract, contractclient, contractimpl, contracttype, Address, BytesN, Env, String,
};

// ── payment_escrow cross-contract interface ─────────────────────────────────
//
// Minimal mirror of payment_escrow's `PaymentEscrow` record and `get_payment`
// entry point, following the same local-trait pattern liquidity_router uses
// for its AmmClient (see contracts/liquidity_router/src/lib.rs). Field names,
// types, and order must stay in sync with payment_escrow::PaymentEscrow.

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum PaymentEscrowAssetType {
    Xlm,
    Usdc,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum PaymentEscrowStatus {
    Pending,
    Disputed,
    Released,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PaymentEscrowRecord {
    pub payment_id: BytesN<32>,
    pub amount: i128,
    pub released_amount: i128,
    pub merchant: Address,
    pub customer: Address,
    pub status: PaymentEscrowStatus,
    pub expiry: u32,
    pub dispute_window_end: u32,
    pub dispute_reason: Option<String>,
    pub asset_type: PaymentEscrowAssetType,
}

#[contractclient(name = "PaymentEscrowClient")]
pub trait PaymentEscrowInterface {
    fn get_payment(env: Env, payment_id: BytesN<32>) -> PaymentEscrowRecord;
    fn release(env: Env, payment_id: BytesN<32>);
}

// ── payment state consolidation (issue #74) ────────────────────────────────

/// Consolidated payment confirmation state (issue #74).
/// Combines 6 separate keys per payment into a single struct to reduce storage bloat.
#[contracttype]
#[derive(Clone, Debug)]
pub struct PaymentConfirmationState {
    pub confs: u32,              // confirmed count so far for a payment
    pub first_ledger: u32,       // ledger_seq of the first confirmation
    pub last_ledger: u32,        // ledger_seq of the most recent confirmation
    pub required_confs: u32,     // required count snapshotted at first confirmation
    pub settling: bool,          // already transitioned to settling state
    pub failed: bool,            // terminal failure state
}

// ── storage keys ────────────────────────────────────────────────────────────

#[contracttype]
enum DataKey {
    Admin,
    ConfirmationCount,          // required ledger-close count (u32)
    PaymentState(BytesN<32>),   // consolidated PaymentConfirmationState (issue #74)
    PaymentEscrowContract,      // optional Address of the payment_escrow contract
}
// ── events ───────────────────────────────────────────────────────────────────

#[contracttype]
struct ConfirmationRecordedEvent {
    payment_id: BytesN<32>,
    ledger_seq: u32,
    confirmations: u32,
    required: u32,
}

#[contracttype]
struct SettlementAuthorisedEvent {
    payment_id: BytesN<32>,
    ledger_seq: u32,   // ledger that pushed count over threshold
    amount: i128,      // informational — set by caller
    merchant: Address, // informational — set by caller
}

#[contracttype]
struct ConfirmationFailedEvent {
    payment_id: BytesN<32>,
    confirmations: u32, // confirmations accumulated at time of failure
    required: u32,
}

/// Emitted when the admin updates the required confirmation count (issue #109).
#[contracttype]
#[derive(Clone, Debug)]
pub struct ConfirmationCountUpdatedEvent {
    pub caller: Address,
    pub old_value: u32,
    pub new_value: u32,
}

/// Emitted when the admin links or unlinks the payment_escrow contract (issue #109).
#[contracttype]
#[derive(Clone, Debug)]
pub struct PaymentEscrowContractUpdatedEvent {
    pub caller: Address,
    pub old_value: Option<Address>,
    pub new_value: Option<Address>,
}

// ── contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct StellarConfirmationsContract;

#[contractimpl]
impl StellarConfirmationsContract {
    pub fn __constructor(env: Env, admin: Address, confirmation_count: u32) {
        assert!(confirmation_count > 0, "confirmation_count must be > 0");
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::ConfirmationCount, &confirmation_count);
    }

    /// Called by the NestJS monitor once per observed Stellar ledger close.
    /// `ledger_seq` is the sequence number of the closed ledger.
    /// `amount` and `merchant` are forwarded into the SettlementAuthorised event
    /// so NestJS can trigger the fiat payout without a second lookup. When a
    /// payment_escrow contract has been configured via
    /// `set_payment_escrow_contract`, every call cross-checks `amount`/`merchant`
    /// against `payment_escrow::get_payment` and panics on a mismatch rather than
    /// trusting the caller blindly. The cross-contract lookup is performed at
    /// most once per invocation (issue #108).
    ///
    /// Returns the current confirmation count after this call.
    pub fn confirm_payment(
        env: Env,
        caller: Address,
        payment_id: BytesN<32>,
        ledger_seq: u32,
        amount: i128,
        merchant: Address,
    ) -> u32 {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        // Cross-contract check: if a payment_escrow contract is linked, verify
        // `amount`/`merchant` against the real on-chain payment record for
        // `payment_id` before recording this confirmation. This closes the gap
        // where a compromised/malfunctioning monitor could fabricate
        // confirmation/settlement events for arbitrary amount/merchant values.
        //
        // This is the single cross-contract lookup for the whole invocation;
        // the threshold-crossing branch below reuses this verification rather
        // than issuing a second identical `get_payment` call (issue #108).
        if let Some(escrow_address) = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::PaymentEscrowContract)
        {
            let escrow_client = PaymentEscrowClient::new(&env, &escrow_address);
            let payment = escrow_client.get_payment(&payment_id);
            assert!(payment.amount == amount, "amount mismatch with payment_escrow record");
            assert!(payment.merchant == merchant, "merchant mismatch with payment_escrow record");
        }

        // Guard: ledger_seq must not be in the future relative to the actual chain state
        assert!(ledger_seq <= env.ledger().sequence(), "ledger_seq exceeds current ledger");

        // Load consolidated payment state (issue #74)
        let mut state = Self::load_payment_state(&env, &payment_id);

        // Guard: ledger_seq must be strictly greater than the last one recorded
        // for this payment. Allowing ledger_seq == last_seen would permit multiple
        // confirmations within the same ledger, enabling artificial inflation of
        // the confirmation counter to the threshold in a single ledger block and
        // invalidating the multi-ledger confirmation requirement.
        if state.last_ledger > 0 {
            assert!(ledger_seq > state.last_ledger, "ledger_seq must exceed last recorded ledger");
        }

        // Guard: already settling — idempotent no-op after threshold
        assert!(!state.settling, "payment already settling");

        // Guard: already failed — terminal state, cannot resume confirming
        assert!(!state.failed, "payment already failed");

        // Increment confirmation counter
        let new_confs = state.confs.checked_add(1).expect("overflow");

        // Record first-seen ledger for auditability (only on first confirmation)
        if state.first_ledger == 0 {
            state.first_ledger = ledger_seq;
        }

        // Record last-seen ledger for the monotonicity check above
        state.last_ledger = ledger_seq;
        state.confs = new_confs;

        // Snapshot the required confirmation count at first confirmation so
        // later admin changes to the global ConfirmationCount don't retroactively
        // affect in-flight payments.
        if state.required_confs == 0 {
            let required: u32 = env
                .storage()
                .instance()
                .get(&DataKey::ConfirmationCount)
                .expect("confirmation count not set");
            state.required_confs = required;
        }

        let required = state.required_confs;

        // Emit per-confirmation event
        env.events().publish(
            ("confirmation_recorded", payment_id.clone()),
            ConfirmationRecordedEvent {
                payment_id: payment_id.clone(),
                ledger_seq,
                confirmations: new_confs,
                required,
            },
        );

        // Transition to Settling once the threshold is reached
        if new_confs >= required {
            state.settling = true;

            env.events().publish(
                ("settlement_authorised", payment_id.clone()),
                SettlementAuthorisedEvent {
                    payment_id: payment_id.clone(),
                    ledger_seq,
                    amount,
                    merchant,
                },
            );
        }

        // Persist consolidated state
        Self::save_payment_state(&env, &payment_id, &state);

        new_confs
    }

    /// Marks a pending confirmation as terminally failed. Admin-only.
    /// Once failed, further `confirm_payment` calls for the payment are rejected.
    pub fn fail_confirmation(env: Env, caller: Address, payment_id: BytesN<32>) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let mut state = Self::load_payment_state(&env, &payment_id);

        // Guard: cannot fail an already-settling payment
        assert!(!state.settling, "payment already settling");
        // Guard: already failed — terminal state
        assert!(!state.failed, "payment already failed");

        state.failed = true;

        env.events().publish(
            ("confirmation_failed", payment_id.clone()),
            ConfirmationFailedEvent {
                payment_id: payment_id.clone(),
                confirmations: state.confs,
                required: state.required_confs,
            },
        );

        Self::save_payment_state(&env, &payment_id, &state);
    }

    // ── state queries ───────────────────────────────────────────────────────

    /// Returns the number of confirmations recorded so far for a payment.
    pub fn get_confirmations(env: Env, payment_id: BytesN<32>) -> u32 {
        Self::load_payment_state(&env, &payment_id).confs
    }

    /// Returns the required confirmation count snapshotted for a payment.
    pub fn get_required_confirmations(env: Env, payment_id: BytesN<32>) -> u32 {
        Self::load_payment_state(&env, &payment_id).required_confs
    }

    /// Returns true once the payment has crossed the confirmation threshold.
    pub fn is_settling(env: Env, payment_id: BytesN<32>) -> bool {
        Self::load_payment_state(&env, &payment_id).settling
    }

    /// Returns true once the payment has been marked terminally failed.
    pub fn is_failed(env: Env, payment_id: BytesN<32>) -> bool {
        Self::load_payment_state(&env, &payment_id).failed
    }

    /// Returns the full consolidated confirmation state for a payment.
    pub fn get_payment_state(env: Env, payment_id: BytesN<32>) -> PaymentConfirmationState {
        Self::load_payment_state(&env, &payment_id)
    }

    /// Returns the globally configured required confirmation count.
    pub fn get_confirmation_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::ConfirmationCount)
            .expect("confirmation count not set")
    }

    /// Returns the current admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set")
    }

    /// Returns the linked payment_escrow contract address, if any.
    pub fn get_payment_escrow_contract(env: Env) -> Option<Address> {
        env.storage()
            .instance()
            .get(&DataKey::PaymentEscrowContract)
    }

    // ── admin entry points ──────────────────────────────────────────────────

    /// Updates the global required confirmation count. Admin-only (issue #109).
    pub fn set_confirmation_count(env: Env, caller: Address, new_value: u32) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        assert!(new_value > 0, "confirmation_count must be > 0");

        let old_value: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ConfirmationCount)
            .expect("confirmation count not set");

        env.storage()
            .instance()
            .set(&DataKey::ConfirmationCount, &new_value);

        env.events().publish(
            ("confirmation_count_updated",),
            ConfirmationCountUpdatedEvent {
                caller,
                old_value,
                new_value,
            },
        );
    }

    /// Links or unlinks the payment_escrow contract. Admin-only (issue #109).
    pub fn set_payment_escrow_contract(
        env: Env,
        caller: Address,
        new_value: Option<Address>,
    ) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let old_value: Option<Address> = env
            .storage()
            .instance()
            .get(&DataKey::PaymentEscrowContract);

        match &new_value {
            Some(addr) => env
                .storage()
                .instance()
                .set(&DataKey::PaymentEscrowContract, addr),
            None => env
                .storage()
                .instance()
                .remove(&DataKey::PaymentEscrowContract),
        }

        env.events().publish(
            ("payment_escrow_contract_updated",),
            PaymentEscrowContractUpdatedEvent {
                caller,
                old_value,
                new_value,
            },
        );
    }

    /// Transfers admin rights to a new address. Admin-only.
    pub fn transfer_admin(env: Env, caller: Address, new_admin: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        env.storage().instance().set(&DataKey::Admin, &new_admin);
    }

    // ── internal helpers ────────────────────────────────────────────────────

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set");
        assert!(*caller == admin, "caller is not admin");
    }

    fn load_payment_state(env: &Env, payment_id: &BytesN<32>) -> PaymentConfirmationState {
        env.storage()
            .persistent()
            .get(&DataKey::PaymentState(payment_id.clone()))
            .unwrap_or(PaymentConfirmationState {
                confs: 0,
                first_ledger: 0,
                last_ledger: 0,
                required_confs: 0,
                settling: false,
                failed: false,
            })
    }

    fn save_payment_state(env: &Env, payment_id: &BytesN<32>, state: &PaymentConfirmationState) {
        env.storage()
            .persistent()
            .set(&DataKey::PaymentState(payment_id.clone()), state);
    }
}
