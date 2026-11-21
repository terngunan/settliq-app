#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    Address, BytesN, Env, IntoVal, Symbol, TryFromVal,
};

use crate::{
    ConfirmationFailedEvent, StellarConfirmationsContract, StellarConfirmationsContractClient,
};

fn pid(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn setup(required: u32) -> (Env, Address, StellarConfirmationsContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(10000);
    let admin = Address::generate(&env);
    let id = env.register(StellarConfirmationsContract, (&admin, required));
    let client = StellarConfirmationsContractClient::new(&env, &id);
    (env, admin, client)
}

// helper: call confirm_payment n times
fn confirm_n(
    client: &StellarConfirmationsContractClient,
    admin: &Address,
    payment_id: &BytesN<32>,
    merchant: &Address,
    n: u32,
) -> u32 {
    let mut last = 0;
    for i in 0..n {
        last = client.confirm_payment(admin, payment_id, &(1000 + i), &100_000, merchant);
    }
    last
}

#[test]
fn test_not_settling_before_threshold() {
    let (env, admin, client) = setup(3);
    let payment_id = pid(&env, 1);
    let merchant = Address::generate(&env);

    confirm_n(&client, &admin, &payment_id, &merchant, 2);

    assert_eq!(client.get_payment_confirmations(&payment_id), 2);
    assert!(!client.is_settling(&payment_id));
}

#[test]
fn test_settles_exactly_at_threshold() {
    let (env, admin, client) = setup(3);
    let payment_id = pid(&env, 2);
    let merchant = Address::generate(&env);

    let count = confirm_n(&client, &admin, &payment_id, &merchant, 3);

    assert_eq!(count, 3);
    assert!(client.is_settling(&payment_id));
}

#[test]
fn test_threshold_of_one() {
    let (env, admin, client) = setup(1);
    let payment_id = pid(&env, 3);
    let merchant = Address::generate(&env);

    client.confirm_payment(&admin, &payment_id, &1001, &50_000, &merchant);

    assert!(client.is_settling(&payment_id));
}

#[test]
fn test_extra_confirmations_after_threshold_panic() {
    let (env, admin, client) = setup(2);
    let payment_id = pid(&env, 4);
    let merchant = Address::generate(&env);

    confirm_n(&client, &admin, &payment_id, &merchant, 2);
    assert!(client.is_settling(&payment_id));

    // further calls must panic — payment already settling
    let result = client.try_confirm_payment(&admin, &payment_id, &1010, &50_000, &merchant);
    assert!(result.is_err());
}

#[test]
fn test_independent_payments_dont_interfere() {
    let (env, admin, client) = setup(3);
    let p1 = pid(&env, 5);
    let p2 = pid(&env, 6);
    let merchant = Address::generate(&env);

    confirm_n(&client, &admin, &p1, &merchant, 3);
    confirm_n(&client, &admin, &p2, &merchant, 1);

    assert!(client.is_settling(&p1));
    assert!(!client.is_settling(&p2));
    assert_eq!(client.get_payment_confirmations(&p2), 1);
}

#[test]
fn test_update_confirmation_count() {
    let (env, admin, client) = setup(5);
    client.set_confirmation_count(&admin, &2);
    assert_eq!(client.get_confirmation_count(), 2);

    let payment_id = pid(&env, 7);
    let merchant = Address::generate(&env);
    confirm_n(&client, &admin, &payment_id, &merchant, 2);
    assert!(client.is_settling(&payment_id));
}

#[test]
fn test_lowering_confirmation_count_does_not_affect_in_flight_payments() {
    // Required confirmation count is snapshotted at first confirmation. Lowering
    // global ConfirmationCount mid-flight does not reduce the required threshold
    // for already-in-flight payments.
    let (env, admin, client) = setup(5);
    let payment_id = pid(&env, 9);
    let merchant = Address::generate(&env);

    // Record 2 confirmations under initial threshold of 5.
    confirm_n(&client, &admin, &payment_id, &merchant, 2);
    assert!(!client.is_settling(&payment_id));

    // Admin lowers required confirmation count to 2 mid-flight.
    client.set_confirmation_count(&admin, &2);
    assert_eq!(client.get_confirmation_count(), 2);

    // A 3rd confirmation does not settle because snapshotted threshold is 5.
    client.confirm_payment(&admin, &payment_id, &2001, &100_000, &merchant);

    assert_eq!(client.get_payment_confirmations(&payment_id), 3);
    assert!(!client.is_settling(&payment_id));
}

#[test]
#[should_panic(expected = "not admin")]
fn test_set_confirmation_count_unauthorized_caller_panics() {
    let (env, _admin, client) = setup(5);
    let not_admin = Address::generate(&env);

    client.set_confirmation_count(&not_admin, &2);
}

#[test]
fn test_large_threshold() {
    let (env, admin, client) = setup(12);
    let payment_id = pid(&env, 8);
    let merchant = Address::generate(&env);

    confirm_n(&client, &admin, &payment_id, &merchant, 11);
    assert!(!client.is_settling(&payment_id));

    client.confirm_payment(&admin, &payment_id, &2000, &1_000_000, &merchant);
    assert!(client.is_settling(&payment_id));
}

// ── Issue #109: Audit events on admin setters ────────────────────────────────

#[test]
fn test_set_confirmation_count_emits_event() {
    let (env, admin, client) = setup(5);

    client.set_confirmation_count(&admin, &2);

    let events = env.events().all();
    let mut found = false;
    for (contract, topics, data) in events.iter() {
        if contract != client.address {
            continue;
        }
        if topics.len() != 1 {
            continue;
        }
        let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
        if topic != Symbol::new(&env, "confirmation_count_updated") {
            continue;
        }
        let (caller, old_value, new_value): (Address, u32, u32) =
            <(Address, u32, u32)>::try_from_val(&env, &data).unwrap();
        assert_eq!(caller, admin);
        assert_eq!(old_value, 5);
        assert_eq!(new_value, 2);
        found = true;
    }
    assert!(found, "ConfirmationCountUpdatedEvent not emitted");
}

#[test]
fn test_set_payment_escrow_contract_emits_event() {
    let (env, admin, client) = setup(3);
    let escrow = Address::generate(&env);

    client.set_payment_escrow_contract(&admin, &escrow);

    let events = env.events().all();
    let mut found = false;
    for (contract, topics, data) in events.iter() {
        if contract != client.address {
            continue;
        }
        if topics.len() != 1 {
            continue;
        }
        let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
        if topic != Symbol::new(&env, "payment_escrow_contract_updated") {
            continue;
        }
        let (caller, old_value, new_value): (Address, Option<Address>, Option<Address>) =
            <(Address, Option<Address>, Option<Address>)>::try_from_val(&env, &data).unwrap();
        assert_eq!(caller, admin);
        assert_eq!(old_value, None);
        assert_eq!(new_value, Some(escrow.clone()));
        found = true;
    }
    assert!(found, "PaymentEscrowContractUpdatedEvent not emitted");
}

// ── Issue #16: Strict monotonic ledger progression ───────────────────────────

#[test]
fn test_same_ledger_seq_rejected_on_second_confirmation() {
    // Issue #16: Submitting two confirmations for the same payment with the
    // same ledger_seq must be rejected on the second call. The old guard used
    // >= (allowing equal), so an attacker could reach the confirmation
    // threshold within a single ledger. The fix uses > (strictly greater).
    let (env, admin, client) = setup(3);
    let payment_id = pid(&env, 42);
    let merchant = Address::generate(&env);

    // First confirmation at ledger 1001 — must succeed.
    client.confirm_payment(&admin, &payment_id, &1001, &100_000, &merchant);
    assert_eq!(client.get_payment_confirmations(&payment_id), 1);

    // Second confirmation at the same ledger 1001 — must be rejected.
    let result = client.try_confirm_payment(&admin, &payment_id, &1001, &100_000, &merchant);
    assert!(result.is_err());
    assert_eq!(client.get_payment_confirmations(&payment_id), 1);
}

// ── Issue #261: Escrow release failure must not persist Confirmed state ──────

#[test]
fn test_escrow_release_failure_does_not_mark_settling() {
    // Issue #261: When the escrow release call fails/reverts, the confirmation
    // transaction must revert atomically. The payment must NOT be marked as
    // settling (Confirmed) and the confirmation count must not advance past
    // the pre-release state.
    let (env, admin, client) = setup(2);
    let payment_id = pid(&env, 61);
    let merchant = Address::generate(&env);

    // Point the contract at an escrow address that has no release entrypoint
    // registered, so the cross-contract release call reverts.
    let bad_escrow = Address::generate(&env);
    client.set_payment_escrow_contract(&admin, &bad_escrow);

    // First confirmation succeeds (below threshold).
    client.confirm_payment(&admin, &payment_id, &1001, &100_000, &merchant);
    assert_eq!(client.get_payment_confirmations(&payment_id), 1);
    assert!(!client.is_settling(&payment_id));

    // Second confirmation reaches threshold and triggers escrow release, which
    // fails. The whole call must revert.
    let result = client.try_confirm_payment(&admin, &payment_id, &1002, &100_000, &merchant);
    assert!(result.is_err(), "escrow release failure must revert confirm_payment");

    // Terminal Confirmed state must not be persisted.
    assert!(!client.is_settling(&payment_id));
    // Confirmation count must not have advanced past the pre-release state.
    assert_eq!(client.get_payment_confirmations(&payment_id), 1);
}

#[test]
fn test_escrow_release_failure_at_threshold_of_one() {
    // Issue #261: Even with a threshold of one, a failed escrow release must
    // revert the confirmation and leave the payment unconfirmed.
    let (env, admin, client) = setup(1);
    let payment_id = pid(&env, 62);
    let merchant = Address::generate(&env);

    let bad_escrow = Address::generate(&env);
    client.set_payment_escrow_contract(&admin, &bad_escrow);

    let result = client.try_confirm_payment(&admin, &payment_id, &1001, &50_000, &merchant);
    assert!(result.is_err(), "escrow release failure must revert confirm_payment");

    assert!(!client.is_settling(&payment_id));
    assert_eq!(client.get_payment_confirmations(&payment_id), 0);
}
