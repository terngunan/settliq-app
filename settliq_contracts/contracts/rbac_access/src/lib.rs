#![no_std]

mod test;

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env,
};

// ── TTL management (Issue #245) ─────────────────────────────────────────────
//
// Persistent storage entries expire and are archived if their TTL is not
// extended. Role assignments and the SuperAdminCount counter must be kept
// alive, otherwise administrator access controls would silently expire.
//
// `PERSISTENT_TTL_THRESHOLD`: if the remaining TTL is below this value, the
// entry is extended. `PERSISTENT_TTL_EXTEND_TO`: the TTL the entry is bumped
// to when extended.
const PERSISTENT_TTL_THRESHOLD: u32 = 100_000;
const PERSISTENT_TTL_EXTEND_TO: u32 = 500_000;

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

        // Issue #245: extend TTL for the entries created during init.
        Self::extend_role_ttl(&env, &super_admin);
        Self::extend_super_admin_count_ttl(&env);
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

        // Issue #245: extend TTL for the role entry we just wrote.
        Self::extend_role_ttl(&env, &account);

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
            // Issue #245: extend TTL for the updated counter.
            Self::extend_super_admin_count_ttl(&env);
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
            // Issue #245: extend TTL for the updated counter.
            Self::extend_super_admin_count_ttl(&env);
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
            // Issue #245: extend TTL for the updated counter.
            Self::extend_super_admin_count_ttl(&env);
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
        let key = DataKey::Role(account.clone());
        let role: Option<Role> = env.storage().persistent().get(&key);
        // Issue #245: extend TTL when the role entry is accessed.
        if role.is_some() {
            Self::extend_role_ttl(&env, &account);
        }
        role
    }

    /// Issue #246: expose the current number of super administrators.
    ///
    /// Returns the value stored under `DataKey::SuperAdminCount`, or `0` when
    /// the key is unset (e.g. before any super admin has been granted).
    pub fn get_super_admin_count(env: Env) -> u32 {
        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::SuperAdminCount)
            .unwrap_or(0);
        // Issue #245: extend TTL when the counter is accessed.
        if count > 0 {
            Self::extend_super_admin_count_ttl(&env);
        }
        count
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

    pub fn has_role(env: Env, account: Address, role: Role) -> bool {
        Self::get_role(env, account) == Some(role)
    }

    pub fn require_role(env: &Env, account: &Address, required: Role) {
        let role: Option<Role> = env
            .storage()
            .persistent()
            .get(&DataKey::Role(account.clone()));

        let role = role.unwrap_or_else(|| panic!("account has no role"));

        if role == Role::SuperAdmin {
            return;
        }

        if role != required {
            panic!("insufficient role");
        }
    }

    pub fn transfer_super_admin(env: Env, caller: Address, new_admin: Address) {
        caller.require_auth();
        Self::require_role(&env, &caller, Role::SuperAdmin);

        if caller == new_admin {
            panic!("cannot transfer to self");
        }

        env.storage()
            .persistent()
            .set(&DataKey::Role(new_admin.clone()), &Role::SuperAdmin);
        env.storage()
            .persistent()
            .remove(&DataKey::Role(caller.clone()));

        // Issue #245: extend TTL for the role entries touched by the transfer.
        Self::extend_role_ttl(&env, &new_admin);
        Self::extend_role_ttl(&env, &caller);

        env.events().publish(
            ("RBAC", "role_granted"),
            RoleGrantedEvent {
                account: new_admin,
                role: Role::SuperAdmin,
                granted_by: caller.clone(),
            },
        );
        env.events().publish(
            ("RBAC", "role_revoked"),
            RoleRevokedEvent {
                account: caller,
                revoked_by: caller,
            },
        );
    }

    // ── TTL helpers (Issue #245) ────────────────────────────────────────────

    fn extend_role_ttl(env: &Env, account: &Address) {
        let key = DataKey::Role(account.clone());
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }

    fn extend_super_admin_count_ttl(env: &Env) {
        env.storage().persistent().extend_ttl(
            &DataKey::SuperAdminCount,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_EXTEND_TO,
        );
    }
}
