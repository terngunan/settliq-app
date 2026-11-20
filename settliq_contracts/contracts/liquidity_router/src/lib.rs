#![no_std]

mod test;

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol, Vec};

// Issue #1082: Upper bound on the approved pool allowlist to keep
// add_pool/remove_pool/check_and_route from scaling without limit.
const MAX_POOLS: u32 = 100;

// Issue #1085: Default AMM depth threshold (10% impact) in basis points.
const DEFAULT_DEPTH_THRESHOLD_BPS: u32 = 1000;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    SorobanAMM,
    StellarClassicDEX,
}

// Issue #1025: Add admin-managed pool allowlist
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    ApprovedPools,
    // Issue #1085: Admin-configurable AMM depth threshold in basis points
    DepthThresholdBps,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct FallbackRouteUsed {
    pub pool_id: Address,
    pub amount: i128,
    pub reserve: i128,
}

// Issue #1083: Event emitted when a pool is added to the allowlist
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PoolAddedEvent {
    pub pool_id: Address,
}

// Issue #1083: Event emitted when a pool is removed from the allowlist
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PoolRemovedEvent {
    pub pool_id: Address,
}

// Issue #68: Event emitted when AMM depth threshold is updated
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct DepthThresholdUpdatedEvent {
    pub new_threshold_bps: u32,
}

// Issue #224: Event emitted when admin ownership is transferred
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AdminTransferred {
    pub old_admin: Address,
    pub new_admin: Address,
}

#[soroban_sdk::contractclient(name = "AmmClient")]
pub trait AmmInterface {
    fn get_reserves(env: Env) -> (i128, i128);
}

#[contract]
pub struct LiquidityRouter;

#[contractimpl]
impl LiquidityRouter {
    // Issue #1025: Constructor to initialize admin atomically at deployment time.
    // Issue #1079: Using __constructor eliminates the front-running window that
    // existed when initialize() was a separate post-deploy call: an attacker
    // could have submitted their own initialize() transaction between the deploy
    // and the legitimate initialize() call, seizing the admin role.
    // Issue #1080: Store config singletons in instance() storage so their TTL
    // is bumped automatically with every contract call.
    pub fn __constructor(env: Env, admin: Address) {
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        let pools: Vec<Address> = Vec::new(&env);
        env.storage().instance().set(&DataKey::ApprovedPools, &pools);
        // Issue #1085: Seed the depth threshold with the previous hardcoded default
        env.storage().instance().set(&DataKey::DepthThresholdBps, &DEFAULT_DEPTH_THRESHOLD_BPS);
    }

    // Issue #1025: Add pool to allowlist (admin-gated)
    pub fn add_pool(env: Env, admin: Address, pool: Address) {
        admin.require_auth();

        // Verify caller is the admin
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .expect("admin not set");
        if admin != stored_admin {
            panic!("only admin can add pools");
        }

        let mut pools: Vec<Address> = env.storage().instance().get(&DataKey::ApprovedPools)
            .unwrap_or_else(|| Vec::new(&env));

        // Check for duplicates (issue #71 - fail instead of silently succeeding)
        if pools.iter().any(|p| p == pool) {
            panic!("pool is already allowlisted");
        }

        // Issue #1082: Enforce a maximum allowlist size before inserting
        if pools.len() >= MAX_POOLS {
            panic!("max pools reached");
        }

        pools.push_back(pool.clone());
        env.storage().instance().set(&DataKey::ApprovedPools, &pools);

        // Issue #1083: Emit event for the allowlist change
        env.events().publish(
            (Symbol::new(&env, "PoolAddedEvent"),),
            PoolAddedEvent { pool_id: pool },
        );
    }

    // Issue #1025: Remove pool from allowlist (admin-gated)
    pub fn remove_pool(env: Env, admin: Address, pool: Address) {
        admin.require_auth();

        // Verify caller is the admin
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .expect("admin not set");
        if admin != stored_admin {
            panic!("only admin can remove pools");
        }

        let mut pools: Vec<Address> = env.storage().instance().get(&DataKey::ApprovedPools)
            .unwrap_or_else(|| Vec::new(&env));

        // Remove the pool if it exists
        let mut new_pools = Vec::new(&env);
        let mut removed = false;
        for p in pools.iter() {
            if p != pool {
                new_pools.push_back(p);
            } else {
                removed = true;
            }
        }
        env.storage().instance().set(&DataKey::ApprovedPools, &new_pools);

        // Issue #1083: Emit event only when a pool was actually removed
        if removed {
            env.events().publish(
                (Symbol::new(&env, "PoolRemovedEvent"),),
                PoolRemovedEvent { pool_id: pool },
            );
        }
    }

    // Issue #1085: Admin-gated setter for the AMM depth threshold (in bps)
    pub fn set_depth_threshold_bps(env: Env, admin: Address, threshold_bps: u32) {
        admin.require_auth();

        // Verify caller is the admin
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .expect("admin not set");
        if admin != stored_admin {
            panic!("only admin can set depth threshold");
        }

        if threshold_bps == 0 || threshold_bps > 10_000 {
            panic!("invalid depth threshold");
        }

        env.storage().instance().set(&DataKey::DepthThresholdBps, &threshold_bps);

        // Emit event for audit trail (issue #68)
        env.events().publish(
            ("LIQUIDITY_ROUTER", "depth_threshold_updated"),
            DepthThresholdUpdatedEvent {
                new_threshold_bps: threshold_bps,
            },
        );
    }

    pub fn get_depth_threshold_bps(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::DepthThresholdBps)
            .unwrap_or(DEFAULT_DEPTH_THRESHOLD_BPS)
    }

    /// Returns the current admin address (issue #70)
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set")
    }

    /// Transfers admin privileges to a new address (issue #70)
    pub fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) {
        current_admin.require_auth();

        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .expect("admin not set");
        if current_admin != stored_admin {
            panic!("only admin can transfer admin");
        }

        // Issue #225: Reject transferring admin to the contract itself, which
        // would permanently lock admin capabilities since the contract cannot
        // produce external signatures for require_auth().
        assert!(
            new_admin != env.current_contract_address(),
            "cannot transfer admin to contract"
        );
        // Issue #225: Reject no-op re-assignment to the current admin.
        assert!(new_admin != current_admin, "new admin must be different");

        env.storage().instance().set(&DataKey::Admin, &new_admin);

        // Issue #224: Emit audit event for the ownership change
        env.events().publish(
            (Symbol::new(&env, "AdminTransferred"),),
            AdminTransferred {
                old_admin: stored_admin,
                new_admin,
            },
        );
    }

    /// Returns the list of approved pools (issue #69)
    pub fn get_approved_pools(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::ApprovedPools)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Checks if the AMM pool has sufficient depth for a swap.
    /// Returns SorobanAMM if depth is sufficient (impact below the configured
    /// threshold), otherwise returns StellarClassicDEX and emits an event.
    ///
    /// Issue #223: `is_token_a` selects which reserve (token_a or token_b) the
    /// incoming amount is measured against, so the depth check reflects the
    /// actual swap direction.
    pub fn check_and_route(
        env: Env,
        pool_address: Address,
        amount_in: i128,
        is_token_a: bool,
    ) -> Route {
        // Issue #46: Reject zero and negative input amounts immediately.
        if amount_in <= 0 {
            panic!("amount must be positive");
        }

        // Issue #1025: Only allow routing through allowlisted pools
        let pools: Vec<Address> = env.storage().instance().get(&DataKey::ApprovedPools)
            .unwrap_or_else(|| Vec::new(&env));
        if !pools.iter().any(|p| p == pool_address) {
            panic!("pool not allowlisted");
        }

        // Issue #1086: Query AMM reserves via cross-contract call using the
        // generated AmmClient.
        let amm_client = AmmClient::new(&env, &pool_address);
        let (reserve_a, reserve_b) = amm_client.get_reserves();

        // Issue #23: Select the relevant reserve based on swap direction.
        let relevant_reserve = if is_token_a { reserve_a } else { reserve_b };

        if relevant_reserve <= 0 {
            panic!("pool has no liquidity");
        }

        // Issue #1085: Compare price impact against the admin-configured threshold.
        let threshold_bps = Self::get_depth_threshold_bps(env.clone());
        let impact_bps = (amount_in * 10_000) / relevant_reserve;

        if impact_bps > threshold_bps as i128 {
            env.events().publish(
                (Symbol::new(&env, "FallbackRouteUsed"),),
                FallbackRouteUsed {
                    pool_id: pool_address,
                    amount: amount_in,
                    reserve,
                },
            );
            Route::StellarClassicDEX
        } else {
            Route::SorobanAMM
        }
    }
}
