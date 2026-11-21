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
        let escrow_address = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::PaymentEscrowContract);
        if let Some(ref escrow_address) = escrow_address {
            let escrow_client = PaymentEscrowClient::new(&env, escrow_address);
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

        // Guard: already failed — terminal state, no further confirmations
        assert!(!state.failed, "payment already failed");

        // Snapshot required confirmations on first confirmation so a later
        // admin change to ConfirmationCount does not retroactively affect
        // in-flight payments.
        if state.confs == 0 {
            let required: u32 = env
                .storage()
                .instance()
                .get(&DataKey::ConfirmationCount)
                .unwrap_or(1);
            state.required_confs = required;
            state.first_ledger = ledger_seq;
        }

        state.confs += 1;
        state.last_ledger = ledger_seq;

        let required = state.required_confs;

        // Emit per-confirmation event
        env.events().publish(
            ("confirmation_recorded",),
            ConfirmationRecordedEvent {
                payment_id: payment_id.clone(),
                ledger_seq,
                confirmations: state.confs,
                required,
            },
        );

        // Threshold-crossing branch: release escrow funds BEFORE persisting the
        // terminal `settling` state. If the cross-contract `release` call fails
        // or reverts (e.g. escrow paused, insufficient funds, already released),
        // the panic propagates and the whole transaction reverts atomically —
        // the confirmation is never marked settling while funds remain locked
        // (issue #261).
        if state.confs >= required {
            if let Some(ref escrow_address) = escrow_address {
                let escrow_client = PaymentEscrowClient::new(&env, escrow_address);
                // Panics (reverting the tx) if the release call fails.
                escrow_client.release(&payment_id);
                // Verify the release actually took effect before committing the
                // terminal state. A no-op or partial release must not be treated
                // as success.
                let payment = escrow_client.get_payment(&payment_id);
                assert!(
                    payment.status == PaymentEscrowStatus::Released,
                    "escrow release did not complete"
                );
            }

            // Only now — after a successful (or not-required) release — do we
            // persist the terminal settling state.
            state.settling = true;
            Self::save_payment_state(&env, &payment_id, &state);

            env.events().publish(
                ("settlement_authorised",),
                SettlementAuthorisedEvent {
                    payment_id: payment_id.clone(),
                    ledger_seq,
                    amount,
                    merchant,
                },
            );
        } else {
            Self::save_payment_state(&env, &payment_id, &state);
        }

        state.confs
    }

    // ── internal helpers ────────────────────────────────────────────────────

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(caller == &admin, "caller is not admin");
    }

    fn load_payment_state(env: &Env, payment_id: &BytesN<32>) -> PaymentConfirmationState {
        let key = DataKey::PaymentState(payment_id.clone());
        let state = env
            .storage()
            .persistent()
            .get::<_, PaymentConfirmationState>(&key)
            .unwrap_or(PaymentConfirmationState {
                confs: 0,
                first_ledger: 0,
                last_ledger: 0,
                required_confs: 0,
                settling: false,
                failed: false,
            });
        // Bump TTL on read so in-flight records survive slow confirmation (issue #260).
        env.storage().persistent().extend_ttl(
            &key,
            PAYMENT_STATE_TTL_THRESHOLD,
            PAYMENT_STATE_TTL_EXTEND_TO,
        );
        state
    }

    fn save_payment_state(env: &Env, payment_id: &BytesN<32>, state: &PaymentConfirmationState) {
        let key = DataKey::PaymentState(payment_id.clone());
        env.storage().persistent().set(&key, state);
        env.storage().persistent().extend_ttl(
            &key,
            PAYMENT_STATE_TTL_THRESHOLD,
            PAYMENT_STATE_TTL_EXTEND_TO,
        );
    }
}
