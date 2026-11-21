#![no_std]

mod test;

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, String, Vec,
};

/// Lifecycle states for a registered merchant.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MerchantStatus {
    Active,
    Suspended,
    Terminated,
}

/// On-chain merchant record stored in Persistent storage.
#[contracttype]
#[derive(Clone, Debug)]
pub struct MerchantRecord {
    pub merchant: Address,
    pub name: String,
    pub status: MerchantStatus,
    pub kyc_verified: bool,
    /// Negotiated fee rate for this merchant, in basis points (1/100th of a
    /// percent). Defaults to `DEFAULT_FEE_BPS` at registration and can be
    /// overridden per-merchant by the admin via `update_fee_tier`.
    ///
    /// `0` is a valid, intentional fee rate (0%) and is distinct from an
    /// "unset" value: every registered merchant always has an explicit
    /// `fee_bps` (seeded with `DEFAULT_FEE_BPS` at registration), so a `0`
    /// read back from `get_fee_tier`/`get_merchant` means the admin
    /// deliberately set this merchant's rate to 0% via `update_fee_tier`
    /// (e.g. a VIP or promotional arrangement), not that no fee was ever
    /// configured.
    pub fee_bps: u32,
}



/// Default fee rate applied to newly registered merchants: 150 bps (1.5%).
const DEFAULT_FEE_BPS: u32 = 150;
/// Upper bound on the fee rate an admin can set for a merchant: 1000 bps (10%).
const MAX_FEE_BPS: u32 = 1000;
/// Maximum number of merchants returned in a single paginated query.
/// Prevents runaway storage reads / gas exhaustion from unbounded page sizes.
const MAX_PAGE_SIZE: u32 = 100;

/// Number of ledgers a merchant persistent entry is extended to live for on
/// each touch. At ~5s per ledger this is roughly 30 days, comfortably longer
/// than any operational gap between merchant interactions.
const MERCHANT_TTL_THRESHOLD: u32 = 100_000;
const MERCHANT_TTL_EXTEND_TO: u32 = 518_400;

/// Storage keys used by the registry.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Pending admin proposed via `propose_admin`, awaiting `accept_admin`.
    PendingAdmin,
    Merchant(Address),
    /// Index of all registered merchant addresses, for paginated listing.
    Merchants,
    /// Number of merchants registered so far. Stored as a single scalar so
    /// registration never has to read/rewrite the full merchant index.
    MerchantCount,
    /// Per-merchant index slot: maps a merchant's position to its address.
    /// Each entry is written independently, so registering merchant N does
    /// not touch any prior merchant's entry.
    MerchantAt(u32),
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[contracttype]
struct MerchantRegisteredEvent {
    merchant: Address,
    name: String,
}

#[contracttype]
struct MerchantSuspendedEvent {
    merchant: Address,
}

#[contracttype]
struct MerchantReactivatedEvent {
    merchant: Address,
}

#[contracttype]
struct KYCStatusUpdatedEvent {
    merchant: Address,
    verified: bool,
}

#[contracttype]
struct AdminTransferProposedEvent {
    current_admin: Address,
    pending_admin: Address,
}

#[contracttype]
struct AdminTransferredEvent {
    old_admin: Address,
    new_admin: Address,
}

#[contracttype]
struct FeeTierUpdatedEvent {
    merchant: Address,
    fee_bps: u32,
}

#[contracttype]
struct MerchantTerminatedEvent {
    merchant: Address,
}

#[contracttype]
struct MerchantUpdatedEvent {
    merchant: Address,
    old_name: String,
    name: String,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct MerchantRegistryContract;

#[contractimpl]
impl MerchantRegistryContract {
    // ------------------------------------------------------------------
    // Constructor
    // ------------------------------------------------------------------

    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::MerchantCount, &0u32);
    }

    // ------------------------------------------------------------------
    // TTL maintenance
    // ------------------------------------------------------------------

    /// Extend the TTL of a merchant's persistent record and its index slot so
    /// registered merchants are never archived out of contract state.
    fn bump_merchant_ttl(env: &Env, merchant: &Address, index: u32) {
        let merchant_key = DataKey::Merchant(merchant.clone());
        if env.storage().persistent().has(&merchant_key) {
            env.storage().persistent().extend_ttl(
                &merchant_key,
                MERCHANT_TTL_THRESHOLD,
                MERCHANT_TTL_EXTEND_TO,
            );
        }

        let index_key = DataKey::MerchantAt(index);
        if env.storage().persistent().has(&index_key) {
            env.storage().persistent().extend_ttl(
                &index_key,
                MERCHANT_TTL_THRESHOLD,
                MERCHANT_TTL_EXTEND_TO,
            );
        }
    }

    // ------------------------------------------------------------------
    // Admin – two-step handover
    // ------------------------------------------------------------------

    /// Propose a new admin. Callable by the current admin only.
    ///
    /// The proposal does not take effect until the proposed address calls
    /// `accept_admin`, so a typo'd or unreachable `new_admin` cannot lock the
    /// registry: the current admin stays in control and can re-propose (or
    /// correct) the pending address at any time.
    pub fn propose_admin(env: Env, caller: Address, new_admin: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);

        env.events().publish(
            ("REGISTRY", "admin_transfer_proposed"),
            AdminTransferProposedEvent {
                current_admin: caller,
                pending_admin: new_admin,
            },
        );
    }

    /// Complete a pending admin transfer. Callable by the pending admin only.
    ///
    /// Requires the pending admin's own `require_auth`, so the transfer only
    /// takes effect once the proposed address proves control of itself.
    pub fn accept_admin(env: Env) {
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .expect("No pending admin transfer");

        pending.require_auth();

        let old_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");

        env.storage().instance().set(&DataKey::Admin, &pending);
        env.storage().instance().remove(&DataKey::PendingAdmin);

        env.events().publish(
            ("REGISTRY", "admin_transferred"),
            AdminTransferredEvent {
                old_admin,
                new_admin: pending,
            },
        );
    }

    /// Read the pending admin, if a transfer has been proposed.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }

    /// Cancel an outstanding admin transfer proposal.  Callable by the
    /// current admin only.
    ///
    /// Removes `DataKey::PendingAdmin` from storage so the previously proposed
    /// address can no longer invoke `accept_admin`.  Emits an audit event so
    /// the cancellation is visible on-chain.
    pub fn cancel_admin_proposal(env: Env, caller: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        assert!(
            env.storage().instance().has(&DataKey::PendingAdmin),
            "No pending admin proposal"
        );
        env.storage().instance().remove(&DataKey::PendingAdmin);

        env.events().publish(
            ("REGISTRY", "admin_transfer_cancelled"),
            caller,
        );
    }

    /// Read the active admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set")
    }

    // ------------------------------------------------------------------
    // Admin – merchant lifecycle
    // ------------------------------------------------------------------

    /// Register a new merchant.  Callable by admin only.
    pub fn register_merchant(env: Env, caller: Address, merchant: Address, name: String) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        if name.len() == 0 {
            panic!("Merchant name cannot be empty");
        }

        let key = DataKey::Merchant(merchant.clone());
        if env.storage().persistent().has(&key) {
            panic!("Merchant already registered");
        }

        let record = MerchantRecord {
            merchant: merchant.clone(),
            name: name.clone(),
            status: MerchantStatus::Active,
            kyc_verified: false,
            fee_bps: DEFAULT_FEE_BPS,
        };

        env.storage().persistent().set(&key, &record);

        let index: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MerchantCount)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::MerchantAt(index), &merchant);
        env.storage()
            .instance()
            .set(&DataKey::MerchantCount, &(index + 1));

        Self::bump_merchant_ttl(&env, &merchant, index);

        env.events().publish(
            ("REGISTRY", "merchant_registered"),
            MerchantRegisteredEvent {
                merchant,
                name,
            },
        );
    }

    /// Look up a merchant record.  Extends the record's TTL on read so active
    /// lookups keep the merchant alive.
    pub fn get_merchant(env: Env, merchant: Address) -> Option<MerchantRecord> {
        let key = DataKey::Merchant(merchant.clone());
        let record: Option<MerchantRecord> = env.storage().persistent().get(&key);
        if record.is_some() {
            env.storage().persistent().extend_ttl(
                &key,
                MERCHANT_TTL_THRESHOLD,
                MERCHANT_TTL_EXTEND_TO,
            );
        }
        record
    }

    /// Check whether a merchant is approved (Active).  Extends the record's
    /// TTL on read so approval status survives long operational timelines.
    pub fn is_approved(env: Env, merchant: Address) -> bool {
        let key = DataKey::Merchant(merchant.clone());
        let record: Option<MerchantRecord> = env.storage().persistent().get(&key);
        match record {
            Some(r) => {
                env.storage().persistent().extend_ttl(
                    &key,
                    MERCHANT_TTL_THRESHOLD,
                    MERCHANT_TTL_EXTEND_TO,
                );
                r.status == MerchantStatus::Active
            }
            None => false,
        }
    }

    /// Suspend an active merchant.  Callable by admin only.
    pub fn suspend_merchant(env: Env, caller: Address, merchant: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::Merchant(merchant.clone());
        let mut record: MerchantRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Merchant not registered");

        record.status = MerchantStatus::Suspended;
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(
            &key,
            MERCHANT_TTL_THRESHOLD,
            MERCHANT_TTL_EXTEND_TO,
        );

        env.events().publish(
            ("REGISTRY", "merchant_suspended"),
            MerchantSuspendedEvent { merchant },
        );
    }

    /// Reactivate a suspended merchant.  Callable by admin only.
    pub fn reactivate_merchant(env: Env, caller: Address, merchant: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::Merchant(merchant.clone());
        let mut record: MerchantRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Merchant not registered");

        record.status = MerchantStatus::Active;
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(
            &key,
            MERCHANT_TTL_THRESHOLD,
            MERCHANT_TTL_EXTEND_TO,
        );

        env.events().publish(
            ("REGISTRY", "merchant_reactivated"),
            MerchantReactivatedEvent { merchant },
        );
    }

    /// Terminate a merchant.  Callable by admin only.
    pub fn terminate_merchant(env: Env, caller: Address, merchant: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::Merchant(merchant.clone());
        let mut record: MerchantRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Merchant not registered");

        record.status = MerchantStatus::Terminated;
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(
            &key,
            MERCHANT_TTL_THRESHOLD,
            MERCHANT_TTL_EXTEND_TO,
        );

        env.events().publish(
            ("REGISTRY", "merchant_terminated"),
            MerchantTerminatedEvent { merchant },
        );
    }

    /// Update a merchant's KYC verification flag.  Callable by admin only.
    pub fn set_kyc_status(env: Env, caller: Address, merchant: Address, verified: bool) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::Merchant(merchant.clone());
        let mut record: MerchantRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Merchant not registered");

        record.kyc_verified = verified;
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(
            &key,
            MERCHANT_TTL_THRESHOLD,
            MERCHANT_TTL_EXTEND_TO,
        );

        env.events().publish(
            ("REGISTRY", "kyc_status_updated"),
            KYCStatusUpdatedEvent { merchant, verified },
        );
    }

    /// Update a merchant's negotiated fee tier.  Callable by admin only.
    pub fn update_fee_tier(env: Env, caller: Address, merchant: Address, fee_bps: u32) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        if fee_bps > MAX_FEE_BPS {
            panic!("Fee rate exceeds maximum");
        }

        let key = DataKey::Merchant(merchant.clone());
        let mut record: MerchantRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Merchant not registered");

        record.fee_bps = fee_bps;
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(
            &key,
            MERCHANT_TTL_THRESHOLD,
            MERCHANT_TTL_EXTEND_TO,
        );

        env.events().publish(
            ("REGISTRY", "fee_tier_updated"),
            FeeTierUpdatedEvent { merchant, fee_bps },
        );
    }

    /// Read a merchant's negotiated fee tier, if registered.
    pub fn get_fee_tier(env: Env, merchant: Address) -> Option<u32> {
        let key = DataKey::Merchant(merchant.clone());
        let record: Option<MerchantRecord> = env.storage().persistent().get(&key);
        match record {
            Some(r) => {
                env.storage().persistent().extend_ttl(
                    &key,
                    MERCHANT_TTL_THRESHOLD,
                    MERCHANT_TTL_EXTEND_TO,
                );
                Some(r.fee_bps)
            }
            None => None,
        }
    }

    /// Update a merchant's display name.  Callable by admin only.
    pub fn update_merchant(env: Env, caller: Address, merchant: Address, name: String) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        if name.len() == 0 {
            panic!("Merchant name cannot be empty");
        }

        let key = DataKey::Merchant(merchant.clone());
        let mut record: MerchantRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Merchant not registered");

        let old_name = record.name.clone();
        record.name = name.clone();
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(
            &key,
            MERCHANT_TTL_THRESHOLD,
            MERCHANT_TTL_EXTEND_TO,
        );

        env.events().publish(
            ("REGISTRY", "merchant_updated"),
            MerchantUpdatedEvent {
                merchant,
                old_name,
                name,
            },
        );
    }

    /// List registered merchants with pagination.  Extends the TTL of each
    /// returned merchant record and its index slot.
    pub fn list_merchants(env: Env, start: u32, limit: u32) -> Vec<Address> {
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MerchantCount)
            .unwrap_or(0);

        let capped = if limit > MAX_PAGE_SIZE { MAX_PAGE_SIZE } else { limit };
        let mut out = Vec::new(&env);
        let mut i = start;
        while i < count && out.len() < capped {
            let index_key = DataKey::MerchantAt(i);
            if let Some(addr) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&index_key)
            {
                env.storage().persistent().extend_ttl(
                    &index_key,
                    MERCHANT_TTL_THRESHOLD,
                    MERCHANT_TTL_EXTEND_TO,
                );
                Self::bump_merchant_ttl(&env, &addr, i);
                out.push_back(addr);
            }
            i += 1;
        }
        out
    }

    /// Total number of registered merchants.
    pub fn merchant_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::MerchantCount)
            .unwrap_or(0)
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        assert!(caller == &admin, "Caller is not admin");
    }
}
