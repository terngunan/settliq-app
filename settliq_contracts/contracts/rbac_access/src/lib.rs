#![no_std]

mod test;

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env,
};

// ── Role ────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum Role {
    ReadOnly,
    ComplianceAdmin,
    OperationsAdmin,
    SuperAdmin,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Role(Address),
    SuperAdminCount,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RoleGrantedEvent {
    pub account: Address,
    pub role: Role,
    pub granted_by: Address,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RoleRevokedEvent {
    pub account: Address,
    pub revoked_by: Address,
}

#[contract]
pub struct RbacAccessContract;

#[contractimpl]
impl RbacAccessContract {
    pub fn __constructor(env: Env, super_admin: Address) {
        // Issue #1026: Store roles in persistent() instead of instance()
        env.storage()
            .persistent()
            .set(&DataKey::Role(super_admin), &Role::SuperAdmin);
        env.storage()
            .persistent()
            .set(&DataKey::SuperAdminCount, &1u32);
    }

    pub fn grant_role(env: Env, caller: Address, account: Address, role: Role) {
        caller.require_auth();
        Self::require_role(&env, &caller, Role::SuperAdmin);

        // Read the account's existing role (if any) before overwriting it.
        // This is required to correctly maintain SuperAdminCount for both:
        //   - Issue #13: upgrading an existing non-SuperAdmin to SuperAdmin
        //     (has() was true so the old code never incremented the count)
        //   - Issue #14: demoting an existing SuperAdmin to a lower role
        //     (the old code never decremented the count on demotion via grant_role)
        let existing_role: Option<Role> = env
            .storage()
            .persistent()
            .get(&DataKey::Role(account.clone()));

        let is_becoming_super_admin =
            role == Role::SuperAdmin && existing_role != Some(Role::SuperAdmin);

        let is_demoting_super_admin =
            existing_role == Some(Role::SuperAdmin) && role != Role::SuperAdmin;

        // Issue #14: block demoting the last SuperAdmin or self-demotion via grant_role.
        if is_demoting_super_admin {
            if caller == account {
                panic!("cannot demote own super admin role");
            }
            let count: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::SuperAdminCount)
                .unwrap_or(0);
            if count <= 1 {
                panic!("cannot demote last super admin");
            }
        }

        env.storage()
            .persistent()
            .set(&DataKey::Role(account.clone()), &role);

        // Issue #13: increment count when any account (new or pre-existing) becomes SuperAdmin.
        if is_becoming_super_admin {
            let count: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::SuperAdminCount)
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&DataKey::SuperAdminCount, &(count + 1));
        }

        // Issue #14: decrement count when an existing SuperAdmin is demoted.
        if is_demoting_super_admin {
            let count: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::SuperAdminCount)
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(
                    &DataKey::SuperAdminCount,
                    &count.checked_sub(1).expect("SuperAdminCount underflow"),
                );
        }

        env.events().publish(
            ("RBAC", "role_granted"),
            RoleGrantedEvent {
                account,
                role,
                granted_by: caller,
            },
        );
    }

    pub fn revoke_role(env: Env, caller: Address, account: Address) {
        caller.require_auth();
        Self::require_role(&env, &caller, Role::SuperAdmin);

        if caller == account {
            panic!("cannot revoke own role");
        }

        let key = DataKey::Role(account.clone());
        if !env.storage().persistent().has(&key) {
            panic!("role not assigned");
        }

        let role: Role = env
            .storage()
            .persistent()
            .get(&key)
            .expect("role not assigned");

        if role == Role::SuperAdmin {
            let count: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::SuperAdminCount)
                .unwrap_or(0);
            if count <= 1 {
                panic!("cannot revoke last super admin");
            }
            env.storage()
                .persistent()
                .set(
                    &DataKey::SuperAdminCount,
                    &count.checked_sub(1).expect("SuperAdminCount underflow"),
                );
        }

        env.storage().persistent().remove(&key);
        env.events().publish(
            ("RBAC", "role_revoked"),
            RoleRevokedEvent {
                account,
                revoked_by: caller,
            },
        );
    }

    pub fn get_role(env: Env, account: Address) -> Option<Role> {
        env.storage().persistent().get(&DataKey::Role(account))
    }

    /// Sensitive operation requiring `OperationsAdmin` (or `SuperAdmin`).
    ///
    /// Reference implementation of the role-gating pattern: it enforces
    /// `require_auth()` plus the required role and performs no business logic.
    /// There is intentionally no state mutation or event emission -- integrators
    /// should model real sensitive operations on this pattern and add their own
    /// logic after the gate.
    pub fn execute_operations_task(env: Env, caller: Address) {
        caller.require_auth();
        Self::require_role(&env, &caller, Role::OperationsAdmin);
    }

    /// Sensitive operation requiring `ComplianceAdmin` (or `SuperAdmin`).
    ///
    /// Reference implementation of the role-gating pattern: it enforces
    /// `require_auth()` plus the required role and performs no business logic.
    /// There is intentionally no state mutation or event emission -- integrators
    /// should model real sensitive operations on this pattern and add their own
    /// logic after the gate.
    pub fn execute_compliance_task(env: Env, caller: Address) {
        caller.require_auth();
        Self::require_role(&env, &caller, Role::ComplianceAdmin);
    }

    /// Sensitive operation requiring `ReadOnly` (or any higher role).
    ///
    /// Reference implementation of the role-gating pattern: it enforces
    /// `require_auth()` plus the required role and performs no business logic.
    /// There is intentionally no state mutation or event emission -- integrators
    /// should model real sensitive operations on this pattern and add their own
    /// logic after the gate.
    pub fn execute_read_task(env: Env, caller: Address) {
        caller.require_auth();
        Self::require_role(&env, &caller, Role::ReadOnly);
    }

    pub fn transfer_super_admin(env: Env, caller: Address, new_admin: Address) {
        caller.require_auth();
        Self::require_role(&env, &caller, Role::SuperAdmin);

        if caller == new_admin {
            panic!("cannot transfer to self");
        }

        // Issue #244: transferring to an account that is already a SuperAdmin
        // would remove the caller without adding a new SuperAdmin, silently
        // desynchronizing SuperAdminCount. Reject it explicitly.
        let new_admin_role: Option<Role> = env
            .storage()
            .persistent()
            .get(&DataKey::Role(new_admin.clone()));
        if new_admin_role == Some(Role::SuperAdmin) {
            panic!("already super admin");
        }

        // `require_role` above already guarantees the caller has an assigned
        // role record, so no separate existence check is needed here.
        let caller_key = DataKey::Role(caller.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Role(new_admin.clone()), &Role::SuperAdmin);
        env.storage().persistent().remove(&caller_key);

        // Issue #244: the caller loses SuperAdmin and new_admin gains it, so the
        // net SuperAdmin count is unchanged. Keep the stored count consistent
        // with the actual number of SuperAdmin holders.
        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::SuperAdminCount)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::SuperAdminCount, &count);

        env.events().publish(
            ("RBAC", "super_admin_transferred"),
            RoleGrantedEvent {
                account: new_admin,
                role: Role::SuperAdmin,
                granted_by: caller,
            },
        );
    }

    fn require_role(env: &Env, account: &Address, required: Role) {
        let role: Role = env
            .storage()
            .persistent()
            .get(&DataKey::Role(account.clone()))
            .expect("role not assigned");

        let ok = match required {
            Role::SuperAdmin => role == Role::SuperAdmin,
            Role::OperationsAdmin => {
                role == Role::OperationsAdmin || role == Role::SuperAdmin
            }
            Role::ComplianceAdmin => {
                role == Role::ComplianceAdmin || role == Role::SuperAdmin
            }
            Role::ReadOnly => true,
        };

        if !ok {
            panic!("insufficient role");
        }
    }
}
