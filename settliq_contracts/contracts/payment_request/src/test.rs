#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env, String};

fn setup_contract(env: &Env) -> (PaymentRequestContractClient<'static>, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PaymentRequestContract);
    let client = PaymentRequestContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (client, admin)
}

#[test]
fn test_payment_lifecycle_success() {
    let env = Env::default();
    let (client, admin) = setup_contract(&env);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_123");
    let amount = 1000i128;
    let expiry = 1000u64;

    // Start ledger at t=0
    env.ledger().set_timestamp(0);

    // 1. Create Payment (merchant auth)
    client.create_payment(&payment_id, &merchant, &amount, &asset, &expiry);
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert_eq!(payment.amount, amount);

    // 2. Confirm Payment (admin auth)
    client.confirm(&admin, &payment_id);
    assert_eq!(client.get_payment(&payment_id).status, PaymentStatus::Confirmed);

    // 3. Set Settling (admin auth)
    client.set_settling(&admin, &payment_id);
    assert_eq!(client.get_payment(&payment_id).status, PaymentStatus::Settling);

    // 4. Settle (admin auth)
    client.settle(&admin, &payment_id);
    assert_eq!(client.get_payment(&payment_id).status, PaymentStatus::Settled);
}

#[test]
#[should_panic(expected = "Invalid transition: can only settle from Settling")]
fn test_direct_settlement_from_confirmed() {
    let env = Env::default();
    let (client, admin) = setup_contract(&env);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_123");
    
    client.create_payment(&payment_id, &merchant, &100, &asset, &1000);
    client.confirm(&admin, &payment_id);
    
    // Skipping the settling state is rejected
    client.settle(&admin, &payment_id);
}

#[test]
#[should_panic(expected = "Invalid transition: can only settle from Settling")]
fn test_invalid_settle_from_pending() {
    let env = Env::default();
    let (client, admin) = setup_contract(&env);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_123");
    
    client.create_payment(&payment_id, &merchant, &100, &asset, &1000);
    
    // Try to settle directly from pending
    client.settle(&admin, &payment_id);
}

#[test]
fn test_payment_expiry() {
    let env = Env::default();
    let (client, _admin) = setup_contract(&env);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_123");
    let expiry = 100u64;

    env.ledger().set_timestamp(0);
    client.create_payment(&payment_id, &merchant, &100, &asset, &expiry);

    // Advance time past expiry — permissionless, no caller required
    env.ledger().set_timestamp(101);

    client.expire(&payment_id);
    assert_eq!(client.get_payment(&payment_id).status, PaymentStatus::Expired);
}

#[test]
#[should_panic(expected = "Payment has not yet reached expiry time")]
fn test_early_expiry_fails() {
    let env = Env::default();
    let (client, _admin) = setup_contract(&env);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_123");
    let expiry = 100u64;

    env.ledger().set_timestamp(0);
    client.create_payment(&payment_id, &merchant, &100, &asset, &expiry);

    // Try to expire at t=50
    env.ledger().set_timestamp(50);
    client.expire(&payment_id);
}

#[test]
fn test_payment_failure() {
    let env = Env::default();
    let (client, admin) = setup_contract(&env);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_123");

    client.create_payment(&payment_id, &merchant, &100, &asset, &1000);
    client.confirm(&admin, &payment_id);
    
    client.fail(&admin, &payment_id);
    assert_eq!(client.get_payment(&payment_id).status, PaymentStatus::Failed);
}

#[test]
#[should_panic(expected = "Cannot fail a finalized payment")]
fn test_fail_after_settled_panics() {
    let env = Env::default();
    let (client, admin) = setup_contract(&env);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_123");

    client.create_payment(&payment_id, &merchant, &100, &asset, &1000);
    client.confirm(&admin, &payment_id);
    client.set_settling(&admin, &payment_id);
    client.settle(&admin, &payment_id);
    
    client.fail(&admin, &payment_id);
}

#[test]
#[should_panic(expected = "expiry must be in the future")]
fn test_create_payment_rejects_zero_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PaymentRequestContract);
    let client = PaymentRequestContractClient::new(&env, &contract_id);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_zero_exp");

    env.ledger().set_timestamp(500);
    // expiry = 0 <= timestamp = 500 → must reject
    client.create_payment(&payment_id, &merchant, &100, &asset, &0);
}

#[test]
#[should_panic(expected = "expiry must be in the future")]
fn test_create_payment_rejects_past_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PaymentRequestContract);
    let client = PaymentRequestContractClient::new(&env, &contract_id);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_past_exp");

    env.ledger().set_timestamp(1000);
    // expiry = 999 <= timestamp = 1000 → must reject
    client.create_payment(&payment_id, &merchant, &100, &asset, &999);
}

#[test]
#[should_panic(expected = "expiry must be in the future")]
fn test_create_payment_rejects_equal_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PaymentRequestContract);
    let client = PaymentRequestContractClient::new(&env, &contract_id);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_equal_exp");

    env.ledger().set_timestamp(1000);
    // expiry = 1000 == timestamp = 1000 → must reject (expiry must be strictly greater)
    client.create_payment(&payment_id, &merchant, &100, &asset, &1000);
}

#[test]
fn test_create_payment_accepts_future_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PaymentRequestContract);
    let client = PaymentRequestContractClient::new(&env, &contract_id);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_future_exp");

    env.ledger().set_timestamp(500);
    // expiry = 1000 > timestamp = 500 → must succeed
    client.create_payment(&payment_id, &merchant, &100, &asset, &1000);
    assert_eq!(client.get_payment(&payment_id).status, PaymentStatus::Pending);
}

#[test]
#[should_panic(expected = "payment id must not be empty")]
fn test_empty_payment_id_rejected() {
    let env = Env::default();
    let (client, _admin) = setup_contract(&env);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    client.create_payment(&String::from_str(&env, ""), &merchant, &100, &asset, &1000);
}

// ---------------------------------------------------------------------------
// Issue #237: Admin authority rotation
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_admin_rotates_authority() {
    let env = Env::default();
    let (client, admin) = setup_contract(&env);

    let new_admin = Address::generate(&env);

    // Rotate admin authority from the original admin to the new admin.
    client.transfer_admin(&admin, &new_admin);

    // The new admin can now perform admin-gated operations.
    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_rotated");

    client.create_payment(&payment_id, &merchant, &100, &asset, &1000);
    client.confirm(&new_admin, &payment_id);
    assert_eq!(client.get_payment(&payment_id).status, PaymentStatus::Confirmed);
}

#[test]
#[should_panic]
fn test_transfer_admin_rejects_non_admin_caller() {
    let env = Env::default();
    let (client, _admin) = setup_contract(&env);

    let attacker = Address::generate(&env);
    let new_admin = Address::generate(&env);

    // A non-admin caller must not be able to rotate admin authority.
    client.transfer_admin(&attacker, &new_admin);
}

#[test]
#[should_panic]
fn test_old_admin_loses_authority_after_transfer() {
    let env = Env::default();
    let (client, admin) = setup_contract(&env);

    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);

    // The previous admin must no longer be able to perform admin-gated calls.
    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_old_admin");

    client.create_payment(&payment_id, &merchant, &100, &asset, &1000);
    client.confirm(&admin, &payment_id);
}
