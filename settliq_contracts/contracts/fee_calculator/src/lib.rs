#![no_std]

mod test;

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec};

const LEDGERS_PER_30_DAYS: u32 = 172_800;
/// Number of sub-window buckets used to approximate a rolling 30-day window.
/// Each bucket covers `LEDGERS_PER_30_DAYS / VOLUME_BUCKETS` ledgers (5 days).
const VOLUME_BUCKETS: u32 = 6;
/// Ledgers covered by a single sub-window bucket (5 days).
const LEDGERS_PER_BUCKET: u32 = LEDGERS_PER_30_DAYS / VOLUME_BUCKETS;
const BPS_DENOMINATOR: i128 = 10_000;
/// Upper sanity bound for the configured minimum fee (100_000_000 stroops,
/// i.e. $10.00 equivalent). Prevents an accidental typo from clamping the fee
/// to the full payment amount and confiscating merchant proceeds.
const MAX_MIN_FEE_STROOPS: i128 = 100_000_000;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct FeeTier {
    pub threshold_usdc: i128,
    pub fee_bps: u32,
}

/// Trailing 30-day volume tracked as a ring of `VOLUME_BUCKETS` sub-windows.
///
/// Instead of a single tumbling window that resets to zero every 30 days,
/// volume is accumulated into `VOLUME_BUCKETS` buckets of
/// `LEDGERS_PER_BUCKET` ledgers each. As time advances, buckets that fall
/// outside the trailing 30-day span are cleared, so the reported volume
/// decays smoothly (one bucket at a time) rather than cliff-resetting.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MerchantVolume {
    /// Ledger at which the current bucket ring was started.
    pub window_start_ledger: u32,
    /// Total trailing volume across all live buckets (USDC).
    pub volume_usdc: i128,
    /// Per-bucket volume, indexed by `(bucket_index % VOLUME_BUCKETS)`.
    pub buckets: Vec<i128>,
    /// Ledger at which each bucket was last written, used to expire stale
    /// buckets. Indexed the same way as `buckets`.
    pub bucket_ledgers: Vec<u32>,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    SettlementCaller,
    Admin,
    FeeTiers,
    MinFeeStroops,
    MerchantVolume(Address),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TierAppliedEvent {
    pub merchant: Address,
    pub volume_usdc: i128,
    pub fee_bps: u32,
    pub fee: i128,
    pub net: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct FeeTiersUpdatedEvent {
    pub admin: Address,
    pub tiers: Vec<FeeTier>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SettlementCallerUpdatedEvent {
    pub caller: Address,
    pub settlement_caller: Address,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MinFeeUpdatedEvent {
    pub caller: Address,
    pub old_min_fee_stroops: i128,
    pub new_min_fee_stroops: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AdminTransferredEvent {
    pub caller: Address,
    pub new_admin: Address,
}

#[contract]
pub struct FeeCalculatorContract;

#[contractimpl]
impl FeeCalculatorContract {
    pub fn __constructor(env: Env, admin: Address, tiers: Vec<FeeTier>) {
        Self::validate_tiers(&tiers);
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::FeeTiers, &tiers);
        env.storage().instance().set(&DataKey::MinFeeStroops, &0i128);
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not initialized")
    }

    pub fn transfer_admin(env: Env, caller: Address, new_admin: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        if caller == new_admin {
            panic!("cannot transfer to self");
        }
        env.storage().instance().set(&DataKey::Admin, &new_admin);

        env.events().publish(
            ("FEE_CALC", "admin_transferred"),
            AdminTransferredEvent {
                caller,
                new_admin,
            },
        );
    }

    pub fn set_fee_tiers(env: Env, caller: Address, tiers: Vec<FeeTier>) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        Self::validate_tiers(&tiers);
        env.storage().instance().set(&DataKey::FeeTiers, &tiers);

        env.events().publish(
            ("FEE", "tiers_updated"),
            FeeTiersUpdatedEvent {
                admin: caller,
                tiers,
            },
        );
    }

    pub fn set_settlement_caller(env: Env, caller: Address, settlement_caller: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        env.storage()
            .instance()
            .set(&DataKey::SettlementCaller, &settlement_caller);

        env.events().publish(
            ("FEE", "settlement_caller_updated"),
            SettlementCallerUpdatedEvent {
                caller,
                settlement_caller,
            },
        );
    }

    pub fn get_settlement_caller(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::SettlementCaller)
    }

    pub fn set_min_fee_stroops(env: Env, caller: Address, min_fee_stroops: i128) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        if min_fee_stroops < 0 || min_fee_stroops > MAX_MIN_FEE_STROOPS {
            panic!("min_fee_stroops out of range");
        }
        let old_min_fee_stroops: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MinFeeStroops)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::MinFeeStroops, &min_fee_stroops);

        env.events().publish(
            ("FEE", "min_fee_updated"),
            MinFeeUpdatedEvent {
                caller,
                old_min_fee_stroops,
                new_min_fee_stroops: min_fee_stroops,
            },
        );
    }

    pub fn get_min_fee_stroops(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MinFeeStroops)
            .unwrap_or(0)
    }

    pub fn get_fee_tiers(env: Env) -> Vec<FeeTier> {
        // `__constructor` always validates and stores `FeeTiers`, so for any
        // live contract instance the tiers are guaranteed to be present.
        env.storage()
            .instance()
            .get(&DataKey::FeeTiers)
            .expect("tiers not initialized")
    }

    pub fn calculate_fee(
        env: Env,
        caller: Address,
        merchant: Address,
        amount: i128,
    ) -> (i128, i128, u32) {
        caller.require_auth();
        Self::require_authorized_caller(&env, &caller);

        if amount <= 0 {
            panic!("amount must be > 0");
        }

        let volume = Self::update_and_get_volume(&env, &merchant, amount);
        let fee_bps = Self::select_fee_bps(&env, volume);

        let mut fee = amount
            .checked_mul(fee_bps as i128)
            .expect("overflow")
            .checked_div(BPS_DENOMINATOR)
            .expect("division failure");

        let min_fee_stroops: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MinFeeStroops)
            .unwrap_or(0);
        if fee < min_fee_stroops {
            fee = min_fee_stroops;
        }
        if fee > amount {
            fee = amount;
        }

        let net = amount.checked_sub(fee).expect("underflow");

        env.events().publish(
            ("FEE", "tier_applied"),
            TierAppliedEvent {
                merchant,
                volume_usdc: volume,
                fee_bps,
                fee,
                net,
            },
        );

        (fee, net, fee_bps)
    }

    /// Read-only quote: returns the fee breakdown for `amount` using the
    /// merchant's current trailing 30-day volume, without mutating any
    /// storage (issue #61).
    ///
    /// Safe for permissionless use by payment frontends and checkout APIs
    /// that need to display an estimated fee before committing to a settlement.
    /// The returned `(fee, net, fee_bps)` values are identical to what
    /// `calculate_fee` would produce for the same inputs, but no volume
    /// accumulation occurs.
    pub fn quote_fee(env: Env, merchant: Address, amount: i128) -> (i128, i128, u32) {
        if amount <= 0 {
            panic!("amount must be > 0");
        }

        // Read current trailing volume without mutating storage.
        let volume = Self::get_merchant_volume(env.clone(), merchant).volume_usdc;
        let fee_bps = Self::select_fee_bps(&env, volume);

        let mut fee = amount
            .checked_mul(fee_bps as i128)
            .expect("overflow")
            .checked_div(BPS_DENOMINATOR)
            .expect("division failure");

        let min_fee_stroops: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MinFeeStroops)
            .unwrap_or(0);
        if fee < min_fee_stroops {
            fee = min_fee_stroops;
        }
        if fee > amount {
            fee = amount;
        }

        let net = amount.checked_sub(fee).expect("underflow");

        (fee, net, fee_bps)
    }

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not initialized");
        if &admin != caller {
            panic!("unauthorized: caller is not admin");
        }
    }

    fn require_authorized_caller(env: &Env, caller: &Address) {
        let settlement_caller: Address = env
            .storage()
            .instance()
            .get(&DataKey::SettlementCaller)
            .expect("settlement caller not configured");
        if &settlement_caller != caller {
            panic!("unauthorized: caller is not settlement caller");
        }
    }

    fn validate_tiers(tiers: &Vec<FeeTier>) {
        if tiers.is_empty() {
            panic!("tiers must not be empty");
        }
        let mut prev_threshold: i128 = -1;
        for tier in tiers.iter() {
            if tier.threshold_usdc <= prev_threshold {
                panic!("tiers must be strictly ascending");
            }
            if tier.fee_bps > BPS_DENOMINATOR as u32 {
                panic!("fee_bps exceeds 100%");
            }
            prev_threshold = tier.threshold_usdc;
        }
    }

    fn select_fee_bps(env: &Env, volume_usdc: i128) -> u32 {
        let tiers: Vec<FeeTier> = env
            .storage()
            .instance()
            .get(&DataKey::FeeTiers)
            .expect("tiers not initialized");
        let mut selected: u32 = 0;
        for tier in tiers.iter() {
            if volume_usdc >= tier.threshold_usdc {
                selected = tier.fee_bps;
            } else {
                break;
            }
        }
        selected
    }

    fn get_merchant_volume(env: Env, merchant: Address) -> MerchantVolume {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantVolume(merchant))
            .unwrap_or(MerchantVolume {
                window_start_ledger: env.ledger().sequence(),
                volume_usdc: 0,
                buckets: Vec::new(&env),
                bucket_ledgers: Vec::new(&env),
            })
    }

    fn update_and_get_volume(env: &Env, merchant: &Address, amount: i128) -> i128 {
        let key = DataKey::MerchantVolume(merchant.clone());
        let current_ledger = env.ledger().sequence();
        let mut volume: MerchantVolume = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(MerchantVolume {
                window_start_ledger: current_ledger,
                volume_usdc: 0,
                buckets: Vec::new(env),
                bucket_ledgers: Vec::new(env),
            });

        // Initialize bucket ring if empty.
        if volume.buckets.is_empty() {
            let mut buckets = Vec::new(env);
            let mut ledgers = Vec::new(env);
            for _ in 0..VOLUME_BUCKETS {
                buckets.push_back(0i128);
                ledgers.push_back(0u32);
            }
            volume.buckets = buckets;
            volume.bucket_ledgers = ledgers;
            volume.window_start_ledger = current_ledger;
        }

        // Expire stale buckets and recompute the live total.
        let mut total: i128 = 0;
        let mut buckets = Vec::new(env);
        let mut ledgers = Vec::new(env);
        for i in 0..VOLUME_BUCKETS {
            let bucket_volume = volume.buckets.get(i).unwrap_or(0);
            let bucket_ledger = volume.bucket_ledgers.get(i).unwrap_or(0);
            let expired = bucket_ledger != 0
                && current_ledger.saturating_sub(bucket_ledger) >= LEDGERS_PER_30_DAYS;
            if expired {
                buckets.push_back(0i128);
                ledgers.push_back(0u32);
            } else {
                buckets.push_back(bucket_volume);
                ledgers.push_back(bucket_ledger);
                total = total.checked_add(bucket_volume).expect("overflow");
            }
        }

        // Write into the current bucket.
        let bucket_index = ((current_ledger.saturating_sub(volume.window_start_ledger)
            / LEDGERS_PER_BUCKET)
            % VOLUME_BUCKETS) as u32;
        let existing = buckets.get(bucket_index).unwrap_or(0);
        let updated = existing.checked_add(amount).expect("overflow");
        buckets.set(bucket_index, updated);
        ledgers.set(bucket_index, current_ledger);
        total = total.checked_add(amount).expect("overflow");

        volume.volume_usdc = total;
        volume.buckets = buckets;
        volume.bucket_ledgers = ledgers;
        env.storage().persistent().set(&key, &volume);

        total
    }
}
