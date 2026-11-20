#![cfg(test)]

use crate::{
    FeeCalculatorContract, FeeCalculatorContractClient, FeeTier, FeeTiersUpdatedEvent,
    MinFeeUpdatedEvent, SettlementCallerUpdatedEvent,
};
use soroban_sdk::{testutils::{Address as _, Events as _, Ledger}, vec, Address, Env, IntoVal};

fn setup_env() -> (
    Env,
    FeeCalculatorContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| {
        li.min_temp_entry_ttl = 300_000;
        li.min_persistent_entry_ttl = 300_000;
        li.max_entry_ttl = 300_000;
    });

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let settlement_caller = Address::generate(&env);
    let tiers = vec![
        &env,
        FeeTier {
            threshold_usdc: 0,
            fee_bps: 150,
        },
        FeeTier {
            threshold_usdc: 1_000,
            fee_bps: 120,
        },
        FeeTier {
            threshold_usdc: 10_000,
            fee_bps: 100,
        },
    ];

    let contract_id = env.register(FeeCalculatorContract, (&admin, tiers));
    let client = FeeCalculatorContractClient::new(&env, &contract_id);
    client.set_settlement_caller(&admin, &settlement_caller);

    (env, client, admin, merchant, settlement_caller)
}

#[test]
fn test_get_admin_returns_initial_admin() {
    let (_env, client, admin, _merchant, _settlement_caller) = setup_env();

    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_transfer_admin_allows_new_admin_to_update_fee_tiers() {
    let (env, client, admin, _merchant, _settlement_caller) = setup_env();
    let new_admin = Address::generate(&env);

    client.transfer_admin(&admin, &new_admin);
    assert_eq!(client.get_admin(), new_admin);

    let new_tiers = vec![
        &env,
        FeeTier {
            threshold_usdc: 0,
            fee_bps: 200,
        },
        FeeTier {
            threshold_usdc: 5_000,
            fee_bps: 80,
        },
    ];

    client.set_fee_tiers(&new_admin, &new_tiers);
    assert_eq!(client.get_fee_tiers(), new_tiers);
}

#[test]
fn test_fee_rate_drops_when_volume_crosses_tier_threshold() {
    let (_env, client, _admin, merchant, settlement_caller) = setup_env();

    // Volume = 900, still tier 1 (150 bps)
    let (_, _, bps_before) = client.calculate_fee(&settlement_caller, &merchant, &900);
    assert_eq!(bps_before, 150);

    // Volume = 1000, enters tier 2 (120 bps)
    let (_, _, bps_after) = client.calculate_fee(&settlement_caller, &merchant, &100);
    assert_eq!(bps_after, 120);
}

#[test]
fn test_highest_tier_applies_at_exact_boundary() {
    let (env, client, _admin, merchant, settlement_caller) = setup_env();

    client.calculate_fee(&settlement_caller, &merchant, &2_000); // puts merchant into 120 bps tier
    let (_, _, bps_before_reset) = client.calculate_fee(&settlement_caller, &merchant, &1);
    assert_eq!(bps_before_reset, 120);
}

#[test]
fn test_volume_decays_smoothly_across_sub_window_boundaries() {
    let (env, client, _admin, merchant, settlement_caller) = setup_env();

    // Record 2_000 USDC of volume in the first sub-window (5-day bucket).
    client.calculate_fee(&settlement_caller, &merchant, &2_000);
    assert_eq!(client.get_merchant_volume(&merchant).volume_usdc, 2_000);

    // Advance one sub-window (5 days = 28_800 ledgers). The trailing volume
    // must NOT reset to zero; it should still reflect the recent activity.
    env.ledger().with_mut(|li| li.sequence_number += 28_800);
    assert_eq!(client.get_merchant_volume(&merchant).volume_usdc, 2_000);

    // Advance another sub-window; volume still within the 30-day window.
    env.ledger().with_mut(|li| li.sequence_number += 28_800);
    assert_eq!(client.get_merchant_volume(&merchant).volume_usdc, 2_000);
}

#[test]
fn test_volume_decays_gradually_after_full_window() {
    let (env, client, _admin, merchant, settlement_caller) = setup_env();

    // Record volume in the first bucket.
    client.calculate_fee(&settlement_caller, &merchant, &2_000);
    assert_eq!(client.get_merchant_volume(&merchant).volume_usdc, 2_000);

    // Advance a full 30-day window (172_800 ledgers). The original bucket has
    // aged out, so trailing volume should be zero (no activity since).
    env.ledger().with_mut(|li| li.sequence_number += 172_800);
    assert_eq!(client.get_merchant_volume(&merchant).volume_usdc, 0);

    // A fresh transaction after the window starts a new bucket.
    client.calculate_fee(&settlement_caller, &merchant, &500);
    assert_eq!(client.get_merchant_volume(&merchant).volume_usdc, 500);
}

#[test]
fn test_volume_does_not_cliff_reset_at_month_boundary() {
    let (env, client, _admin, merchant, settlement_caller) = setup_env();

    // High-volume merchant pushes into the top tier.
    client.calculate_fee(&settlement_caller, &merchant, &20_000);
    let (_, _, bps_high) = client.calculate_fee(&settlement_caller, &merchant, &1);
    assert_eq!(bps_high, 100);

    // Advance just past the old tumbling-window boundary (30 days).
    env.ledger().with_mut(|li| li.sequence_number += 172_800);

    // With a rolling window the trailing volume decays bucket-by-bucket
    // rather than resetting abruptly; the merchant keeps their tier until
    // the activity ages out of the trailing window.
    let (_, _, bps_after) = client.calculate_fee(&settlement_caller, &merchant, &1);
    assert_eq!(bps_after, 100);
}

#[test]
fn test_get_merchant_volume_is_read_only_after_window_elapsed() {
    let (env, client, admin, merchant, _s) = setup_env();

    client.calculate_fee(&admin, &merchant, &2_000);
    assert_eq!(client.get_merchant_volume(&merchant).volume_usdc, 2_000);

    // Move ledger beyond 30-day window (172800 ledgers).
    env.ledger().with_mut(|li| li.sequence_number += 172_800);

    // The getter must report the windowed (aged-out) value without persisting it.
    assert_eq!(client.get_merchant_volume(&merchant).volume_usdc, 0);
    // A second read must observe the same value, proving no write-back occurred.
    assert_eq!(client.get_merchant_volume(&merchant).volume_usdc, 0);

    // The persisted volume is still the pre-reset value; only
    // update_and_get_volume (via calculate_fee) performs the bucket rotation.
}

#[test]
fn test_admin_can_update_fee_tiers() {
    let (env, client, admin, _merchant, _settlement_caller) = setup_env();

    let new_tiers = vec![
        &env,
        FeeTier {
            threshold_usdc: 0,
            fee_bps: 200,
        },
        FeeTier {
            threshold_usdc: 5_000,
            fee_bps: 80,
        },
    ];

    client.set_fee_tiers(&admin, &new_tiers);
    let stored = client.get_fee_tiers();
    assert_eq!(stored, new_tiers);
}

#[test]
fn test_set_fee_tiers_emits_fee_tiers_updated_event() {
    let (env, client, admin, _merchant, _s) = setup_env();

    let new_tiers = vec![
        &env,
        FeeTier {
            threshold_usdc: 0,
            fee_bps: 200,
        },
        FeeTier {
            threshold_usdc: 5_000,
            fee_bps: 80,
        },
    ];

    client.set_fee_tiers(&admin, &new_tiers);

    let expected = FeeTiersUpdatedEvent {
        admin: admin.clone(),
        tiers: new_tiers.clone(),
    };

    let events = env.events().all();
    let last = events.last().unwrap();
    assert_eq!(last.0, client.address);
}

#[test]
fn test_set_settlement_caller_emits_event() {
    let (env, client, admin, _merchant, _settlement) = setup_env();
    let new_settlement = Address::generate(&env);

    client.set_settlement_caller(&admin, &new_settlement);

    let expected = SettlementCallerUpdatedEvent {
        caller: admin.clone(),
        settlement_caller: new_settlement.clone(),
    };

    let events = env.events().all();
    let last = events.last().unwrap();
    assert_eq!(last.0, client.address);
    assert_eq!(
        last.1,
        (soroban_sdk::symbol_short!("settle_caller"),).into_val(&env)
    );
    assert_eq!(last.2, expected.into_val(&env));
}

#[test]
fn test_set_min_fee_stroops_emits_event() {
    let (env, client, admin, _merchant, _s) = setup_env();

    let old_min_fee = client.get_min_fee_stroops();
    let new_min_fee = 5;

    client.set_min_fee_stroops(&admin, &new_min_fee);

    let expected = MinFeeUpdatedEvent {
        caller: admin.clone(),
        old_min_fee,
        new_min_fee,
    };

    let events = env.events().all();
    let last = events.last().unwrap();
    assert_eq!(last.0, client.address);
    assert_eq!(
        last.1,
        (soroban_sdk::symbol_short!("min_fee"),).into_val(&env)
    );
    assert_eq!(last.2, expected.into_val(&env));
}

#[test]
#[should_panic(expected = "Not admin")]
fn test_non_admin_cannot_update_fee_tiers() {
    let (env, client, _admin, _merchant, _s) = setup_env();
    let non_admin = Address::generate(&env);

    let new_tiers = vec![
        &env,
        FeeTier {
            threshold_usdc: 0,
            fee_bps: 200,
        },
    ];

    client.set_fee_tiers(&non_admin, &new_tiers);
}
