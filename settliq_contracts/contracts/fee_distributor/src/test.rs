#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events as _, MockAuth, MockAuthInvoke},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal, Symbol,
};

use crate::{FeeDistributorContract, FeeDistributorContractClient};

struct Setup<'a> {
    env: Env,
    admin: Address,
    treasury: Address,
    lp: Address,
    caller: Address,
    token: TokenClient<'a>,
    token_address: Address,
    client: FeeDistributorContractClient<'a>,
}

fn setup(lp_share_bps: i128) -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let lp = Address::generate(&env);
    let caller = Address::generate(&env);

    // Deploy a test token (Stellar Asset Contract) — used as USDC stand-in.
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    let sac = StellarAssetClient::new(&env, &token_id.address());

    let contract_id = env.register(
        FeeDistributorContract,
        (&admin, &treasury, &lp, lp_share_bps, &token_id.address(), &caller),
    );

    // Mint 1_000_000 to the distributor contract so distribute() can transfer
    // from the contract's own balance (issue #221).
    sac.mint(&contract_id, &1_000_000);

    let client = FeeDistributorContractClient::new(&env, &contract_id);
    let token = TokenClient::new(&env, &token_id.address());
    let token_address = token_id.address();

    Setup { env, admin, treasury, lp, caller, token, token_address, client }
}

// ─── Re-initialization guard (issue #220) ────────────────────────────────────

/// A second call to `initialize` must revert with "already initialized" so an
/// external actor cannot overwrite Admin/Treasury/LpAddress/AllowedCaller.
#[test]
#[should_panic(expected = "already initialized")]
fn test_cannot_reinitialize() {
    let s = setup(5_000);

    // Attacker attempts to re-initialize with their own admin address.
    let attacker = Address::generate(&s.env);
    let new_treasury = Address::generate(&s.env);
    let new_lp = Address::generate(&s.env);
    s.client.initialize(
        &attacker,
        &new_treasury,
        &new_lp,
        &10_000,
        &s.token_address,
        &attacker,
    );
}

// ─── Split-ratio tests (USDC, the default allowed token) ─────────────────────

#[test]
fn test_50_50_split() {
    let s = setup(5_000); // 50% LP
    s.client.distribute(&s.caller, &s.token_address, &10_000);
    assert_eq!(s.token.balance(&s.treasury), 5_000);
    assert_eq!(s.token.balance(&s.lp), 5_000);
}

#[test]
fn test_30_70_split() {
    let s = setup(3_000); // 30% LP, 70% treasury
    s.client.distribute(&s.caller, &s.token_address, &10_000);
    assert_eq!(s.token.balance(&s.lp), 3_000);
    assert_eq!(s.token.balance(&s.treasury), 7_000);
}

#[test]
fn test_zero_lp_share() {
    let s = setup(0); // 0% LP — all goes to treasury
    s.client.distribute(&s.caller, &s.token_address, &10_000);
    assert_eq!(s.token.balance(&s.treasury), 10_000);
    assert_eq!(s.token.balance(&s.lp), 0);
}

#[test]
fn test_hundred_percent_lp() {
    let s = setup(10_000); // 100% LP — nothing to treasury
    s.client.distribute(&s.caller, &s.token_address, &10_000);
    assert_eq!(s.token.balance(&s.lp), 10_000);
    assert_eq!(s.token.balance(&s.treasury), 0);
}

#[test]
fn test_rounding_remainder_goes_to_treasury() {
    let s = setup(3_333); // 33.33% LP
    s.client.distribute(&s.caller, &s.token_address, &10);
    // lp = 10 * 3333 / 10000 = 3 (truncated), treasury = 7
    assert_eq!(s.token.balance(&s.lp), 3);
    assert_eq!(s.token.balance(&s.treasury), 7);
}

// ─── Multi-token distribution (XLM and USDC) ─────────────────────────────────

/// Distribute fees denominated in a second token (XLM stand-in) after
/// registering it via `add_allowed_token`. Verifies that treasury and LP
/// receive the correct amounts in the XLM token — not the initially configured
/// USDC token — demonstrating fix for issue #63.
#[test]
fn test_distribute_xlm_fees() {
    let s = setup(5_000); // 50 / 50 split

    // Deploy a second asset to act as native XLM.
    let xlm_admin = Address::generate(&s.env);
    let xlm_id = s.env.register_stellar_asset_contract_v2(xlm_admin.clone());
    let xlm_sac = StellarAssetClient::new(&s.env, &xlm_id.address());
    // Mint XLM to the distributor contract so distribute() can transfer from
    // the contract's own balance (issue #221).
    xlm_sac.mint(&s.client.address, &1_000_000);

    let xlm_token = TokenClient::new(&s.env, &xlm_id.address());

    // Allow the XLM token.
    s.client.add_allowed_token(&s.admin, &xlm_id.address());

    // Distribute XLM fees.
    s.client.distribute(&s.caller, &xlm_id.address(), &10_000);

    // Correct XLM balances.
    assert_eq!(xlm_token.balance(&s.treasury), 5_000);
    assert_eq!(xlm_token.balance(&s.lp), 5_000);

    // USDC balances are untouched.
    assert_eq!(s.token.balance(&s.treasury), 0);
    assert_eq!(s.token.balance(&s.lp), 0);
}

/// Distribute USDC fees first, then XLM fees in a subsequent call.
/// Both distributions should succeed and balances for each token should be
/// tracked independently.
#[test]
fn test_distribute_both_usdc_and_xlm() {
    let s = setup(5_000); // 50 / 50

    // Deploy XLM stand-in.
    let xlm_admin = Address::generate(&s.env);
    let xlm_id = s.env.register_stellar_asset_contract_v2(xlm_admin.clone());
    let xlm_sac = StellarAssetClient::new(&s.env, &xlm_id.address());
    xlm_sac.mint(&s.client.address, &1_000_000);

    let xlm_token = TokenClient::new(&s.env, &xlm_id.address());

    // Register XLM as an allowed fee asset.
    s.client.add_allowed_token(&s.admin, &xlm_id.address());

    // First distribution: USDC.
    s.client.distribute(&s.caller, &s.token_address, &10_000);

    // Second distribution: XLM.
    s.client.distribute(&s.caller, &xlm_id.address(), &20_000);

    // USDC balances.
    assert_eq!(s.token.balance(&s.treasury), 5_000);
    assert_eq!(s.token.balance(&s.lp), 5_000);

    // XLM balances.
    assert_eq!(xlm_token.balance(&s.treasury), 10_000);
    assert_eq!(xlm_token.balance(&s.lp), 10_000);
}

/// `distribute` rejects a token address that has not been added to the
/// allowlist, even if the caller holds that token.
#[test]
#[should_panic(expected = "token not allowed")]
fn test_distribute_rejects_unapproved_token() {
    let s = setup(5_000);

    // Deploy a random unknown token.
    let unknown_admin = Address::generate(&s.env);
    let unknown_id = s.env.register_stellar_asset_contract_v2(unknown_admin.clone());
    let unknown_sac = StellarAssetClient::new(&s.env, &unknown_id.address());
    unknown_sac.mint(&s.client.address, &10_000);

    // Attempt to distribute without adding to the allowlist — should panic.
    s.client.distribute(&s.caller, &unknown_id.address(), &10_000);
}

/// After adding and then removing a token, distribution for that token should
/// be rejected again.
#[test]
#[should_panic(expected = "token not allowed")]
fn test_distribute_rejects_removed_token() {
    let s = setup(5_000);

    let xlm_admin = Address::generate(&s.env);
    let xlm_id = s.env.register_stellar_asset_contract_v2(xlm_admin.clone());
    let xlm_sac = StellarAssetClient::new(&s.env, &xlm_id.address());
    xlm_sac.mint(&s.client.address, &10_000);

    // Add then remove.
    s.client.add_allowed_token(&s.admin, &xlm_id.address());
    s.client.remove_allowed_token(&s.admin, &xlm_id.address());

    // Distribution should now fail.
    s.client.distribute(&s.caller, &xlm_id.address(), &10_000);
}

/// `get_allowed_tokens` returns the USDC address seeded in the constructor.
#[test]
fn test_get_allowed_tokens_contains_usdc_at_init() {
    let s = setup(5_000);
    let tokens = s.client.get_allowed_tokens();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens.get(0).unwrap(), s.token_address);
}

/// `get_allowed_tokens` reflects additions.
#[test]
fn test_get_allowed_tokens_reflects_additions() {
    let s = setup(5_000);

    let xlm_admin = Address::generate(&s.env);
    let xlm_id = s.env.register_stellar_asset_contract_v2(xlm_admin.clone());

    s.client.add_allowed_token(&s.admin, &xlm_id.address());

    let tokens = s.client.get_allowed_tokens();
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens.get(0).unwrap(), s.token_address);
    assert_eq!(tokens.get(1).unwrap(), xlm_id.address());
}

// ─── Contract-balance distribution (issue #221) ──────────────────────────────

/// `distribute` must transfer from the distributor contract's own balance
/// (`env.current_contract_address()`), not from the caller. The caller holds no
/// tokens here; only the contract was funded in `setup`.
#[test]
fn test_distribute_from_contract_balance() {
    let s = setup(5_000);

    // Caller holds nothing — distribution must still succeed from the
    // contract's balance.
    assert_eq!(s.token.balance(&s.caller), 0);
    assert_eq!(s.token.balance(&s.client.address), 1_000_000);

    s.client.distribute(&s.caller, &s.token_address, &10_000);

    assert_eq!(s.token.balance(&s.treasury), 5_000);
    assert_eq!(s.token.balance(&s.lp), 5_000);
    assert_eq!(s.token.balance(&s.client.address), 990_000);
}
