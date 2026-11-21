#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    vec, xdr::ToXdr, Address, BytesN, Env, String,
};

use crate::{AdminTransferredEvent, SettlementLedgerContract, SettlementLedgerContractClient, SettlementVoidedEvent};

fn pid(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn fiat_ref(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

fn setup() -> (Env, Address, SettlementLedgerContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_700_000_001);
    let admin = Address::generate(&env);
    let id = env.register(SettlementLedgerContract, (&admin,));
    let client = SettlementLedgerContractClient::new(&env, &id);
    (env, admin, client)
}

fn record(
    client: &SettlementLedgerContractClient,
    admin: &Address,
    env: &Env,
    seed: u8,
    merchant: &Address,
    amount: i128,
    fee: i128,
) {
    client.record_settlement(
        admin,
        &pid(env, seed),
        merchant,
        &amount,
        &fee,
        &(amount - fee),
        &1_700_000_000u64,
        &fiat_ref(env, "REF-001"),
    );
}

// ── basic record + get ────────────────────────────────────────────────────────

#[test]
fn test_record_and_get() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    client.record_settlement(
        &admin,
        &pid(&env, 1),
        &merchant,
        &10_000,
        &200,
        &9_800,
        &1_700_000_000u64,
        &fiat_ref(&env, "TXN-ABC"),
    );

    let r = client.get_settlement(&pid(&env, 1));
    assert_eq!(r.amount, 10_000);
    assert_eq!(r.fee, 200);
    assert_eq!(r.net, 9_800);
    assert_eq!(r.timestamp, 1_700_000_000u64);
}

// ── administration ──────────────────────────────────────────────────────────

#[test]
fn test_get_admin_returns_deployment_admin() {
    let (env, admin, client) = setup();
    env.set_auths(&[]);

    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_get_admin_returns_rotated_admin() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    env.set_auths(&[]);

    assert_eq!(client.get_admin(), new_admin);
}

#[test]
fn test_admin_rotation_allows_new_admin_to_record() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);
    let merchant = Address::generate(&env);

    client.transfer_admin(&admin, &new_admin);
    record(&client, &new_admin, &env, 5, &merchant, 10_000, 200);

    assert_eq!(client.get_merchant_settlement_count(&merchant), 1);
}

#[test]
fn test_transfer_admin_emits_event() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);

    client.transfer_admin(&admin, &new_admin);

    let expected = AdminTransferredEvent {
        old_admin: admin.clone(),
        new_admin: new_admin.clone(),
    };

    let expected_topics = vec![
        &env,
        String::from_str(&env, "SETTLEMENT_LEDGER"),
        String::from_str(&env, "admin_transferred"),
    ]
    .to_xdr(&env);
    let expected_data = expected.to_xdr(&env);
    let events = env.events().all();
    let found = events.iter().any(|(contract, topics, data)| {
        contract == client.address
            && topics.to_xdr(&env) == expected_topics
            && data.to_xdr(&env) == expected_data
    });
    assert!(found, "AdminTransferredEvent not emitted");
}

// ── immutability: duplicate write must panic ──────────────────────────────────

#[test]
fn test_duplicate_record_panics() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    record(&client, &admin, &env, 2, &merchant, 5_000, 100);

    let result = client.try_record_settlement(
        &admin,
        &pid(&env, 2),
        &merchant,
        &5_000,
        &100,
        &4_900,
        &1_700_000_001u64,
        &fiat_ref(&env, "DUP"),
    );
    assert!(result.is_err());
}

#[test]
#[should_panic(expected = "caller is not admin")]
fn test_record_settlement_unauthorized_caller_panics() {
    let (env, _admin, client) = setup();
    let caller = Address::generate(&env);
    let merchant = Address::generate(&env);
    record(&client, &caller, &env, 8, &merchant, 1_000, 20);
}

#[test]
fn test_void_preserves_original_settlement() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    record(&client, &admin, &env, 7, &merchant, 1_000, 20);
    client.void_settlement(&admin, &pid(&env, 7), &fiat_ref(&env, "operator correction"));
    assert_eq!(client.get_settlement(&pid(&env, 7)).amount, 1_000);
    assert_eq!(client.get_void(&pid(&env, 7)).unwrap().reason, fiat_ref(&env, "operator correction"));
}

#[test]
fn test_void_settlement_emits_event() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    record(&client, &admin, &env, 9, &merchant, 1_000, 20);

    let reason = fiat_ref(&env, "operator correction");
    client.void_settlement(&admin, &pid(&env, 9), &reason);

    let voided_at = env.ledger().timestamp();
    let expected = SettlementVoidedEvent {
        payment_id: pid(&env, 9),
        reason: reason.clone(),
        voided_at,
    };

    let expected_topics = vec![
        &env,
        String::from_str(&env, "SETTLEMENT_LEDGER"),
        String::from_str(&env, "settlement_voided"),
    ]
    .to_xdr(&env);
    let expected_data = expected.to_xdr(&env);
    let events = env.events().all();
    let found = events.iter().any(|(contract, topics, data)| {
        contract == client.address
            && topics.to_xdr(&env) == expected_topics
            && data.to_xdr(&env) == expected_data
    });
    assert!(found, "SettlementVoidedEvent not emitted");
}

// ── fee + net must equal amount ───────────────────────────────────────────────

#[test]
fn test_invariant_fee_plus_net_equals_amount() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    let result = client.try_record_settlement(
        &admin,
        &pid(&env, 3),
        &merchant,
        &10_000,
        &300,
        &9_500, // wrong: 300 + 9500 != 10000
        &1_700_000_000u64,
        &fiat_ref(&env, "BAD"),
    );
    assert!(result.is_err());
}

// ── zero fee (fee-free merchant) ──────────────────────────────────────────────

#[test]
fn test_zero_fee() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    client.record_settlement(
        &admin,
        &pid(&env, 4),
        &merchant,
        &8_000,
        &0,
        &8_000,
        &1_700_000_000u64,
        &fiat_ref(&env, "FREE"),
    );

    let r = client.get_settlement(&pid(&env, 4));
    assert_eq!(r.fee, 0);
    assert_eq!(r.net, 8_000);
}

// ── pagination ────────────────────────────────────────────────────────────────

#[test]
fn test_list_settlements_pagination() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    // Record 25 settlements for the same merchant (seeds 10..34)
    for seed in 10u8..35 {
        record(&client, &admin, &env, seed, &merchant, 1_000, 20);
    }

    assert_eq!(client.get_merchant_settlement_count(&merchant), 25);

    let page0 = client.get_merchant_settlement_page(&merchant, &0);
    assert_eq!(page0.len(), 20); // full page

    let page1 = client.get_merchant_settlement_page(&merchant, &1);
    assert_eq!(page1.len(), 5); // remainder

    let page2 = client.get_merchant_settlement_page(&merchant, &2);
    assert_eq!(page2.len(), 0); // beyond end
}

// ── different merchants are isolated ─────────────────────────────────────────

#[test]
fn test_merchant_isolation() {
    let (env, admin, client) = setup();
    let m1 = Address::generate(&env);
    let m2 = Address::generate(&env);

    record(&client, &admin, &env, 50, &m1, 2_000, 40);
    record(&client, &admin, &env, 51, &m1, 2_000, 40);
    record(&client, &admin, &env, 52, &m2, 3_000, 60);

    assert_eq!(client.get_merchant_settlement_count(&m1), 2);
    assert_eq!(client.get_merchant_settlement_count(&m2), 1);
}

#[test]
fn test_platform_wide_index_lists_every_merchant() {
    let (env, admin, client) = setup();
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    record(&client, &admin, &env, 60, &first, 1_000, 20);
    record(&client, &admin, &env, 61, &second, 2_000, 40);
    assert_eq!(client.get_total_settlement_count(), 2);
    assert_eq!(client.get_all_settlement_page(&0).len(), 2);
}

// ── get on unknown payment_id panics ─────────────────────────────────────────

#[test]
fn test_get_unknown_panics() {
    let (env, _admin, client) = setup();
    let result = client.try_get_settlement(&pid(&env, 99));
    assert!(result.is_err());
}

// ── AllPage pagination: bounded storage per key ───────────────────────────────

/// Records enough settlements to span multiple AllPage keys and asserts that
/// no single storage entry grows unbounded with total settlement count, that
/// get_total_settlement_count is O(1), and that get_all_settlement_page reads only
/// the requested page.
#[test]
fn test_platform_wide_index_uses_bounded_pages() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    // Record 45 settlements: seeds 100..145
    // With PAGE_SIZE = 20 this creates page 0 (20 entries), page 1 (20 entries),
    // page 2 (5 entries).
    for seed in 100u8..145 {
        record(&client, &admin, &env, seed, &merchant, 1_000, 20);
    }

    // Scalar count must be exact.
    assert_eq!(client.get_total_settlement_count(), 45);

    // Each page must be bounded by PAGE_SIZE.
    assert_eq!(client.get_all_settlement_page(&0).len(), 20);
    assert_eq!(client.get_all_settlement_page(&1).len(), 20);
    assert_eq!(client.get_all_settlement_page(&2).len(), 5);
    assert_eq!(client.get_all_settlement_page(&3).len(), 0);
}
