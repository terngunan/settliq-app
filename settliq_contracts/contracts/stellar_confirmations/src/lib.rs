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

// ── TTL constants (issue #260) ──────────────────────────────────────────────
//
// Payment confirmation records live in persistent storage and are subject to
// Soroban rent reclamation. Slow on-chain confirmation or network delays can
// leave a payment in-flight for many ledgers, so we bump the entry's TTL on
// every write and read to keep the record alive until it reaches its required
// confirmation threshold (or a terminal state).

/// Minimum remaining TTL (in ledgers) we want on a PaymentState entry.
const PAYMENT_STATE_TTL_THRESHOLD: u32 = 100_000;
/// TTL (in ledgers) we extend a PaymentState entry to when below threshold.
const PAYMENT_STATE_TTL_EXTEND_TO: u32 = 500_000;

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
            state.required_confs = Self::required_confs(&env);
        }

        let required = state.required_confs;

        // Persist the updated state and extend its TTL so in-flight payments
        // are not pruned by rent reclamation before reaching the threshold
        // (issue #260).
        Self::save_payment_state(&env, &payment_id, &state);

        env.events().publish(
            (soroban_sdk::symbol_short!("conf_rec"), payment_id.clone()),
            ConfirmationRecordedEvent {
                payment_id: payment_id.clone(),
                ledger_seq,
                confirmations: new_confs,
                required,
            },
        );

        if new_confs >= required {
            state.settling = true;
            Self::save_payment_state(&env, &payment_id, &state);

            env.events().publish(
                (soroban_sdk::symbol_short!("settle_auth"), payment_id.clone()),
                SettlementAuthorisedEvent {
                    payment_id: payment_id.clone(),
                    ledger_seq,
                    amount,
                    merchant,
                },
            );
        }

        new_confs
    }

    /// Returns the current confirmation count for `payment_id`.
    /// Extends the TTL of the payment state entry on read so status checks
    /// keep in-flight records alive (issue #260).
    pub fn get_confirmations(env: Env, payment_id: BytesN<32>) -> u32 {
        Self::load_payment_state(&env, &payment_id).confs
    }

    /// Returns the full consolidated confirmation state for `payment_id`.
    /// Extends the TTL of the payment state entry on read (issue #260).
    pub fn get_payment_state(env: Env, payment_id: BytesN<32>) -> PaymentConfirmationState {
        Self::load_payment_state(&env, &payment_id)
    }

    // ── internal helpers ────────────────────────────────────────────────────

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("admin not set");
        assert!(*caller == admin, "caller is not admin");
    }

    fn required_confs(env: &Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::ConfirmationCount)
            .expect("confirmation count not set")
    }

    /// Loads the consolidated payment state, extending its TTL if present so
    /// in-flight records are protected from archival (issue #260).
    fn load_payment_state(env: &Env, payment_id: &BytesN<32>) -> PaymentConfirmationState {
        let key = DataKey::PaymentState(payment_id.clone());
        let storage = env.storage().persistent();
        if storage.has(&key) {
            storage.extend_ttl(&key, PAYMENT_STATE_TTL_THRESHOLD, PAYMENT_STATE_TTL_EXTEND_TO);
            storage.get(&key).expect("payment state missing")
        } else {
            PaymentConfirmationState {
                confs: 0,
                first_ledger: 0,
                last_ledger: 0,
                required_confs: 0,
                settling: false,
                failed: false,
            }
        }
    }

    /// Persists the consolidated payment state and extends its TTL so the
    /// record survives until the payment reaches a terminal state (issue #260).
    fn save_payment_state(env: &Env, payment_id: &BytesN<32>, state: &PaymentConfirmationState) {
        let key = DataKey::PaymentState(payment_id.clone());
        let storage = env.storage().persistent();
        storage.set(&key, state);
        storage.extend_ttl(&key, PAYMENT_STATE_TTL_THRESHOLD, PAYMENT_STATE_TTL_EXTEND_TO);
    }
}
