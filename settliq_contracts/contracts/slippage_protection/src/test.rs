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

// ── set_pair_max_slippage: audit event tests (issue #66) ─────────────────────

/// set_pair_max_slippage emits PairMaxSlippageUpdatedEvent on first set.
/// previous_bps should be the global default (100) since no override existed.
#[test]
fn test_set_pair_max_slippage_emits_event_first_set() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client.set_pair_max_slippage(&admin, &token_a, &token_b, &200);

    let events = env.events().all();
    let last = events.last().unwrap();

    // Topic check.
    assert_eq!(
        last.1,
        (
            soroban_sdk::String::from_str(&env, "SLIPPAGE"),
            soroban_sdk::String::from_str(&env, "pair_max_slippage_updated")
        )
            .into_val(&env)
    );

    // Data check: previous_bps is the global default, new_bps is 200.
    let event: crate::PairMaxSlippageUpdatedEvent = last.2.into_val(&env);
    assert_eq!(event.previous_bps, 100); // global default
    assert_eq!(event.new_bps, 200);
}

/// set_pair_max_slippage emits PairMaxSlippageUpdatedEvent on an update to an
/// already-configured pair. previous_bps should reflect the stored override,
/// not the global default.
#[test]
fn test_set_pair_max_slippage_emits_event_on_update() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // First set.
    client.set_pair_max_slippage(&admin, &token_a, &token_b, &200);

    // Second set — override the existing value.
    client.set_pair_max_slippage(&admin, &token_a, &token_b, &75);

    let events = env.events().all();
    let last = events.last().unwrap();
    let event: crate::PairMaxSlippageUpdatedEvent = last.2.into_val(&env);

    // previous_bps must be the prior override (200), not the global default.
    assert_eq!(event.previous_bps, 200);
    assert_eq!(event.new_bps, 75);
}

/// Event asset fields match the arguments passed to set_pair_max_slippage
/// regardless of whether they are reordered internally (canonical ordering).
#[test]
fn test_set_pair_max_slippage_event_contains_correct_assets() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client.set_pair_max_slippage(&admin, &token_a, &token_b, &300);

    let events = env.events().all();
    let last = events.last().unwrap();
    let event: crate::PairMaxSlippageUpdatedEvent = last.2.into_val(&env);

    // The event must carry the original caller-supplied addresses.
    assert_eq!(event.asset_a, token_a);
    assert_eq!(event.asset_b, token_b);
    assert_eq!(event.new_bps, 300);
}

/// Calling set_pair_max_slippage with arguments in reversed order (B, A)
/// still emits the event with the caller-supplied asset_a / asset_b values.
#[test]
fn test_set_pair_max_slippage_event_reversed_order_carries_caller_args() {
    let (env, client, admin) = setup_env();
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Set using reversed argument order.
    client.set_pair_max_slippage(&admin, &token_b, &token_a, &150);

    let events = env.events().all();
    let last = events.last().unwrap();
    let event: crate::PairMaxSlippageUpdatedEvent = last.2.into_val(&env);

    // asset_a and asset_b in the event should be what the caller passed in.
    assert_eq!(event.asset_a, token_b);
    assert_eq!(event.asset_b, token_a);
    assert_eq!(event.new_bps, 150);
}

#[test]
fn test_admin_transfer() {
    let (env, client, admin) = setup_env();
    let successor = Address::generate(&env);
    assert_eq!(client.get_admin(), admin);
    assert!(client.try_transfer_admin(&admin, &admin).is_err());
    assert!(client.try_transfer_admin(&successor, &admin).is_err());

    client.transfer_admin(&admin, &successor);
    assert_eq!(client.get_admin(), successor);
    assert!(client.try_transfer_admin(&admin, &successor).is_err());
    client.transfer_admin(&successor, &admin);
    assert_eq!(client.get_admin(), admin);
}
