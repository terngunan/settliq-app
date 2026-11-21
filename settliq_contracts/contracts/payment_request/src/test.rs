#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _, Events as _, Ledger}, Address, Env, IntoVal, String, Symbol, TryFromVal};

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
fn test_created_event_payload_is_rich() {
    let env = Env::default();
    let (client, _admin) = setup_contract(&env);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_evt_created");
    let amount = 4242i128;
    let expiry = 5000u64;

    env.ledger().set_timestamp(0);
    client.create_payment(&payment_id, &merchant, &amount, &asset, &expiry);

    let events = env.events().all();
    let (topics, data) = events.last().expect("created event must be emitted");

    // Topic structure: ("payment", id, "created")
    let expected_topics = (
        symbol_short!("payment"),
        payment_id.clone(),
        symbol_short!("created"),
    );
    assert_eq!(topics, expected_topics.into_val(&env));

    // Payload carries merchant, amount, asset, expiry, status
    let payload = PaymentCreatedEvent::try_from_val(&env, &data)
        .expect("created payload must decode as PaymentCreatedEvent");
    assert_eq!(payload.merchant, merchant);
    assert_eq!(payload.amount, amount);
    assert_eq!(payload.asset, asset);
    assert_eq!(payload.expiry, expiry);
    assert_eq!(payload.status, PaymentStatus::Pending);
}

#[test]
fn test_lifecycle_events_include_amount_asset_merchant() {
    let env = Env::default();
    let (client, admin) = setup_contract(&env);

    let merchant = Address::generate(&env);
    let asset = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_evt_lifecycle");
    let amount = 777i128;

    env.ledger().set_timestamp(0);
    client.create_payment(&payment_id, &merchant, &amount, &asset, &1000);
    client.confirm(&admin, &payment_id);
    client.set_settling(&admin, &payment_id);
    client.settle(&admin, &payment_id);

    let events = env.events().all();
    let mut seen: u32 = 0;
    for (topics, data) in events.iter() {
        let topic_vec = soroban_sdk::Vec::<soroban_sdk::Val>::try_from_val(&env, &topics)
            .expect("topics must be a vec");
        if topic_vec.len() != 3 {
            continue;
        }
        let kind = Symbol::try_from_val(&env, &topic_vec.get(2).unwrap()).unwrap();
        if kind != symbol_short!("created")
            && kind != symbol_short!("confirmed")
            && kind != symbol_short!("settling")
            && kind != symbol_short!("settled")
        {
            continue;
        }
        let payload = PaymentStatusEvent::try_from_val(&env, &data)
            .expect("lifecycle payload must decode as PaymentStatusEvent");
        assert_eq!(payload.merchant, merchant);
        assert_eq!(payload.amount, amount);
        assert_eq!(payload.asset, asset);
        seen += 1;
    }
    assert_eq!(seen, 4, "created/confirmed/settling/settled events must all be rich");
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
