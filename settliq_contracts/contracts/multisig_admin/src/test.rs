#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, vec, Address, Bytes, Env, String};

fn setup_env() -> (Env, MultisigAdminContractClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    let contract_id = env.register(MultisigAdminContract, (&admin1, &admin2, &admin3));
    let client = MultisigAdminContractClient::new(&env, &contract_id);

    (env, client, admin1, admin2, admin3)
}

fn op(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

fn args(env: &Env) -> Bytes {
    Bytes::from_slice(env, b"payload")
}

#[test]
fn test_propose_and_approve_by_admin1_and_admin2() {
    let (env, client, admin1, admin2, _admin3) = setup_env();

    let id = client.propose(&admin1, &op(&env, "transfer"), &args(&env));
    assert_eq!(id, 0);

    let proposal = client.get_proposal(&id).unwrap();
    assert_eq!(proposal.approvals.len(), 1);
    assert!(!proposal.executed);

    client.approve(&admin2, &id);

    let proposal = client.get_proposal(&id).unwrap();
    assert_eq!(proposal.approvals.len(), 2);
    assert!(proposal.executed);
}

#[test]
fn test_admin3_proposes_and_admin1_approves() {
    let (env, client, admin1, _admin2, admin3) = setup_env();

    let id = client.propose(&admin3, &op(&env, "transfer"), &args(&env));
    assert_eq!(id, 0);

    let proposal = client.get_proposal(&id).unwrap();
    assert_eq!(proposal.proposer, admin3);
    assert_eq!(proposal.approvals.len(), 1);
    assert!(!proposal.executed);

    client.approve(&admin1, &id);

    let proposal = client.get_proposal(&id).unwrap();
    assert_eq!(proposal.approvals.len(), 2);
    assert!(proposal.executed);
}

#[test]
fn test_admin1_proposes_and_admin3_approves() {
    let (env, client, admin1, _admin2, admin3) = setup_env();

    let id = client.propose(&admin1, &op(&env, "transfer"), &args(&env));
    assert_eq!(id, 0);

    client.approve(&admin3, &id);

    let proposal = client.get_proposal(&id).unwrap();
    assert_eq!(proposal.approvals.len(), 2);
    assert!(proposal.executed);
}

#[test]
#[should_panic(expected = "proposal already executed")]
fn test_admin3_cannot_approve_executed_proposal() {
    let (env, client, admin1, admin2, admin3) = setup_env();

    let id = client.propose(&admin1, &op(&env, "transfer"), &args(&env));
    client.approve(&admin2, &id);

    let proposal = client.get_proposal(&id).unwrap();
    assert!(proposal.executed);

    client.approve(&admin3, &id);
}

#[test]
#[should_panic(expected = "proposal already executed")]
fn test_admin1_cannot_approve_executed_proposal() {
    let (env, client, admin1, admin2, admin3) = setup_env();

    let id = client.propose(&admin3, &op(&env, "transfer"), &args(&env));
    client.approve(&admin2, &id);

    let proposal = client.get_proposal(&id).unwrap();
    assert!(proposal.executed);

    client.approve(&admin1, &id);
}

#[test]
fn test_any_two_of_three_admins_reach_threshold() {
    let (env, client, admin1, admin2, admin3) = setup_env();

    let id = client.propose(&admin2, &op(&env, "transfer"), &args(&env));
    client.approve(&admin3, &id);

    let proposal = client.get_proposal(&id).unwrap();
    assert_eq!(proposal.approvals.len(), 2);
    assert!(proposal.executed);

    let admins = client.get_admins();
    assert_eq!(admins, vec![&env, admin1, admin2, admin3]);
}

#[test]
fn test_admin_rotation_requires_threshold_approval() {
    let (env, client, admin1, admin2, _admin3) = setup_env();
    let add_admin_args = Bytes::from_slice(&env, b"CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAOLZM");

    let proposal_id = client.propose(&admin1, &op(&env, "add_admin"), &add_admin_args);

    let proposal_before = client.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal_before.approvals.len(), 1);
    assert!(!proposal_before.executed);

    client.approve(&admin2, &proposal_id);

    let proposal_after = client.get_proposal(&proposal_id).unwrap();
    assert!(proposal_after.executed);
}

#[test]
fn test_threshold_change_requires_threshold_approval() {
    let (env, client, admin1, admin2, _admin3) = setup_env();
    let threshold_args = Bytes::from_slice(&env, &2u32.to_be_bytes());

    let proposal_id = client.propose(&admin1, &op(&env, "set_threshold"), &threshold_args);

    let proposal_before = client.get_proposal(&proposal_id).unwrap();
    assert_eq!(proposal_before.approvals.len(), 1);
    assert!(!proposal_before.executed);

    client.approve(&admin2, &proposal_id);

    let proposal_after = client.get_proposal(&proposal_id).unwrap();
    assert!(proposal_after.executed);
}

#[test]
#[should_panic(expected = "Not admin")]
fn test_non_admin_cannot_propose_admin_change() {
    let (env, client, _admin1, _admin2, _admin3) = setup_env();
    let outsider = Address::generate(&env);

    client.propose(&outsider, &op(&env, "add_admin"), &args(&env));
}

// ── Issue #1: maybe_execute must not execute or emit below threshold ──────────

/// With the default threshold of 2, a proposal with only one approval (the
/// proposer) must remain un-executed.  The old bug unconditionally set
/// `executed = true` outside the threshold guard, so this test would have
/// failed before the fix.
#[test]
fn test_proposal_below_threshold_remains_unexecuted() {
    let (env, client, admin1, _admin2, _admin3) = setup_env();

    let id = client.propose(&admin1, &op(&env, "transfer"), &args(&env));

    let proposal = client.get_proposal(&id).unwrap();
    assert_eq!(proposal.approvals.len(), 1);
    assert!(!proposal.executed, "proposal must NOT be executed with only 1 approval");
}

/// With threshold raised to 3, a proposal with 2 approvals must still be
/// non-executed.  This validates the threshold-from-storage path (Issue #1).
#[test]
fn test_proposal_not_executed_with_two_approvals_when_threshold_is_three() {
    let (env, client, admin1, admin2, admin3) = setup_env();

    // Raise threshold to 3 via a properly-approved set_threshold proposal.
    let threshold_args = Bytes::from_slice(&env, &3u32.to_be_bytes());
    let setup_id = client.propose(&admin1, &op(&env, "set_threshold"), &threshold_args);
    client.approve(&admin2, &setup_id);
    // Confirm threshold was changed.
    assert_eq!(client.get_threshold(), 3);

    // Now propose a new operation; admin1 is proposer (1 approval).
    let id = client.propose(&admin2, &op(&env, "transfer"), &args(&env));

    let proposal_after_propose = client.get_proposal(&id).unwrap();
    assert!(!proposal_after_propose.executed, "must not execute at 1 approval");

    // Second approval: admin3.
    client.approve(&admin3, &id);

    let proposal_after_second = client.get_proposal(&id).unwrap();
    assert!(!proposal_after_second.executed, "must not execute at 2 approvals when threshold is 3");

    // Third approval: admin1.
    client.approve(&admin1, &id);

    let proposal_after_third = client.get_proposal(&id).unwrap();
    assert!(proposal_after_third.executed, "must execute at 3 approvals");
}

// ── Issue #2: set_threshold must reject values below 2 ───────────────────────

/// Attempting to execute a set_threshold proposal with argument 1 must panic.
/// The security invariant requires threshold >= 2 at all times.
#[test]
fn test_set_threshold_to_one_panics() {
    let (env, client, admin1, admin2, _admin3) = setup_env();
    let bad_args = Bytes::from_slice(&env, &1u32.to_be_bytes());

    let id = client.propose(&admin1, &op(&env, "set_threshold"), &bad_args);

    let result = client.try_approve(&admin2, &id);
    assert!(result.is_err(), "set_threshold(1) must panic with threshold must be between 2 and admin count");
}

/// Setting threshold to a valid value (2 with 3 admins) must succeed.
#[test]
fn test_set_threshold_to_two_succeeds() {
    let (env, client, admin1, admin2, _admin3) = setup_env();
    let valid_args = Bytes::from_slice(&env, &2u32.to_be_bytes());

    let id = client.propose(&admin1, &op(&env, "set_threshold"), &valid_args);
    client.approve(&admin2, &id);

    let proposal = client.get_proposal(&id).unwrap();
    assert!(proposal.executed);
    assert_eq!(client.get_threshold(), 2);
}

// ── Issue #229: decode_address must validate StrKey before decoding ──────────

/// A raw 32-byte public key (not a 56-byte ASCII StrKey) must be rejected at
/// proposal creation time with a descriptive panic, rather than panicking
/// later during execution and leaving the approved proposal stuck.
#[test]
#[should_panic(expected = "invalid address: expected 56-byte StrKey string")]
fn test_propose_add_admin_with_raw_pubkey_bytes_panics() {
    let (env, client, admin1, _admin2, _admin3) = setup_env();

    // 32 raw bytes — the shape of a raw public key, not a StrKey string.
    let raw_pubkey = Bytes::from_slice(&env, &[0u8; 32]);

    client.propose(&admin1, &op(&env, "add_admin"), &raw_pubkey);
}

/// A 56-byte payload that is not a valid StrKey (wrong leading character) must
/// also be rejected at proposal creation time.
#[test]
#[should_panic(expected = "invalid address: not a valid StrKey")]
fn test_propose_add_admin_with_bad_strkey_prefix_panics() {
    let (env, client, admin1, _admin2, _admin3) = setup_env();

    // 56 bytes but leading byte is not 'G' or 'C'.
    let mut buf = [b'X'; 56];
    buf[0] = b'X';
    let bad = Bytes::from_slice(&env, &buf);

    client.propose(&admin1, &op(&env, "add_admin"), &bad);
}

/// A valid 56-byte StrKey address must still be accepted and executed.
#[test]
fn test_propose_add_admin_with_valid_strkey_succeeds() {
    let (env, client, admin1, admin2, _admin3) = setup_env();
    let valid = Bytes::from_slice(&env, b"CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAOLZM");

    let id = client.propose(&admin1, &op(&env, "add_admin"), &valid);
    client.approve(&admin2, &id);

    let proposal = client.get_proposal(&id).unwrap();
    assert!(proposal.executed);
}
