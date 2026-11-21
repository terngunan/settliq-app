#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Vec};

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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeDistributedEvent {
    pub token: Address,
    pub treasury_amount: i128,
    pub lp_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminTransferredEvent {
    pub old_admin: Address,
    pub new_admin: Address,
}

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
        assert!(
            !env.storage().instance().has(&DataKey::Admin),
            "already initialized"
        );
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

    pub fn distribute(env: Env, caller: Address, token: Address, amount: i128) {
        caller.require_auth();
        let allowed_caller: Address = env.storage().instance().get(&DataKey::AllowedCaller).unwrap();
        assert!(caller == allowed_caller, "unauthorized caller");
        assert!(amount > 0, "amount must be positive");

        let allowed_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedTokens)
            .unwrap_or(Vec::new(&env));
        assert!(allowed_tokens.contains(&token), "token not allowed");

        let treasury: Address = env.storage().instance().get(&DataKey::Treasury).unwrap();
        let lp_address: Address = env.storage().instance().get(&DataKey::LpAddress).unwrap();
        let lp_share_bps: i128 = env.storage().instance().get(&DataKey::LpShareBps).unwrap();

        let lp_amount = amount * lp_share_bps / 10_000;
        let treasury_amount = amount - lp_amount;

        // Fees are held by the distributor contract, so transfers must originate
        // from the contract address rather than the caller.
        let contract_address = env.current_contract_address();
        let token_client = token::Client::new(&env, &token);
        if lp_amount > 0 {
            token_client.transfer(&contract_address, &lp_address, &lp_amount);
        }
        if treasury_amount > 0 {
            token_client.transfer(&contract_address, &treasury, &treasury_amount);
        }

        env.events().publish(
            (soroban_sdk::symbol_short!("fee_dist"),),
            FeeDistributedEvent { token, treasury_amount, lp_amount },
        );
    }

    pub fn transfer_admin(env: Env, caller: Address, new_admin: Address) {
        caller.require_auth();
        let old_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(caller == old_admin, "unauthorized");
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.events().publish(
            (
                soroban_sdk::symbol_short!("FEE_DISTRIBUTOR"),
                soroban_sdk::symbol_short!("admin_transferred"),
            ),
            AdminTransferredEvent { old_admin, new_admin },
        );
    }

    pub fn add_allowed_token(env: Env, admin: Address, token: Address) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(admin == stored_admin, "unauthorized");
        let mut allowed: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedTokens)
            .unwrap_or(Vec::new(&env));
        if !allowed.contains(&token) {
            allowed.push_back(token);
            env.storage().instance().set(&DataKey::AllowedTokens, &allowed);
        }
    }

    pub fn remove_allowed_token(env: Env, admin: Address, token: Address) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        assert!(admin == stored_admin, "unauthorized");
        let allowed: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedTokens)
            .unwrap_or(Vec::new(&env));
        let mut updated: Vec<Address> = Vec::new(&env);
        for t in allowed.iter() {
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
            .unwrap_or(Vec::new(&env))
    }
}
