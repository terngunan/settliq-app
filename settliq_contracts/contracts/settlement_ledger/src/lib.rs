#![no_std]

mod test;

use soroban_sdk::{contract, contractimpl, contracttype, vec, Address, BytesN, Env, String, Vec};

const PAGE_SIZE: u32 = 20;

/// Number of ledgers below which a persistent entry is considered "close to
/// expiry" and should be bumped. Standard threshold used across the ledger.
const TTL_THRESHOLD: u32 = 100_000;
/// Number of ledgers to extend a persistent entry to when bumping.
const TTL_EXTEND_TO: u32 = 500_000;

// ── types ────────────────────────────────────────────────────────────────────

/// Immutable on-chain settlement record.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SettlementRecord {
    pub payment_id: BytesN<32>,
    pub merchant: Address,
    pub amount: i128,   // gross amount paid
    pub fee: i128,      // platform fee deducted
    pub net: i128,      // amount remitted to merchant (amount - fee)
    pub timestamp: u64, // Unix timestamp supplied by the caller (NestJS)
    pub fiat_ref: String, // bank / fiat-rail reference ID
}

/// An append-only correction marker. The original settlement remains readable
/// and auditable; consumers must exclude voided records from financial totals.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SettlementVoid {
    pub payment_id: BytesN<32>,
    pub reason: String,
    pub voided_at: u64,
}

#[contracttype]
enum DataKey {
    Admin,
    /// SettlementRecord keyed by payment_id
    Settlement(BytesN<32>),
    /// Legacy unbounded merchant index. New records use page-sized index
    /// entries instead so appending does not rewrite a merchant's history.
    MerchantIndex(Address),
    /// Vec<BytesN<32>> — one bounded page of payment IDs for a merchant.
    MerchantPage(Address, u32),
    /// Number of settlement IDs stored for a merchant.
    MerchantCount(Address),
    /// Vec<BytesN<32>> — one bounded page of platform-wide settlement IDs.
    /// Replaces the former single unbounded `AllIndex` entry.
    AllPage(u32),
    /// Scalar count of all platform-wide settlements recorded so far.
    TotalCount,
    /// A void marker keyed by the original payment ID; never overwrites it.
    Void(BytesN<32>),
}

// ── events ───────────────────────────────────────────────────────────────────

#[contracttype]
struct SettlementRecordedEvent {
    payment_id: BytesN<32>,
    merchant: Address,
    amount: i128,
    fee: i128,
    net: i128,
    fiat_ref: String,
}

#[contracttype]
struct SettlementVoidedEvent {
    payment_id: BytesN<32>,
    reason: String,
    voided_at: u64,
}

#[contracttype]
struct AdminTransferredEvent {
    old_admin: Address,
    new_admin: Address,
}

// ── contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct SettlementLedgerContract;

#[contractimpl]
impl SettlementLedgerContract {
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Transfers authority to write future immutable settlement records.
    /// The current administrator must authorize the rotation.
    pub fn transfer_admin(env: Env, caller: Address, new_admin: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        let old_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set");
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.events().publish(
            ("SETTLEMENT_LEDGER", "admin_transferred"),
            AdminTransferredEvent {
                old_admin,
                new_admin,
            },
        );
    }

    /// Write an immutable settlement record. Admin-only (called by NestJS backend).
    /// Panics if a record for `payment_id` already exists — records are append-only.
    ///
    /// This contract is a trusted-write ledger: it validates that `fee + net`
    /// equals `amount`, but does not query a fee-calculator contract. The
    /// authorized backend is responsible for applying the merchant's configured
    /// fee policy before recording a settlement.
    pub fn record_settlement(
        env: Env,
        caller: Address,
        payment_id: BytesN<32>,
        merchant: Address,
        amount: i128,
        fee: i128,
        net: i128,
        timestamp: u64,
        fiat_ref: String,
    ) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        assert!(amount > 0, "amount must be > 0");
        assert!(fee >= 0, "fee must be >= 0");
        assert!(net >= 0, "net must be >= 0");
        assert!(fee + net == amount, "fee + net must equal amount");

        // Validate timestamp is reasonable (issue #75)
        let current_time = env.ledger().timestamp();
        assert!(timestamp > 0, "timestamp must not be zero");
        assert!(timestamp <= current_time, "timestamp must not be in the future");
        // Allow timestamps up to 90 days in the past for backdated settlements
        assert!(
            timestamp + (90 * 24 * 60 * 60) >= current_time,
            "timestamp too far in the past (> 90 days)"
        );

        let key = DataKey::Settlement(payment_id.clone());
        assert!(!env.storage().persistent().has(&key), "settlement already recorded");

        let record = SettlementRecord {
            payment_id: payment_id.clone(),
            merchant: merchant.clone(),
            amount,
            fee,
            net,
            timestamp,
            fiat_ref: fiat_ref.clone(),
        };

        env.storage().persistent().set(&key, &record);
        Self::bump_ttl(&env, &key);

        // Append to one bounded page so recording a settlement never rewrites
        // the merchant's full historical index.
        let count_key = DataKey::MerchantCount(merchant.clone());
        let count: u32 = env
            .storage()
            .persistent()
            .get(&count_key)
            .unwrap_or(0);
        let page = count / PAGE_SIZE;
        let page_key = DataKey::MerchantPage(merchant.clone(), page);
        let mut index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&page_key)
            .unwrap_or_else(|| vec![&env]);
        index.push_back(payment_id.clone());
        env.storage().persistent().set(&page_key, &index);
        Self::bump_ttl(&env, &page_key);
        env.storage().persistent().set(&count_key, &(count + 1));
        Self::bump_ttl(&env, &count_key);

        // Append to one bounded platform-wide page so recording a settlement
        // never rewrites the full global index.  Uses the same page-size
        // strategy as the per-merchant index above.
        let total_count_key = DataKey::TotalCount;
        let total_count: u32 = env
            .storage()
            .persistent()
            .get(&total_count_key)
            .unwrap_or(0);
        let all_page = total_count / PAGE_SIZE;
        let all_page_key = DataKey::AllPage(all_page);
        let mut all_index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&all_page_key)
            .unwrap_or_else(|| vec![&env]);
        all_index.push_back(payment_id.clone());
        env.storage().persistent().set(&all_page_key, &all_index);
        Self::bump_ttl(&env, &all_page_key);
        env.storage().persistent().set(&total_count_key, &(total_count + 1));
        Self::bump_ttl(&env, &total_count_key);

        env.events().publish(
            ("SETTLEMENT_LEDGER", "settlement_recorded"),
            SettlementRecordedEvent {
                payment_id,
                merchant,
                amount,
                fee,
                net,
                fiat_ref,
            },
        );
    }

    /// Fetch a single settlement record by payment_id. Callable by anyone.
    pub fn get_settlement(env: Env, payment_id: BytesN<32>) -> SettlementRecord {
        let key = DataKey::Settlement(payment_id);
        let record: SettlementRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("settlement not found");
        Self::bump_ttl(&env, &key);
        record
    }

    /// Returns `true` if the settlement identified by `payment_id` has been
    /// voided, `false` otherwise. Callable by anyone.
    pub fn is_voided(env: Env, payment_id: BytesN<32>) -> bool {
        let key = DataKey::Void(payment_id);
        let exists = env.storage().persistent().has(&key);
        if exists {
            Self::bump_ttl(&env, &key);
        }
        exists
    }

    /// Returns the void details (reason and timestamp) for a settlement, or
    /// `None` if the settlement has not been voided. Callable by anyone.
    pub fn get_void(env: Env, payment_id: BytesN<32>) -> Option<SettlementVoid> {
        let key = DataKey::Void(payment_id);
        let void: Option<SettlementVoid> = env.storage().persistent().get(&key);
        if void.is_some() {
            Self::bump_ttl(&env, &key);
        }
        void
    }

    /// Mark a settlement as voided without mutating the original record.
    /// Admin-only. Panics if the settlement does not exist or is already voided.
    pub fn void_settlement(
        env: Env,
        caller: Address,
        payment_id: BytesN<32>,
        reason: String,
    ) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let settlement_key = DataKey::Settlement(payment_id.clone());
        assert!(
            env.storage().persistent().has(&settlement_key),
            "settlement not found"
        );

        let void_key = DataKey::Void(payment_id.clone());
        assert!(
            !env.storage().persistent().has(&void_key),
            "settlement already voided"
        );

        let voided_at = env.ledger().timestamp();
        let void = SettlementVoid {
            payment_id: payment_id.clone(),
            reason: reason.clone(),
            voided_at,
        };
        env.storage().persistent().set(&void_key, &void);
        Self::bump_ttl(&env, &void_key);

        env.events().publish(
            ("SETTLEMENT_LEDGER", "settlement_voided"),
            SettlementVoidedEvent {
                payment_id,
                reason,
                voided_at,
            },
        );
    }

    // ── internal helpers ─────────────────────────────────────────────────────

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set");
        assert!(caller == &admin, "caller is not admin");
    }

    fn bump_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }
}
