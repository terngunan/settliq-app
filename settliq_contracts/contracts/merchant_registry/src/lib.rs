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

        // Append to the index in O(1): read the scalar count, write the new
        // address into its own slot, then bump the count. No prior merchant
        // entry is read or rewritten, and no single value grows with N.
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MerchantCount)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::MerchantAt(count), &merchant.clone());
        env.storage()
            .instance()
            .set(&DataKey::MerchantCount, &(count + 1));

        env.events().publish(
            ("REGISTRY", "merchant_registered"),
            MerchantRegisteredEvent { merchant: merchant.clone(), name: name.clone() },
        );
    }

    /// Update a registered merchant's business name. Callable by admin only.
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
            .expect("Merchant not found");

        if record.status == MerchantStatus::Terminated {
            panic!("Cannot modify terminated merchant");
        }

        let old_name = record.name.clone();
        record.name = name.clone();
        env.storage().persistent().set(&key, &record);

        env.events().publish(
            ("REGISTRY", "merchant_updated"),
            MerchantUpdatedEvent {
                merchant,
                old_name,
                name,
            },
        );
    }

    /// Suspend a merchant.  Callable by admin only.
    /// After suspension, the Escrow contract will reject new deposits for
    /// this merchant.
    pub fn suspend_merchant(env: Env, caller: Address, merchant: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::Merchant(merchant.clone());
        let mut record: MerchantRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Merchant not found");

        if record.status == MerchantStatus::Suspended {
            panic!("Merchant already suspended");
        }
        if record.status == MerchantStatus::Terminated {
            panic!("Merchant is terminated");
        }

        record.status = MerchantStatus::Suspended;
        env.storage().persistent().set(&key, &record);

        env.events().publish(
            ("REGISTRY", "merchant_suspended"),
            MerchantSuspendedEvent { merchant: merchant.clone() },
        );
    }

    /// Reactivate a previously suspended merchant.  Callable by admin only.
    pub fn reactivate_merchant(env: Env, caller: Address, merchant: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::Merchant(merchant.clone());
        let mut record: MerchantRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Merchant not found");

        if record.status == MerchantStatus::Active {
            panic!("Merchant already active");
        }
        if record.status == MerchantStatus::Terminated {
            panic!("Cannot reactivate terminated merchant");
        }
        record.status = MerchantStatus::Active;
        env.storage().persistent().set(&key, &record);

        env.events().publish(
            ("REGISTRY", "merchant_reactivated"),
            MerchantReactivatedEvent { merchant: merchant.clone() },
        );
    }

    /// Permanently terminate a merchant.  Callable by admin only.
    /// Unlike suspension, termination is irreversible: a terminated
    /// merchant can never be reactivated (see `reactivate_merchant`) or
    /// suspended again (see `suspend_merchant`).
    pub fn terminate_merchant(env: Env, caller: Address, merchant: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::Merchant(merchant.clone());
        let mut record: MerchantRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Merchant not found");

        if record.status == MerchantStatus::Terminated {
            panic!("Merchant already terminated");
        }

        record.status = MerchantStatus::Terminated;
        env.storage().persistent().set(&key, &record);

        env.events().publish(
            ("REGISTRY", "merchant_terminated"),
            MerchantTerminatedEvent { merchant: merchant.clone() },
        );
    }

    /// Update the KYC verification flag for a merchant.  Callable by admin only.
    pub fn set_kyc_status(env: Env, caller: Address, merchant: Address, verified: bool) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::Merchant(merchant.clone());
        let mut record: MerchantRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Merchant not found");

        record.kyc_verified = verified;
        env.storage().persistent().set(&key, &record);

        env.events().publish(
            ("REGISTRY", "kyc_status_updated"),
            KYCStatusUpdatedEvent { merchant: merchant.clone(), verified },
        );
    }

    /// Update the negotiated fee tier for a merchant.  Callable by admin only.
    pub fn update_fee_tier(env: Env, caller: Address, merchant: Address, fee_bps: u32) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        if fee_bps > MAX_FEE_BPS {
            panic!("Fee exceeds maximum");
        }

        let key = DataKey::Merchant(merchant.clone());
        let mut record: MerchantRecord = env
            .storage()
            .persistent()
            .get(&key)
            .expect("Merchant not found");

        if record.status == MerchantStatus::Terminated {
            panic!("Cannot modify terminated merchant");
        }

        record.fee_bps = fee_bps;
        env.storage().persistent().set(&key, &record);

        env.events().publish(
            ("REGISTRY", "fee_tier_updated"),
            FeeTierUpdatedEvent { merchant: merchant.clone(), fee_bps },
        );
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// Fetch a single merchant record.  Returns `None` if not registered.
    pub fn get_merchant(env: Env, merchant: Address) -> Option<MerchantRecord> {
        env.storage()
            .persistent()
            .get(&DataKey::Merchant(merchant))
    }

    /// Returns true if the merchant exists, is Active, and is KYC verified.
    pub fn is_approved(env: Env, merchant: Address) -> bool {
        match Self::get_merchant(env, merchant) {
            Some(record) => record.status == MerchantStatus::Active && record.kyc_verified,
            None => false,
        }
    }

    /// Return the total number of registered merchants.
    pub fn merchant_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::MerchantCount)
            .unwrap_or(0)
    }

    /// Paginated listing of active (non-terminated) registered merchants.
    ///
    /// The index slot at `DataKey::MerchantAt(i)` is never removed when a
    /// merchant is terminated — removing an arbitrary slot would require
    /// rewriting all higher-index slots, making termination O(N).  Instead
    /// this query reads each slot and filters out any address whose status is
    /// `Terminated`, so terminated merchants no longer appear in the returned
    /// page even though they remain in the underlying index.
    ///
    /// The page is filled with up to `page_size` *active* addresses, scanning
    /// forward through the raw index as needed.  Callers must use page numbers
    /// relative to the active subset, not the raw index.
    ///
    /// Cost in the worst case is bounded by a scan over all remaining index
    /// slots plus one persistent record read per slot.  For typical workloads
    /// (low termination rate) this is effectively O(page_size).
    pub fn merchants(env: Env, page: u32, page_size: u32) -> Vec<Address> {
        let mut result = Vec::new(&env);
        if page_size == 0 {
            return result;
        }

        // Cap the page size to prevent unbounded storage reads and gas exhaustion.
        let effective_page_size = page_size.min(MAX_PAGE_SIZE);

        let total: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MerchantCount)
            .unwrap_or(0);

        // Number of active merchants to skip before we start collecting.
        let skip = page.saturating_mul(effective_page_size);
        let mut skipped: u32 = 0;
        let mut i: u32 = 0;

        while i < total && (result.len() as u32) < effective_page_size {
            if let Some(addr) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::MerchantAt(i))
            {
                // Read the merchant record to inspect status.
                let is_terminated = env
                    .storage()
                    .persistent()
                    .get::<DataKey, MerchantRecord>(&DataKey::Merchant(addr.clone()))
                    .map(|r| r.status == MerchantStatus::Terminated)
                    .unwrap_or(false);

                if !is_terminated {
                    if skipped < skip {
                        skipped += 1;
                    } else {
                        result.push_back(addr);
                    }
                }
            }
            i += 1;
        }

        result
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
        if &admin != caller {
            panic!("Caller is not admin");
        }
    }
}
