#![cfg(test)]

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Address, Env, Vec};

fn default_tiers(env: &Env) -> Vec<FeeTier> {
    vec![
        env,
        FeeTier {
            threshold_usdc: 0,
            fee_bps: 100,
        },
        FeeTier {
            threshold_usdc: 10_000,
            fee_bps: 50,
        },
        FeeTier {
            threshold_usdc: 100_000,
            fee_bps: 25,
        },
    ]
}

fn setup(env: &Env) -> (Address, Address, FeeCalculatorContractClient<'_>) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let settlement = Address::generate(env);
    let contract_id = env.register(FeeCalculatorContract, (&admin, &default_tiers(env)));
    let client = FeeCalculatorContractClient::new(env, &contract_id);
    client.set_settlement_caller(&admin, &settlement);
    (admin, settlement, client)
}

#[test]
fn test_calculate_fee_base_tier() {
    let env = Env::default();
    let (_admin, settlement, client) = setup(&env);
    let merchant = Address::generate(&env);

    let (fee, net, fee_bps) = client.calculate_fee(&settlement, &merchant, &1_000);
    assert_eq!(fee_bps, 100);
    assert_eq!(fee, 10);
    assert_eq!(net, 990);
}

#[test]
fn test_calculate_fee_rejects_zero_amount() {
    let env = Env::default();
    let (_admin, settlement, client) = setup(&env);
    let merchant = Address::generate(&env);

    let result = client.try_calculate_fee(&settlement, &merchant, &0);
    assert!(result.is_err());
}

#[test]
fn test_calculate_fee_rejects_unauthorized_caller() {
    let env = Env::default();
    let (_admin, _settlement, client) = setup(&env);
    let merchant = Address::generate(&env);
    let attacker = Address::generate(&env);

    let result = client.try_calculate_fee(&attacker, &merchant, &1_000);
    assert!(result.is_err());
}

#[test]
fn test_volume_tier_transition() {
    let env = Env::default();
    let (_admin, settlement, client) = setup(&env);
    let merchant = Address::generate(&env);

    // First settlement stays in the base tier.
    let (_, _, fee_bps) = client.calculate_fee(&settlement, &merchant, &5_000);
    assert_eq!(fee_bps, 100);

    // Crossing the 10_000 threshold moves the merchant to the 50 bps tier.
    let (_, _, fee_bps) = client.calculate_fee(&settlement, &merchant, &6_000);
    assert_eq!(fee_bps, 50);

    // Crossing the 100_000 threshold moves the merchant to the 25 bps tier.
    let (_, _, fee_bps) = client.calculate_fee(&settlement, &merchant, &100_000);
    assert_eq!(fee_bps, 25);
}

#[test]
fn test_min_fee_applied() {
    let env = Env::default();
    let (admin, settlement, client) = setup(&env);
    let merchant = Address::generate(&env);

    client.set_min_fee_stroops(&admin, &5);

    let (fee, net, _fee_bps) = client.calculate_fee(&settlement, &merchant, &100);
    assert_eq!(fee, 5);
    assert_eq!(net, 95);
}

#[test]
fn test_min_fee_capped_at_amount() {
    let env = Env::default();
    let (admin, settlement, client) = setup(&env);
    let merchant = Address::generate(&env);

    client.set_min_fee_stroops(&admin, &1_000);

    let (fee, net, _fee_bps) = client.calculate_fee(&settlement, &merchant, &100);
    assert_eq!(fee, 100);
    assert_eq!(net, 0);
}

#[test]
fn test_quote_fee_does_not_mutate_volume() {
    let env = Env::default();
    let (_admin, settlement, client) = setup(&env);
    let merchant = Address::generate(&env);

    let (fee, net, fee_bps) = client.quote_fee(&merchant, &1_000);
    assert_eq!(fee_bps, 100);
    assert_eq!(fee, 10);
    assert_eq!(net, 990);

    // A quote must not accrue volume: the first real settlement still sees
    // the base tier.
    let (_, _, fee_bps) = client.calculate_fee(&settlement, &merchant, &1_000);
    assert_eq!(fee_bps, 100);
}

#[test]
fn test_attacker_cannot_grief_merchant_fee_tier() {
    let env = Env::default();
    let (_admin, settlement, client) = setup(&env);
    let merchant = Address::generate(&env);
    let attacker = Address::generate(&env);

    // Attacker attempts to push the victim merchant's volume past the tier
    // threshold with no real settlement activity.
    let result = client.try_calculate_fee(&attacker, &merchant, &1_000);
    assert!(result.is_err());

    // The victim merchant's volume is untouched, so the first legitimate
    // settlement still lands in the base tier.
    let (_, _, fee_bps) = client.calculate_fee(&settlement, &merchant, &1_000);
    assert_eq!(fee_bps, 100);
}

#[test]
fn test_set_fee_tiers_requires_admin() {
    let env = Env::default();
    let (_admin, _settlement, client) = setup(&env);
    let attacker = Address::generate(&env);

    let result = client.try_set_fee_tiers(&attacker, &default_tiers(&env));
    assert!(result.is_err());
}

#[test]
fn test_set_fee_tiers_rejects_unsorted() {
    let env = Env::default();
    let (admin, _settlement, client) = setup(&env);

    let bad = vec![
        &env,
        FeeTier {
            threshold_usdc: 10_000,
            fee_bps: 50,
        },
        FeeTier {
            threshold_usdc: 0,
            fee_bps: 100,
        },
    ];
    let result = client.try_set_fee_tiers(&admin, &bad);
    assert!(result.is_err());
}
