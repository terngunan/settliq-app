#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Env, String};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, MerchantRegistryContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register(MerchantRegistryContract, (&admin,));
    let client = MerchantRegistryContractClient::new(&env, &contract_id);

    (env, client, admin)
}

fn sample_name(env: &Env) -> String {
    String::from_str(env, "Acme Corp")
}

// ---------------------------------------------------------------------------
// register_merchant
// ---------------------------------------------------------------------------

#[test]
fn test_register_merchant_happy_path() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));

    let record = client.get_merchant(&merchant).unwrap();
    assert_eq!(record.merchant, merchant);
    assert_eq!(record.status, MerchantStatus::Active);
}

#[test]
#[should_panic(expected = "Merchant already registered")]
fn test_register_duplicate_merchant_panics() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.register_merchant(&admin, &merchant, &sample_name(&env));
}

#[test]
#[should_panic(expected = "Caller is not admin")]
fn test_register_unauthorized() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.register_merchant(&attacker, &merchant, &sample_name(&env));
}

// ---------------------------------------------------------------------------
// suspend_merchant / reactivate_merchant
// ---------------------------------------------------------------------------

#[test]
fn test_suspend_merchant() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.suspend_merchant(&admin, &merchant);

    let record = client.get_merchant(&merchant).unwrap();
    assert_eq!(record.status, MerchantStatus::Suspended);
}

#[test]
fn test_reactivate_merchant() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.suspend_merchant(&admin, &merchant);
    client.reactivate_merchant(&admin, &merchant);

    let record = client.get_merchant(&merchant).unwrap();
    assert_eq!(record.status, MerchantStatus::Active);
}

#[test]
#[should_panic(expected = "Caller is not admin")]
fn test_suspend_unauthorized() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.suspend_merchant(&attacker, &merchant);
}

#[test]
#[should_panic(expected = "Caller is not admin")]
fn test_reactivate_unauthorized() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.suspend_merchant(&admin, &merchant);
    client.reactivate_merchant(&attacker, &merchant);
}

#[test]
#[should_panic(expected = "Merchant already suspended")]
fn test_suspend_already_suspended() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.suspend_merchant(&admin, &merchant);
    client.suspend_merchant(&admin, &merchant);
}

#[test]
#[should_panic(expected = "Merchant already active")]
fn test_reactivate_already_active() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    // merchant is Active after registration – reactivating should panic
    client.reactivate_merchant(&admin, &merchant);
}

// ---------------------------------------------------------------------------
// terminate_merchant
// ---------------------------------------------------------------------------

#[test]
fn test_terminate_merchant_from_active() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.terminate_merchant(&admin, &merchant);

    let record = client.get_merchant(&merchant).unwrap();
    assert_eq!(record.status, MerchantStatus::Terminated);
}

#[test]
fn test_terminate_merchant_from_suspended() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.suspend_merchant(&admin, &merchant);
    client.terminate_merchant(&admin, &merchant);

    let record = client.get_merchant(&merchant).unwrap();
    assert_eq!(record.status, MerchantStatus::Terminated);
}

#[test]
fn test_is_merchant_active_returns_false_after_termination() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.terminate_merchant(&admin, &merchant);

    let record = client.get_merchant(&merchant).unwrap();
    assert_eq!(record.status, MerchantStatus::Terminated);
}

#[test]
#[should_panic(expected = "Caller is not admin")]
fn test_terminate_unauthorized() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.terminate_merchant(&attacker, &merchant);
}

#[test]
#[should_panic(expected = "Merchant not found")]
fn test_terminate_unknown_merchant() {
    let (env, client, admin) = setup();
    let unknown = Address::generate(&env);

    client.terminate_merchant(&admin, &unknown);
}

#[test]
#[should_panic(expected = "Merchant already terminated")]
fn test_terminate_already_terminated() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.terminate_merchant(&admin, &merchant);
    client.terminate_merchant(&admin, &merchant);
}

#[test]
#[should_panic(expected = "Cannot reactivate terminated merchant")]
fn test_reactivate_after_termination_fails() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.terminate_merchant(&admin, &merchant);
    client.reactivate_merchant(&admin, &merchant);
}

#[test]
#[should_panic(expected = "Merchant is terminated")]
fn test_suspend_after_termination_fails() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.terminate_merchant(&admin, &merchant);
    client.suspend_merchant(&admin, &merchant);
}

// ---------------------------------------------------------------------------
// merchants() paginated listing
// ---------------------------------------------------------------------------

#[test]
fn test_merchants_listing_excludes_terminated_merchants() {
    let (env, client, admin) = setup();

    let merchant_a = Address::generate(&env);
    let merchant_b = Address::generate(&env);
    let merchant_c = Address::generate(&env);

    client.register_merchant(&admin, &merchant_a, &sample_name(&env));
    client.register_merchant(&admin, &merchant_b, &sample_name(&env));
    client.register_merchant(&admin, &merchant_c, &sample_name(&env));

    client.terminate_merchant(&admin, &merchant_b);

    let listed = client.merchants(&0, &10);
    assert_eq!(listed.len(), 2);
    assert_eq!(listed.get(0).unwrap().merchant, merchant_a);
    assert_eq!(listed.get(1).unwrap().merchant, merchant_c);
}

#[test]
fn test_merchants_listing_respects_limit() {
    let (env, client, admin) = setup();

    let merchant_a = Address::generate(&env);
    let merchant_b = Address::generate(&env);

    client.register_merchant(&admin, &merchant_a, &sample_name(&env));
    client.register_merchant(&admin, &merchant_b, &sample_name(&env));

    let listed = client.merchants(&0, &1);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed.get(0).unwrap().merchant, merchant_a);
}

// ---------------------------------------------------------------------------
// TTL maintenance
// ---------------------------------------------------------------------------

#[test]
fn test_register_extends_merchant_ttl() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));

    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let key = DataKey::Merchant(merchant.clone());
        let ttl = env.storage().persistent().get_ttl(&key);
        assert!(ttl >= MERCHANT_TTL_THRESHOLD);
    });
}

#[test]
fn test_register_extends_merchant_at_ttl() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));

    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let key = DataKey::MerchantAt(0);
        let ttl = env.storage().persistent().get_ttl(&key);
        assert!(ttl >= MERCHANT_TTL_THRESHOLD);
    });
}

#[test]
fn test_status_update_extends_merchant_ttl() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    client.suspend_merchant(&admin, &merchant);

    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let key = DataKey::Merchant(merchant.clone());
        let ttl = env.storage().persistent().get_ttl(&key);
        assert!(ttl >= MERCHANT_TTL_THRESHOLD);
    });
}

#[test]
fn test_lookup_extends_merchant_ttl() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));
    let _ = client.get_merchant(&merchant);

    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let key = DataKey::Merchant(merchant.clone());
        let ttl = env.storage().persistent().get_ttl(&key);
        assert!(ttl >= MERCHANT_TTL_THRESHOLD);
    });
}

#[test]
fn test_merchant_status_preserved_across_ledger_advance() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);

    client.register_merchant(&admin, &merchant, &sample_name(&env));

    // Advance the ledger well past the threshold; TTL extension should keep
    // the merchant record live and its approval status intact.
    env.ledger().set_sequence_number(MERCHANT_TTL_THRESHOLD + 1);

    let record = client.get_merchant(&merchant).unwrap();
    assert_eq!(record.status, MerchantStatus::Active);
    assert!(client.is_merchant_active(&merchant));
}
