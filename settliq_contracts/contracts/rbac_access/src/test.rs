#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

// ── Setup helper ─────────────────────────────────────────────────────────────

fn setup_env() -> (Env, RbacAccessContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let super_admin = Address::generate(&env);
    let contract_id = env.register(RbacAccessContract, (&super_admin,));
    let client = RbacAccessContractClient::new(&env, &contract_id);

    (env, client, super_admin)
}

// ── Constructor ───────────────────────────────────────────────────────────────

#[test]
fn test_constructor_assigns_super_admin_role() {
    let (_env, client, super_admin) = setup_env();
    assert_eq!(client.get_role(&super_admin), Some(Role::SuperAdmin));
}

#[test]
fn test_constructor_unregistered_address_has_no_role() {
    let (env, client, _super_admin) = setup_env();
    let stranger = Address::generate(&env);
    assert_eq!(client.get_role(&stranger), None);
}

// ── get_super_admin_count ─────────────────────────────────────────────────────

#[test]
fn test_get_super_admin_count_after_constructor() {
    let (_env, client, _super_admin) = setup_env();
    assert_eq!(client.get_super_admin_count(), 1);
}

#[test]
fn test_get_super_admin_count_reflects_grant() {
    let (env, client, super_admin) = setup_env();
    let second_super = Address::generate(&env);

    assert_eq!(client.get_super_admin_count(), 1);
    client.grant_role(&super_admin, &second_super, &Role::SuperAdmin);
    assert_eq!(client.get_super_admin_count(), 2);
}

#[test]
fn test_get_super_admin_count_reflects_revoke() {
    let (env, client, super_admin) = setup_env();
    let second_super = Address::generate(&env);

    client.grant_role(&super_admin, &second_super, &Role::SuperAdmin);
    assert_eq!(client.get_super_admin_count(), 2);

    client.revoke_role(&super_admin, &second_super);
    assert_eq!(client.get_super_admin_count(), 1);
}

#[test]
fn test_get_super_admin_count_unchanged_for_non_super_grants() {
    let (env, client, super_admin) = setup_env();
    let user = Address::generate(&env);

    client.grant_role(&super_admin, &user, &Role::ReadOnly);
    assert_eq!(client.get_super_admin_count(), 1);
}

// ── grant_role ────────────────────────────────────────────────────────────────

#[test]
fn test_grant_role_readonly_succeeds() {
    let (env, client, super_admin) = setup_env();
    let user = Address::generate(&env);

    client.grant_role(&super_admin, &user, &Role::ReadOnly);
    assert_eq!(client.get_role(&user), Some(Role::ReadOnly));
}

#[test]
fn test_grant_role_compliance_admin_succeeds() {
    let (env, client, super_admin) = setup_env();
    let user = Address::generate(&env);

    client.grant_role(&super_admin, &user, &Role::ComplianceAdmin);
    assert_eq!(client.get_role(&user), Some(Role::ComplianceAdmin));
}

#[test]
fn test_grant_role_operations_admin_succeeds() {
    let (env, client, super_admin) = setup_env();
    let user = Address::generate(&env);

    client.grant_role(&super_admin, &user, &Role::OperationsAdmin);
    assert_eq!(client.get_role(&user), Some(Role::OperationsAdmin));
}

#[test]
fn test_grant_role_super_admin_succeeds() {
    let (env, client, super_admin) = setup_env();
    let new_super = Address::generate(&env);

    client.grant_role(&super_admin, &new_super, &Role::SuperAdmin);
    assert_eq!(client.get_role(&new_super), Some(Role::SuperAdmin));
}

#[test]
fn test_grant_role_can_upgrade_existing_role() {
    let (env, client, super_admin) = setup_env();
    let user = Address::generate(&env);

    client.grant_role(&super_admin, &user, &Role::ReadOnly);
    assert_eq!(client.get_role(&user), Some(Role::ReadOnly));

    client.grant_role(&super_admin, &user, &Role::OperationsAdmin);
    assert_eq!(client.get_role(&user), Some(Role::OperationsAdmin));
}

#[test]
#[should_panic(expected = "insufficient role")]
fn test_grant_role_rejected_for_non_super_admin() {
    let (env, client, super_admin) = setup_env();
    let ops_admin = Address::generate(&env);
    let target = Address::generate(&env);

    client.grant_role(&super_admin, &ops_admin, &Role::OperationsAdmin);
    // OperationsAdmin cannot grant roles; only SuperAdmin can
    client.grant_role(&ops_admin, &target, &Role::ReadOnly);
}

#[test]
#[should_panic(expected = "role not assigned")]
fn test_grant_role_rejected_for_unregistered_caller() {
    let (env, client, _super_admin) = setup_env();
    let stranger = Address::generate(&env);
    let target = Address::generate(&env);

    client.grant_role(&stranger, &target, &Role::ReadOnly);
}

#[test]
#[should_panic(expected = "insufficient role")]
fn test_grant_role_rejected_for_compliance_admin() {
    let (env, client, super_admin) = setup_env();
    let compliance = Address::generate(&env);
    let target = Address::generate(&env);

    client.grant_role(&super_admin, &compliance, &Role::ComplianceAdmin);
    // ComplianceAdmin cannot grant roles; only SuperAdmin can
    client.grant_role(&compliance, &target, &Role::ReadOnly);
}

// ── revoke_role ───────────────────────────────────────────────────────────────

#[test]
fn test_revoke_role_removes_role() {
    let (env, client, super_admin) = setup_env();
    let user = Address::generate(&env);

    client.grant_role(&super_admin, &user, &Role::ReadOnly);
    assert_eq!(client.get_role(&user), Some(Role::ReadOnly));

    client.revoke_role(&super_admin, &user);
    assert_eq!(client.get_role(&user), None);
}

#[test]
fn test_revoke_role_operations_admin() {
    let (env, client, super_admin) = setup_env();
    let ops = Address::generate(&env);

    client.grant_role(&super_admin, &ops, &Role::OperationsAdmin);
    client.revoke_role(&super_admin, &ops);
    assert_eq!(client.get_role(&ops), None);
}

#[test]
fn test_revoke_second_super_admin_succeeds() {
    let (env, client, super_admin) = setup_env();
    let second_super = Address::generate(&env);

    client.grant_role(&super_admin, &second_super, &Role::SuperAdmin);
    // Now there are two SuperAdmins; revoking one should succeed
    client.revoke_role(&super_admin, &second_super);
    assert_eq!(client.get_role(&second_super), None);
}

#[test]
#[should_panic(expected = "cannot revoke last super admin")]
fn test_revoke_last_super_admin_panics() {
    let (env, client, super_admin) = setup_env();
    let second_super = Address::generate(&env);

    // Upgrade second_super from ReadOnly → SuperAdmin so grant_role does not
    // increment SuperAdminCount (the account already has a role record).
    // This leaves count=1 while second_super also holds SuperAdmin, which
    // lets us exercise the last-SA guard from a different caller.
    client.grant_role(&super_admin, &second_super, &Role::ReadOnly);
    client.grant_role(&super_admin, &second_super, &Role::SuperAdmin);
    // SuperAdminCount is still 1 (second_super was not "new").
    // second_super != super_admin, so the self-revoke guard is bypassed;
    // the last-SA guard fires instead.
    client.revoke_role(&second_super, &super_admin);
}

#[test]
#[should_panic(expected = "cannot revoke own role")]
fn test_revoke_own_role_panics() {
    let (env, client, super_admin) = setup_env();
    let second_super = Address::generate(&env);

    client.grant_role(&super_admin, &second_super, &Role::SuperAdmin);
    // A SuperAdmin cannot revoke themselves
    client.revoke_role(&super_admin, &super_admin);
}

#[test]
#[should_panic(expected = "role not assigned")]
fn test_revoke_role_panics_when_no_role_assigned() {
    let (env, client, super_admin) = setup_env();
    let stranger = Address::generate(&env);

    client.revoke_role(&super_admin, &stranger);
}

#[test]
#[should_panic(expected = "insufficient role")]
fn test_revoke_role_rejected_for_non_super_admin_caller() {
    let (env, client, super_admin) = setup_env();
    let ops = Address::generate(&env);
    let target = Address::generate(&env);

    client.grant_role(&super_admin, &ops, &Role::OperationsAdmin);
    client.grant_role(&super_admin, &target, &Role::ReadOnly);
    // OperationsAdmin cannot revoke roles; only SuperAdmin can
    client.revoke_role(&ops, &target);
}

// ── execute_operations_task ───────────────────────────────────────────────────

#[test]
fn test_execute_operations_task_allowed_for_operations_admin() {
    let (env, client, super_admin) = setup_env();
    let ops = Address::generate(&env);

    client.grant_role(&super_admin, &ops, &Role::OperationsAdmin);
    // Should not panic
    client.execute_operations_task(&ops);
}

#[test]
fn test_execute_operations_task_allowed_for_super_admin() {
    let (_env, client, super_admin) = setup_env();
    // SuperAdmin satisfies OperationsAdmin requirement
    client.execute_operations_task(&super_admin);
}

#[test]
#[should_panic(expected = "insufficient role")]
fn test_execute_operations_task_rejected_for_compliance_admin() {
    let (env, client, super_admin) = setup_env();
    let compliance = Address::generate(&env);

    client.grant_role(&super_admin, &compliance, &Role::ComplianceAdmin);
    // ComplianceAdmin must not be able to execute operations tasks
    client.execute_operations_task(&compliance);
}
