//! Fee Distributor Contract
//!
//! Splits incoming USDC fees between the treasury and the LP reward pool
//! according to a configurable basis-points ratio.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec};

// ─── Storage keys ────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Treasury,
    LpAddress,
    LpShareBps,
    Token,
    AllowedCaller,
    AllowedTokens,
}

// ─── Events ──────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeDistributedEvent {
    pub token: Address,
    pub treasury_amount: i128,
    pub lp_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllowedCallerUpdatedEvent {
    pub old_caller: Address,
    pub new_caller: Address,
}

// ─── Contract ────────────────────────────────────────────────────────────────

#[contract]
pub struct FeeDistributorContract;

#[contractimpl]
impl FeeDistributorContract {
    pub fn __constructor(
        env: Env,
        admin: Address,
        treasury: Address,
        lp_address: Address,
        lp_share_bps: i128,
        token: Address,
        allowed_caller: Address,
    ) {
        admin.require_auth();
        assert!(lp_share_bps >= 0 && lp_share_bps <= 10_000, "invalid lp share");

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage().instance().set(&DataKey::LpAddress, &lp_address);
        env.storage().instance().set(&DataKey::LpShareBps, &lp_share_bps);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::AllowedCaller, &allowed_caller);

        let mut tokens: Vec<Address> = Vec::new(&env);
        tokens.push_back(token);
        env.storage().instance().set(&DataKey::AllowedTokens, &tokens);
    }

    // ─── Admin setters ───────────────────────────────────────────────────────

    pub fn set_lp_share(env: Env, caller: Address, lp_share_bps: i128) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        assert!(lp_share_bps >= 0 && lp_share_bps <= 10_000, "invalid lp share");
        env.storage().instance().set(&DataKey::LpShareBps, &lp_share_bps);
    }

    pub fn set_treasury(env: Env, caller: Address, treasury: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
    }

    pub fn set_lp_address(env: Env, caller: Address, lp_address: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        env.storage().instance().set(&DataKey::LpAddress, &lp_address);
    }

    pub fn set_allowed_caller(env: Env, caller: Address, allowed_caller: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        let old_caller: Address = env
            .storage()
            .instance()
            .get(&DataKey::AllowedCaller)
            .expect("allowed caller not set");
        env.storage().instance().set(&DataKey::AllowedCaller, &allowed_caller);
        env.events().publish(
            (Symbol::new(&env, "FEE_DISTRIBUTOR"), Symbol::new(&env, "allowed_caller_updated")),
            AllowedCallerUpdatedEvent { old_caller, new_caller: allowed_caller },
        );
    }

    pub fn transfer_admin(env: Env, caller: Address, new_admin: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        env.storage().instance().set(&DataKey::Admin, &new_admin);
    }

    // ─── Token allowlist ─────────────────────────────────────────────────────

    pub fn add_allowed_token(env: Env, caller: Address, token: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let mut allowed: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedTokens)
            .unwrap_or_else(|| Vec::new(&env));
        if !tokens.contains(&token) {
            tokens.push_back(token);
            env.storage().instance().set(&DataKey::AllowedTokens, &tokens);
        }
    }

    pub fn remove_allowed_token(env: Env, caller: Address, token: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let allowed: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedTokens)
            .unwrap_or_else(|| Vec::new(&env));
        let mut updated: Vec<Address> = Vec::new(&env);
        for t in tokens.iter() {
            if t != token {
                updated.push_back(t);
            }
        }
        env.storage().instance().set(&DataKey::AllowedTokens, &updated);
    }

    pub fn get_allowed_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::AllowedTokens)
            .unwrap_or_else(|| Vec::new(&env))
    }

    // ─── Distribution ────────────────────────────────────────────────────────

    pub fn distribute(env: Env, caller: Address, token: Address, amount: i128) {
        caller.require_auth();
        let allowed_caller: Address = env
            .storage()
            .instance()
            .get(&DataKey::AllowedCaller)
            .expect("allowed caller not set");
        assert!(caller == allowed_caller, "caller not allowed");

        let allowed_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedTokens)
            .unwrap_or_else(|| Vec::new(&env));
        assert!(allowed_tokens.contains(&token), "token not allowed");

        assert!(amount > 0, "amount must be positive");

        let lp_share_bps: i128 = env
            .storage()
            .instance()
            .get(&DataKey::LpShareBps)
            .expect("lp share not set");
        let treasury: Address = env
            .storage()
            .instance()
            .get(&DataKey::Treasury)
            .expect("treasury not set");
        let lp_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::LpAddress)
            .expect("lp address not set");

        let lp_amount = amount * lp_share_bps / 10_000;
        let treasury_amount = amount - lp_amount;

        let token_client = token::Client::new(&env, &token);
        if lp_amount > 0 {
            token_client.transfer(&caller, &lp_address, &lp_amount);
        }
        if treasury_amount > 0 {
            token_client.transfer(&caller, &treasury, &treasury_amount);
        }

        env.events().publish(
            (Symbol::new(&env, "FEE_DISTRIBUTOR"), Symbol::new(&env, "fee_distributed")),
            FeeDistributedEvent { token, treasury_amount, lp_amount },
        );
    }

    // ─── Helpers ─────────────────────────────────────────────────────────────

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set");
        assert!(caller == &admin, "caller is not admin");
    }
}
