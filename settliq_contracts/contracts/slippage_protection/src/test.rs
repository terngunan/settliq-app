#![cfg(test)]

use crate::{SlippageProtectionContract, SlippageProtectionContractClient};
use soroban_sdk::{testutils::{Address as _, Events as _}, Address, Env, IntoVal};

fn setup_env() -> (Env, SlippageProtectionContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register(SlippageProtectionContract, (&admin,));
    let client = SlippageProtectionContractClient::new(&env, &contract_id);

    (env, client, admin)
}

// ── constructor ──────────────────────────────────────────────────────────────

#[test]
fn test_constructor_defaults() {
    let (_env, client, admin) = setup_env();
    assert_eq!(client.get_max_slippage(), 100);
    assert_eq!(client.get_admin(), admin);
}

// ── check_slippage: happy paths ──────────────────────────────────────────────

#[test]
fn test_check_slippage_exact_boundary_passes() {
    // deviation == max_bps (100 bps = 1%) → should NOT revert
    let (_env, client, _admin) = setup_env();
    // actual is exactly 1% below expected: 10_000 → 9_900
    client.check_slippage(&10_000, &9_900);
}

#[test]
fn test_check_slippage_within_tolerance_passes() {
    let (_env, client, _admin) = setup_env();
    // 0.5% deviation — well within 1%
    client.check_slippage(&10_000, &9_950);
}

#[test]
fn test_check_slippage_zero_deviation_passes() {
    let (_env, client, _admin) = setup_env();
    client.check_slippage(&10_000, &10_000);
}

// ── check_slippage: favorable price improvement (issue #257) ─────────────────

#[test]
fn test_check_slippage_positive_improvement_passes() {
    // actual > expected: trader receives MORE than expected → must NOT revert.
    let (_env, client, _admin) = setup_env();
    client.check_slippage(&10_000, &10_500);
}

#[test]
fn test_check_slippage_large_positive_improvement_passes() {
    // Even a large favorable movement must not be treated as slippage.
    let (_env, client, _admin) = setup_env();
    client.check_slippage(&10_000, &20_000);
}

#[test]
fn test_check_slippage_for_pair_positive_improvement_passes() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Tight per-pair limit (50 bps) must not penalize favorable movement.
    client.set_pair_max_slippage(&admin, &token_a, &token_b, &50);
    client.check_slippage_for_pair(&token_a, &token_b, &10_000, &10_500);
}

// ── check_slippage: revert paths ─────────────────────────────────────────────

#[test]
#[should_panic(expected = "SlippageExceeded")]
fn test_check_slippage_one_bps_over_reverts() {
    // deviation == 101 bps → must revert
    let (_env, client, _admin) = setup_env();
    // 101 bps below: 10_000 → 9_899
    client.check_slippage(&10_000, &9_899);
}

#[test]
#[should_panic(expected = "SlippageExceeded")]
fn test_check_slippage_large_deviation_reverts() {
    let (_env, client, _admin) = setup_env();
    client.check_slippage(&10_000, &5_000); // 50% deviation
}

#[test]
#[should_panic(expected = "expected must be > 0")]
fn test_check_slippage_zero_expected_reverts() {
    let (_env, client, _admin) = setup_env();
    client.check_slippage(&0, &100);
}

// ── set_max_slippage ─────────────────────────────────────────────────────────

#[test]
fn test_admin_can_tighten_slippage() {
    let (_env, client, admin) = setup_env();
    client.set_max_slippage(&admin, &50); // tighten to 0.5%
    assert_eq!(client.get_max_slippage(), 50);
}

#[test]
fn test_admin_can_relax_slippage() {
    let (_env, client, admin) = setup_env();
    client.set_max_slippage(&admin, &300); // relax to 3%
    assert_eq!(client.get_max_slippage(), 300);
}

#[test]
#[should_panic(expected = "bps must be <= 10000")]
fn test_admin_cannot_set_slippage_above_bps_denominator() {
    let (_env, client, admin) = setup_env();
    client.set_max_slippage(&admin, &10_001);
}

#[test]
fn test_tightened_slippage_rejects_previously_valid_swap() {
    let (_env, client, admin) = setup_env();
    // 0.8% deviation passes at default 1%
    client.check_slippage(&10_000, &9_920);
    // tighten to 0.5%
    client.set_max_slippage(&admin, &50);
    // now same deviation should revert — tested separately below
    assert_eq!(client.get_max_slippage(), 50);
}

#[test]
#[should_panic(expected = "SlippageExceeded")]
fn test_tightened_slippage_reverts_swap() {
    let (_env, client, admin) = setup_env();
    client.set_max_slippage(&admin, &50); // 0.5%
                                          // 0.8% deviation → should now revert
    client.check_slippage(&10_000, &9_920);
}

#[test]
#[should_panic(expected = "Not admin")]
fn test_non_admin_cannot_set_slippage() {
    let (env, client, _admin) = setup_env();
    let random = Address::generate(&env);
    client.set_max_slippage(&random, &200);
}

#[test]
#[should_panic(expected = "bps must be <= 10000")]
fn test_set_max_slippage_rejects_out_of_range_bps() {
    let (_env, client, admin) = setup_env();
    client.set_max_slippage(&admin, &10_001);
}

// ── Issue #24: commutative pair slippage lookup ───────────────────────────────

#[test]
fn test_set_pair_and_query_same_order() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client.set_pair_max_slippage(&admin, &token_a, &token_b, &200);
    // Querying (A, B) must return the configured value.
    assert_eq!(client.get_pair_max_slippage(&token_a, &token_b), 200);
}

#[test]
fn test_set_pair_ab_query_reversed_ba() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Store with (A, B) order.
    client.set_pair_max_slippage(&admin, &token_a, &token_b, &200);

    // Querying (B, A) must return the same value — lookup is commutative.
    assert_eq!(client.get_pair_max_slippage(&token_b, &token_a), 200);
}

#[test]
fn test_set_pair_ba_query_reversed_ab() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Store with (B, A) order.
    client.set_pair_max_slippage(&admin, &token_b, &token_a, &150);

    // Querying (A, B) must return the same value.
    assert_eq!(client.get_pair_max_slippage(&token_a, &token_b), 150);
}

#[test]
fn test_check_slippage_for_pair_reversed_order_applies_override() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Set a tighter per-pair limit (50 bps) for (A, B).
    client.set_pair_max_slippage(&admin, &token_a, &token_b, &50);

    // A 0.3% deviation (30 bps) passes under both the global (100 bps) and
    // the per-pair (50 bps) overrides.
    client.check_slippage_for_pair(&token_b, &token_a, &10_000, &9_970);
}

#[test]
#[should_panic(expected = "SlippageExceeded")]
fn test_check_slippage_for_pair_reversed_order_enforces_override() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Set a tight per-pair limit (50 bps) for (A, B).
    client.set_pair_max_slippage(&admin, &token_a, &token_b, &50);

    // A 0.8% deviation (80 bps) exceeds 50 bps regardless of argument order.
    // Querying (B, A) must apply the override, not the global default (100 bps).
    client.check_slippage_for_pair(&token_b, &token_a, &10_000, &9_920);
}

#[test]
fn test_pair_override_does_not_affect_other_pairs() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let token_c = Address::generate(&env);

    // Set tight override for (A, B).
    client.set_pair_max_slippage(&admin, &token_a, &token_b, &50);

    // Pair (A, C) must still use the global default (100 bps).
    assert_eq!(client.get_pair_max_slippage(&token_a, &token_c), 100);
}

// ── Issue #258: remove_pair_max_slippage graceful handling ───────────────────

#[test]
fn test_remove_pair_max_slippage_existing_returns_some() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client.set_pair_max_slippage(&admin, &token_a, &token_b, &200);

    // Removing an existing override returns the previous value.
    let removed = client.remove_pair_max_slippage(&admin, &token_a, &token_b);
    assert_eq!(removed, Some(200));

    // After removal the pair falls back to the global default.
    assert_eq!(client.get_pair_max_slippage(&token_a, &token_b), 100);
}

#[test]
fn test_remove_pair_max_slippage_non_existent_returns_none() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // No override was ever set — must not panic, returns None.
    let removed = client.remove_pair_max_slippage(&admin, &token_a, &token_b);
    assert_eq!(removed, None);
}

#[test]
fn test_remove_pair_max_slippage_twice_second_returns_none() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client.set_pair_max_slippage(&admin, &token_a, &token_b, &75);

    // First removal succeeds.
    assert_eq!(
        client.remove_pair_max_slippage(&admin, &token_a, &token_b),
        Some(75)
    );
    // Second removal of the already-cleared override is graceful.
    assert_eq!(
        client.remove_pair_max_slippage(&admin, &token_a, &token_b),
        None
    );
}

#[test]
fn test_remove_pair_max_slippage_reversed_order() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Store with (A, B) order, remove with (B, A) — commutative.
    client.set_pair_max_slippage(&admin, &token_a, &token_b, &150);
    assert_eq!(
        client.remove_pair_max_slippage(&admin, &token_b, &token_a),
        Some(150)
    );
    assert_eq!(client.get_pair_max_slippage(&token_a, &token_b), 100);
}
