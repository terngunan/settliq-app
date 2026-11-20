#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

/// Number of ledgers in the rolling 24h active-payment window.
///
/// Soroban targets ~5s ledger close times, so 24h ≈ 17_280 ledgers.
pub const ACTIVE_WINDOW_LEDGERS: u32 = 17_280;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformHealth {
    pub storage_ok: bool,
    pub stellar_ok: bool,
    pub partner_ok: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformStats {
    pub total_merchants: u32,
    pub total_payments: u32,
    pub total_settled_volume_usd: i128,
    pub active_payments_24h: u32,
    pub health: PlatformHealth,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerchantRecordedEvent {
    pub total_merchants: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentRecordedEvent {
    pub amount_usd: i128,
    pub settled: bool,
    pub total_payments: u32,
    pub total_settled_volume_usd: i128,
    pub active_payments_24h: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartnerHealthUpdatedEvent {
    pub partner_ok: bool,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    TotalMerchants,
    TotalPayments,
    TotalSettledVolume,
    ActiveBucket,
    PartnerOk,
    /// Last observed Stellar ledger sequence for health checks. Tracks ledger
    /// progression to detect network stalls or freezes.
    LastSeenLedgerSequence,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveBucket {
    pub bucket: u32,
    pub count: u32,
}

#[contract]
pub struct PlatformStatsContract;

#[contractimpl]
impl PlatformStatsContract {
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TotalMerchants, &0u32);
        env.storage().instance().set(&DataKey::TotalPayments, &0u32);
        env.storage()
            .instance()
            .set(&DataKey::TotalSettledVolume, &0i128);
        env.storage().instance().set(&DataKey::PartnerOk, &true);
    }

    pub fn record_merchant(env: Env, admin: Address) {
        Self::require_admin(&env, &admin);
        let total: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalMerchants)
            .unwrap_or(0);
        let new_total = total.saturating_add(1);
        env.storage()
            .instance()
            .set(&DataKey::TotalMerchants, &new_total);

        env.events().publish(
            (soroban_sdk::symbol_short!("merchant"),),
            MerchantRecordedEvent {
                total_merchants: new_total,
            },
        );
    }

    /// Admin-only: record a payment. If `settled`, adds to settled USD volume
    /// and increments the rolling 24h active-payments bucket.
    ///
    /// # Panics
    /// If `amount_usd` is not strictly positive (> 0) when `settled` is true.
    pub fn record_payment(env: Env, caller: Address, amount_usd: i128, settled: bool) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let tp: u32 = env.storage().instance().get(&DataKey::TotalPayments).unwrap();
        env.storage().instance().set(&DataKey::TotalPayments, &(tp + 1));

        if settled {
            // Validate amount is positive to prevent volume corruption from negative values
            if amount_usd <= 0 {
                panic!("amount_usd must be strictly positive when settled");
            }

            let vol: i128 = env
                .storage()
                .instance()
                .get(&DataKey::TotalSettledVolume)
                .unwrap();
            env.storage()
                .instance()
                .set(&DataKey::TotalSettledVolume, &(vol + amount_usd));
        }

        let total_payments: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalPayments)
            .unwrap_or(0);
        let new_total_payments = total_payments.saturating_add(1);
        env.storage()
            .instance()
            .set(&DataKey::TotalPayments, &new_total_payments);

        let total_volume: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSettledVolume)
            .unwrap_or(0);
        let new_total_volume = if settled {
            total_volume.saturating_add(amount_usd)
        } else {
            total_volume
        };
        env.storage()
            .instance()
            .set(&DataKey::TotalSettledVolume, &new_total_volume);

        let active = Self::bump_active(&env);

        env.events().publish(
            (soroban_sdk::symbol_short!("payment"),),
            PaymentRecordedEvent {
                amount_usd,
                settled,
                total_payments: new_total_payments,
                total_settled_volume_usd: new_total_volume,
                active_payments_24h: active,
            },
        );
    }

    pub fn set_partner_ok(env: Env, admin: Address, ok: bool) {
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::PartnerOk, &ok);

        env.events().publish(
            (soroban_sdk::symbol_short!("partner"),),
            PartnerHealthUpdatedEvent { partner_ok: ok },
        );
    }

    pub fn stats(env: Env) -> PlatformStats {
        let current_sequence = env.ledger().sequence();

        // Update the last seen sequence for health checks (detects network stalls)
        env.storage()
            .instance()
            .set(&DataKey::LastSeenLedgerSequence, &current_sequence);

        let bucket = current_sequence / ACTIVE_WINDOW_LEDGERS;
        let active: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalMerchants)
            .unwrap_or(0);
        let total_payments: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalPayments)
            .unwrap_or(0);
        let total_settled_volume_usd: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSettledVolume)
            .unwrap_or(0);
        let partner_ok: bool = env
            .storage()
            .instance()
            .get(&DataKey::PartnerOk)
            .unwrap_or(true);

        PlatformStats {
            total_merchants,
            total_payments,
            total_settled_volume_usd,
            active_payments_24h: Self::current_active(&env),
            health: PlatformHealth {
                storage_ok: true,
                stellar_ok: true,
                partner_ok,
            },
        }
    }

    fn require_admin(env: &Env, admin: &Address) {
        let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if &stored != admin {
            panic!("not admin");
        }
        admin.require_auth();
    }

    fn health(env: &Env) -> SystemHealth {
        let current_sequence = env.ledger().sequence();
        let last_seen: u32 = env
            .storage()
            .instance()
            .get(&DataKey::LastSeenLedgerSequence)
            .unwrap_or(current_sequence);

        // stellar_ok is true when the ledger is actively progressing (sequence has increased).
        // On first call or after contract reinitialization, this may be false for the first check,
        // but will become true once a subsequent call detects progression.
        let stellar_ok = current_sequence > last_seen;

        SystemHealth {
            storage_ok: env.storage().instance().has(&DataKey::Admin),
            stellar_ok,
            partner_ok: env
                .storage()
                .instance()
                .get(&DataKey::PartnerOk)
                .unwrap_or(true),
        }
    }

    fn bump_active(env: &Env) -> u32 {
        let bucket = Self::current_bucket(env);
        let count = match env
            .storage()
            .instance()
    

/* … truncated 357 chars — edit only what you need near the top … */
