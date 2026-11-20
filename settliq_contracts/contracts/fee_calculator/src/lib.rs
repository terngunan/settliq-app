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
    /// The returned `(fee, net, fee_bps)` values are computed against the
    /// merchant's current volume (the amount is *not* added to the window).
    pub fn quote_fee(env: Env, merchant: Address, amount: i128) -> (i128, i128, u32) {
        if amount <= 0 {
            panic!("amount must be > 0");
        }

        let volume = Self::current_volume(&env, &merchant);
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

    pub fn get_merchant_volume(env: Env, merchant: Address) -> i128 {
        Self::current_volume(&env, &merchant)
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not initialized");
        if &admin != caller {
            panic!("unauthorized: not admin");
        }
    }

    fn require_authorized_caller(env: &Env, caller: &Address) {
        let settlement_caller: Option<Address> = env
            .storage()
            .instance()
            .get(&DataKey::SettlementCaller);
        match settlement_caller {
            Some(authorized) => {
                if &authorized != caller {
                    panic!("unauthorized: not settlement caller");
                }
            }
            None => panic!("settlement caller not configured"),
        }
    }

    fn validate_tiers(tiers: &Vec<FeeTier>) {
        if tiers.is_empty() {
            panic!("tiers must not be empty");
        }
        let mut prev_threshold: i128 = -1;
        for tier in tiers.iter() {
            if tier.threshold_usdc < 0 {
                panic!("threshold must be >= 0");
            }
            if tier.threshold_usdc <= prev_threshold {
                panic!("thresholds must be strictly ascending");
            }
            if tier.fee_bps > BPS_DENOMINATOR as u32 {
                panic!("fee_bps must be <= 10000");
            }
            prev_threshold = tier.threshold_usdc;
        }
    }

    fn select_fee_bps(env: &Env, volume: i128) -> u32 {
        let tiers: Vec<FeeTier> = env
            .storage()
            .instance()
            .get(&DataKey::FeeTiers)
            .expect("tiers not initialized");

        let mut selected: u32 = tiers.get(0).unwrap().fee_bps;
        for tier in tiers.iter() {
            if volume >= tier.threshold_usdc {
                selected = tier.fee_bps;
            } else {
                break;
            }
        }
        selected
    }

    /// Builds an empty bucket ring anchored at `current_ledger`.
    fn empty_volume(env: &Env, current_ledger: u32) -> MerchantVolume {
        let mut buckets: Vec<i128> = Vec::new(env);
        let mut bucket_ledgers: Vec<u32> = Vec::new(env);
        let mut i: u32 = 0;
        while i < VOLUME_BUCKETS {
            buckets.push_back(0i128);
            bucket_ledgers.push_back(current_ledger);
            i += 1;
        }
        MerchantVolume {
            window_start_ledger: current_ledger,
            volume_usdc: 0,
            buckets,
            bucket_ledgers,
        }
    }

    /// Returns the merchant's current trailing 30-day volume without mutating
    /// storage. Stale buckets are ignored (treated as expired) but not cleared.
    fn current_volume(env: &Env, merchant: &Address) -> i128 {
        let current_ledger = env.ledger().sequence();
        let key = DataKey::MerchantVolume(merchant.clone());
        let data: Option<MerchantVolume> = env.storage().persistent().get(&key);
        match data {
            Some(d) => Self::prune_and_sum(env, &d, current_ledger),
            None => 0,
        }
    }

    /// Adds `amount` to the merchant's trailing volume and returns the updated
    /// trailing 30-day total.
    fn update_and_get_volume(env: &Env, merchant: &Address, amount: i128) -> i128 {
        let current_ledger = env.ledger().sequence();
        let key = DataKey::MerchantVolume(merchant.clone());

        let mut data: MerchantVolume = match env.storage().persistent().get(&key) {
            Some(d) => d,
            None => Self::empty_volume(env, current_ledger),
        };

        // Expire buckets that have fallen outside the trailing 30-day window.
        Self::prune_buckets(env, &mut data, current_ledger);

        // Determine the active bucket index for the current ledger.
        let elapsed = current_ledger.saturating_sub(data.window_start_ledger);
        let bucket_index = (elapsed / LEDGERS_PER_BUCKET) % VOLUME_BUCKETS;

        let existing: i128 = data.buckets.get(bucket_index).unwrap_or(0);
        let updated = existing.checked_add(amount).expect("volume overflow");
        data.buckets.set(bucket_index, updated);
        data.bucket_ledgers.set(bucket_index, current_ledger);

        let total = Self::prune_and_sum(env, &data, current_ledger);
        data.volume_usdc = total;

        env.storage().persistent().set(&key, &data);
        total
    }

    /// Clears any bucket whose last-write ledger is older than the trailing
    /// 30-day window relative to `current_ledger`.
    fn prune_buckets(env: &Env, data: &mut MerchantVolume, current_ledger: u32) {
        let mut i: u32 = 0;
        while i < VOLUME_BUCKETS {
            let last = data.bucket_ledgers.get(i).unwrap_or(0);
            let age = current_ledger.saturating_sub(last);
            if age >= LEDGERS_PER_30_DAYS {
                data.buckets.set(i, 0i128);
                data.bucket_ledgers.set(i, current_ledger);
            }
            i += 1;
        }
        let _ = env;
    }

    /// Sums the live buckets, ignoring any that have expired relative to
    /// `current_ledger`. Does not mutate `data`.
    fn prune_and_sum(env: &Env, data: &MerchantVolume, current_ledger: u32) -> i128 {
        let mut total: i128 = 0;
        let mut i: u32 = 0;
        while i < VOLUME_BUCKETS {
            let last = data.bucket_ledgers.get(i).unwrap_or(0);
            let age = current_ledger.saturating_sub(last);
            if age < LEDGERS_PER_30_DAYS {
                let bucket = data.buckets.get(i).unwrap_or(0);
                total = total.checked_add(bucket).expect("volume overflow");
            }
            i += 1;
        }
        let _ = env;
        total
    }
}
