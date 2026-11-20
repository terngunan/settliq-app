#![no_std]

mod test;

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

const DEFAULT_MAX_SLIPPAGE_BPS: u32 = 100; // 1%

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    MaxSlippageBps,
    PairMaxSlippageBps(Address, Address),
}

#[contracttype]
struct SlippageExceededEvent {
    expected: i128,
    actual: i128,
    max_bps: u32,
}

#[contracttype]
struct MaxSlippageUpdatedEvent {
    old_bps: u32,
    new_bps: u32,
}

#[contracttype]
pub struct PairMaxSlippageUpdatedEvent {
    pub asset_a: Address,
    pub asset_b: Address,
    pub previous_bps: u32,
    pub new_bps: u32,
}

#[contracttype]
pub struct PairMaxSlippageRemovedEvent {
    pub asset_a: Address,
    pub asset_b: Address,
    pub removed_bps: u32,
}

#[contract]
pub struct SlippageProtectionContract;

#[contractimpl]
impl SlippageProtectionContract {
    pub fn __constructor(env: Env, admin: soroban_sdk::Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::MaxSlippageBps, &DEFAULT_MAX_SLIPPAGE_BPS);
    }

    /// Reverts if the actual rate deviates from expected by more than max_slippage_bps.
    /// expected and actual are prices in the same unit (e.g. stroops per USDC).
    pub fn check_slippage(env: Env, expected: i128, actual: i128) {
        let max_bps = Self::global_max_slippage(&env);
        Self::assert_within_tolerance(&env, expected, actual, max_bps);
    }

    /// Same as `check_slippage`, but uses the per-pair override for
    /// (asset_a, asset_b) when one has been set, falling back to the
    /// global `MaxSlippageBps` otherwise.
    ///
    /// The pair is unordered: (A, B) and (B, A) resolve to the same override.
    pub fn check_slippage_for_pair(
        env: Env,
        asset_a: soroban_sdk::Address,
        asset_b: soroban_sdk::Address,
        expected: i128,
        actual: i128,
    ) {
        let max_bps = Self::pair_max_slippage(&env, &asset_a, &asset_b);
        Self::assert_within_tolerance(&env, expected, actual, max_bps);
    }

    /// Admin: update the maximum allowed slippage in basis points.
    pub fn set_max_slippage(env: Env, caller: soroban_sdk::Address, bps: u32) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        if bps > 10_000 {
            panic!("bps must be <= 10000");
        }

        let old_bps = Self::global_max_slippage(&env);
        env.storage().instance().set(&DataKey::MaxSlippageBps, &bps);

        env.events().publish(
            ("SLIPPAGE_PROTECTION", "max_slippage_set"),
            MaxSlippageUpdatedEvent {
                old_bps,
                new_bps: bps,
            },
        );
    }

    /// Admin: set a per-pair override for the maximum allowed slippage in
    /// basis points. The pair is unordered: setting slippage for (A, B) is
    /// identical to setting it for (B, A), and lookups always resolve to the
    /// same entry regardless of argument order.
    ///
    /// Emits a `PairMaxSlippageUpdatedEvent` on topic
    /// `("SLIPPAGE", "pair_max_slippage_updated")` containing the asset pair
    /// and the previous and new bps values, enabling off-chain indexers and
    /// monitoring tools to audit every per-pair override change.
    pub fn set_pair_max_slippage(
        env: Env,
        caller: soroban_sdk::Address,
        asset_a: soroban_sdk::Address,
        asset_b: soroban_sdk::Address,
        bps: u32,
    ) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        if bps > 10_000 {
            panic!("bps must be <= 10000");
        }
        assert!(
            asset_a != asset_b,
            "asset_a and asset_b must be distinct"
        );
        let (k1, k2) = Self::canonical_pair(&env, asset_a.clone(), asset_b.clone());

        // Read the previous value (falls back to global default when no
        // override has been stored yet) so the event carries an accurate diff.
        let previous_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::PairMaxSlippageBps(k1.clone(), k2.clone()))
            .unwrap_or_else(|| Self::global_max_slippage(&env));

        env.storage()
            .instance()
            .set(&DataKey::PairMaxSlippageBps(k1, k2), &bps);

        env.events().publish(
            ("SLIPPAGE", "pair_max_slippage_updated"),
            PairMaxSlippageUpdatedEvent {
                asset_a,
                asset_b,
                previous_bps,
                new_bps: bps,
            },
        );
    }

    /// Admin-only: remove a per-pair slippage override to restore the pair to the global default.
    /// Returns the slippage bps value that was removed, or panics if no override exists.
    pub fn remove_pair_max_slippage(
        env: Env,
        caller: soroban_sdk::Address,
        asset_a: soroban_sdk::Address,
        asset_b: soroban_sdk::Address,
    ) -> u32 {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        let (k1, k2) = Self::canonical_pair(&env, asset_a.clone(), asset_b.clone());

        let key = DataKey::PairMaxSlippageBps(k1, k2);
        let removed_bps: u32 = env
            .storage()
            .instance()
            .get(&key)
            .expect("no pair max slippage override exists");

        env.storage().instance().remove(&key);

        env.events().publish(
            ("SLIPPAGE", "pair_max_slippage_removed"),
            PairMaxSlippageRemovedEvent {
                asset_a,
                asset_b,
                removed_bps,
            },
        );

        removed_bps
    }

    pub fn get_max_slippage(env: Env) -> u32 {
        Self::global_max_slippage(&env)
    }

    /// Returns the effective max slippage (bps) for the given pair: the
    /// per-pair override if one exists, otherwise the global default.
    pub fn get_pair_max_slippage(
        env: Env,
        asset_a: soroban_sdk::Address,
        asset_b: soroban_sdk::Address,
    ) -> u32 {
        Self::pair_max_slippage(&env, &asset_a, &asset_b)
    }

    pub fn get_admin(env: Env) -> soroban_sdk::Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    fn require_admin(env: &Env, caller: &soroban_sdk::Address) {
        let admin: soroban_sdk::Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if caller != &admin {
            panic!("Not admin");
        }
    }

    fn global_max_slippage(env: &Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::MaxSlippageBps)
            .unwrap_or(DEFAULT_MAX_SLIPPAGE_BPS)
    }

    /// Resolves the effective max slippage for a pair: the per-pair
    /// override when present, otherwise the global default.
    ///
    /// The lookup is commutative: (A, B) and (B, A) resolve to the same entry.
    fn pair_max_slippage(env: &Env, asset_a: &Address, asset_b: &Address) -> u32 {
        let (k1, k2) = Self::canonical_pair(env, asset_a.clone(), asset_b.clone());
        env.storage()
            .instance()
            .get(&DataKey::PairMaxSlippageBps(k1, k2))
            .unwrap_or_else(|| Self::global_max_slippage(env))
    }

    /// Return a canonical (deterministic) ordering of two addresses so that
    /// (A, B) and (B, A) always map to the same storage key.
    ///
    /// Ordering is derived from the XDR byte representation of each address
    /// via `soroban_sdk::xdr::ToXdr`, which produces a stable unique byte
    /// sequence for any two distinct addresses.
    fn canonical_pair(env: &Env, a: Address, b: Address) -> (Address, Address) {
        use soroban_sdk::xdr::ToXdr;
        let a_bytes = a.clone().to_xdr(env);
        let b_bytes = b.clone().to_xdr(env);
        if a_bytes <= b_bytes {
            (a, b)
        } else {
            (b, a)
        }
    }

    fn assert_within_tolerance(env: &Env, expected: i128, actual: i128, max_bps: u32) {
        if expected <= 0 {
            panic!("expected must be > 0");
        }
        if actual <= 0 {
            panic!("actual must be > 0");
        }

        let diff = if actual > expected {
            actual - expected
        } else {
            expected - actual
        };

        // diff / expected > max_bps / 10_000
        // => diff * 10_000 > max_bps * expected
        let lhs = diff * 10_000;
        let rhs = (max_bps as i128) * expected;

        if lhs > rhs {
            env.events().publish(
                ("SLIPPAGE_PROTECTION", "slippage_exceeded"),
                SlippageExceededEvent {
                    expected,
                    actual,
                    max_bps,
                },
            );
            panic!("slippage exceeded");
        }
    }
}
