//! Platform statistics contract.
//!
//! Tracks aggregate operational metrics for the Settliq platform: merchant
//! count, payment count, settled volume, and a rolling 24h active-payment
//! counter. Only the admin may mutate stats.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol};

/// Number of ledgers in the rolling 24h active-payment window.
pub const ACTIVE_WINDOW_LEDGERS: u32 = 17_280;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    TotalMerchants,
    TotalPayments,
    TotalSettledVolumeUsd,
    ActiveBucket,
    PartnerOk,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveBucket {
    pub bucket: u32,
    pub count: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthStatus {
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
    pub health: HealthStatus,
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
            .set(&DataKey::TotalSettledVolumeUsd, &0i128);
        env.storage().instance().set(&DataKey::PartnerOk, &true);
    }

    fn require_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
    }

    fn current_bucket(env: &Env) -> u32 {
        env.ledger().sequence() / ACTIVE_WINDOW_LEDGERS
    }

    fn active_count(env: &Env) -> u32 {
        let bucket = Self::current_bucket(env);
        match env.storage().instance().get::<_, ActiveBucket>(&DataKey::ActiveBucket) {
            Some(ab) if ab.bucket == bucket => ab.count,
            _ => 0,
        }
    }

    pub fn record_merchant(env: Env, _caller: Address) {
        Self::require_admin(&env);
        let tm: u32 = env.storage().instance().get(&DataKey::TotalMerchants).unwrap();
        let new_total = tm.checked_add(1).expect("total merchants overflow");
        env.storage().instance().set(&DataKey::TotalMerchants, &new_total);

        env.events().publish(
            (Symbol::new(&env, "merchant"),),
            MerchantRecordedEvent {
                total_merchants: new_total,
            },
        );
    }

    pub fn record_payment(env: Env, _caller: Address, amount_usd: i128, settled: bool) {
        Self::require_admin(&env);

        let old_total_payments: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalPayments)
            .unwrap_or(0);
        let new_total_payments = old_total_payments
            .checked_add(1)
            .expect("total payments overflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalPayments, &new_total_payments);

        let mut new_volume: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSettledVolumeUsd)
            .unwrap_or(0);
        if settled {
            new_volume = new_volume.checked_add(amount_usd).expect("volume overflow");
            env.storage()
                .instance()
                .set(&DataKey::TotalSettledVolumeUsd, &new_volume);
        }

        let bucket = Self::current_bucket(&env);
        let active = match env.storage().instance().get::<_, ActiveBucket>(&DataKey::ActiveBucket) {
            Some(ab) if ab.bucket == bucket => ab.count,
            _ => 0,
        };
        let new_active = active.checked_add(1).expect("active payments overflow");
        env.storage().instance().set(
            &DataKey::ActiveBucket,
            &ActiveBucket {
                bucket,
                count: new_active,
            },
        );

        env.events().publish(
            (Symbol::new(&env, "payment"),),
            PaymentRecordedEvent {
                amount_usd,
                settled,
                total_payments: new_total_payments,
                total_settled_volume_usd: new_volume,
                active_payments_24h: new_active,
            },
        );
    }

    pub fn set_partner_ok(env: Env, _caller: Address, partner_ok: bool) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::PartnerOk, &partner_ok);
        env.events().publish(
            (Symbol::new(&env, "partner"),),
            PartnerHealthUpdatedEvent { partner_ok },
        );
    }

    pub fn stats(env: Env) -> PlatformStats {
        let total_merchants: u32 = env
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
            .get(&DataKey::TotalSettledVolumeUsd)
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
            active_payments_24h: Self::active_count(&env),
            health: HealthStatus {
                storage_ok: true,
                stellar_ok: true,
                partner_ok,
            },
        }
    }
}

#[cfg(test)]
mod test;
