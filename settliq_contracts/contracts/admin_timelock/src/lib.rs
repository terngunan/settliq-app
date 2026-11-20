//! # admin_timelock
//!
//! A delay-gated change-scheduling contract. A single `Admin` address may
//! schedule, apply, and cancel parameter changes, subject to a minimum
//! timelock delay.
//!
//! ## Composing with multisig_admin
//!
//! Deploying this contract with its `admin` set to an EOA (externally-owned
//! account / single key) rather than to a `multisig_admin` contract address
//! means that one compromised private key is sufficient to schedule **and**
//! subsequently apply any change once the delay elapses — defeating the
//! purpose of multi-party review.
//!
//! **Recommended deployment**: set the `admin` constructor argument to the
//! deployed address of a `multisig_admin` instance (2-of-3). Every call to
//! `schedule_change`, `apply_change`, and `cancel_change` then requires a
//! `multisig_admin` proposal to reach the 2-of-3 approval threshold before
//! `admin_timelock` will accept it, providing the independent second check
//! the timelock is designed to enforce.
//!
//! See `test.rs` for an integration test that demonstrates this composition.

#![no_std]

mod test;

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Bytes, BytesN, Env, String, Vec,
};

const MIN_DELAY_LEDGERS: u32 = 17_280;
const MAX_DELAY_LEDGERS: u32 = 52_560_000; // ~10 years at 5 s/ledger
// ~14 days at 5 s/ledger (14 * 24 * 60 * 60 / 5 = 241_920).
// A pending change that is not applied within this window after its
// execute_after ledger is considered stale and must be re-scheduled.
const GRACE_PERIOD_LEDGERS: u32 = 241_920;
const MAX_PARAM_LEN: usize = 256;
const MAX_VALUE_LEN: usize = 4096;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ChangeStatus {
    Pending,
    Applied,
    Cancelled,
}

/// A queued admin parameter change.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ScheduledChange {
    pub change_id: BytesN<32>,
    pub param: String,
    pub value: String,
    pub scheduled_at: u32,
    pub execute_after: u32,
    pub status: ChangeStatus,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Counter,
    Change(BytesN<32>),
    PendingList,
    /// Pending admin address proposed via `propose_rotate_admin`, together with
    /// the ledger sequence number after which `accept_rotate_admin` may be
    /// called.  Cleared once the rotation is accepted (#9).
    PendingAdmin,
    /// The earliest ledger at which the pending admin rotation may be accepted.
    PendingAdminExecuteAfter,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[contracttype]
struct ChangeScheduledEvent {
    change_id: BytesN<32>,
    param: String,
    value: String,
    execute_after: u32,
}

#[contracttype]
struct ChangeAppliedEvent {
    change_id: BytesN<32>,
    param: String,
    value: String,
}

#[contracttype]
struct ChangeCancelledEvent {
    change_id: BytesN<32>,
    param: String,
}

#[contracttype]
struct ChangePrunedEvent {
    change_id: BytesN<32>,
}

#[contracttype]
struct AdminRotatedEvent {
    old_admin: Address,
    new_admin: Address,
}

#[contracttype]
struct AdminRotationProposedEvent {
    current_admin: Address,
    pending_admin: Address,
    execute_after: u32,
}

#[contracttype]
struct AdminRotationCancelledEvent {
    pending_admin: Address,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct AdminTimelockContract;

#[contractimpl]
impl AdminTimelockContract {
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Counter, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::PendingList, &Vec::<BytesN<32>>::new(&env));
    }

    /// Queue a parameter change. Returns the change_id.
    /// delay_ledgers must be at least MIN_DELAY_LEDGERS (~24 hours at 5 s/ledger)
    /// and at most MAX_DELAY_LEDGERS (~10 years at 5 s/ledger).
    pub fn schedule_change(
        env: Env,
        caller: Address,
        param: String,
        value: String,
        delay_ledgers: u32,
    ) -> BytesN<32> {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        // Validate param string (issue #86: prevent empty or unbounded strings)
        if param.len() == 0 {
            panic!("param must not be empty");
        }
        if param.len() > MAX_PARAM_LEN {
            panic!("param exceeds maximum length");
        }

        // Validate value string (issue #86: prevent unbounded strings)
        if value.len() > MAX_VALUE_LEN {
            panic!("value exceeds maximum length");
        }

        // Validate delay range (issue #85: prevent astronomical timelock lockouts)
        if delay_ledgers < MIN_DELAY_LEDGERS {
            panic!("delay_ledgers must be >= 17280 (24 h)");
        }
        if delay_ledgers > MAX_DELAY_LEDGERS {
            panic!("delay_ledgers must be <= 52560000 (~10 years)");
        }

        let counter: u64 = env.storage().instance().get(&DataKey::Counter).unwrap();
        let new_counter = counter + 1;
        env.storage()
            .instance()
            .set(&DataKey::Counter, &new_counter);

        let mut seed = Bytes::new(&env);
        seed.extend_from_array(&new_counter.to_be_bytes());
        let change_id: BytesN<32> = env.crypto().sha256(&seed).into();

        let now = env.ledger().sequence();
        let execute_after = now.saturating_add(delay_ledgers);

        let change = ScheduledChange {
            change_id: change_id.clone(),
            param: param.clone(),
            value: value.clone(),
            scheduled_at: now,
            execute_after,
            status: ChangeStatus::Pending,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Change(change_id.clone()), &change);

        let mut pending: Vec<BytesN<32>> = env
            .storage()
            .instance()
            .get(&DataKey::PendingList)
            .unwrap_or_else(|| Vec::new(&env));
        pending.push_back(change_id.clone());
        env.storage()
            .instance()
            .set(&DataKey::PendingList, &pending);

        env.events().publish(
            ("TIMELOCK", "change_scheduled"),
            ChangeScheduledEvent {
                change_id: change_id.clone(),
                param,
                value,
                execute_after,
            },
        );

        change_id
    }

    /// Execute a queued change after the delay has elapsed.
    ///
    /// # Important — attestation only
    ///
    /// This contract is a **pure attestation / signaling contract**. Calling
    /// `apply_change` does **not** make any on-chain state change to another
    /// contract. It flips the change status to `Applied` and emits a
    /// `change_applied` event. Applying the actual change to a target contract
    /// (e.g. updating `fee_calculator`'s fee tiers) is the sole responsibility
    /// of an **external relayer** that listens for `change_applied` events and
    /// then invokes the target contract directly.
    pub fn apply_change(env: Env, caller: Address, change_id: BytesN<32>) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let mut change = Self::get_change(env.clone(), change_id.clone());

        if change.status != ChangeStatus::Pending {
            panic!("Change is not pending");
        }
        if env.ledger().sequence() < change.execute_after {
            panic!("Delay period has not elapsed");
        }
        if env.ledger().sequence() > change.execute_after.saturating_add(GRACE_PERIOD_LEDGERS) {
            panic!("Change execution window has expired");
        }

        change.status = ChangeStatus::Applied;
        env.storage()
            .persistent()
            .set(&DataKey::Change(change_id.clone()), &change);

        Self::remove_from_pending(&env, &change_id);

        env.events().publish(
            ("TIMELOCK", "change_applied"),
            ChangeAppliedEvent {
                change_id,
                param: change.param,
                value: change.value,
            },
        );
    }

    /// Cancel a queued change. Only the admin may cancel.
    pub fn cancel_change(env: Env, caller: Address, change_id: BytesN<32>) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let mut change = Self::get_change(env.clone(), change_id.clone());

        if change.status != ChangeStatus::Pending {
            panic!("Change is not pending");
        }

        change.status = ChangeStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Change(change_id.clone()), &change);

        Self::remove_from_pending(&env, &change_id);

        env.events().publish(
            ("TIMELOCK", "change_cancelled"),
            ChangeCancelledEvent {
                change_id,
                param: change.param,
            },
        );
    }

    /// Prune a stale pending change whose execution window has expired.
    pub fn prune_change(env: Env, caller: Address, change_id: BytesN<32>) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let mut change = Self::get_change(env.clone(), change_id.clone());

        if change.status != ChangeStatus::Pending {
            panic!("Change is not pending");
        }
        if env.ledger().sequence()
            <= change.execute_after.saturating_add(GRACE_PERIOD_LEDGERS)
        {
            panic!("Change execution window has not expired");
        }

        change.status = ChangeStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Change(change_id.clone()), &change);

        Self::remove_from_pending(&env, &change_id);

        env.events().publish(
            ("TIMELOCK", "change_pruned"),
            ChangePrunedEvent { change_id },
        );
    }

    /// Return the list of change IDs that are still pending.
    pub fn get_pending_changes(env: Env) -> Vec<BytesN<32>> {
        env.storage()
            .instance()
            .get(&DataKey::PendingList)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Return a scheduled change by id.
    pub fn get_change(env: Env, change_id: BytesN<32>) -> ScheduledChange {
        env.storage()
            .persistent()
            .get(&DataKey::Change(change_id))
            .expect("Change not found")
    }

    /// Return the current admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set")
    }

    /// Propose a new admin. The rotation must be accepted after the timelock
    /// delay elapses.
    pub fn propose_rotate_admin(
        env: Env,
        caller: Address,
        new_admin: Address,
        delay_ledgers: u32,
    ) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        if delay_ledgers < MIN_DELAY_LEDGERS {
            panic!("delay_ledgers must be >= 17280 (24 h)");
        }
        if delay_ledgers > MAX_DELAY_LEDGERS {
            panic!("delay_ledgers must be <= 52560000 (~10 years)");
        }

        let execute_after = env.ledger().sequence().saturating_add(delay_ledgers);

        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        env.storage()
            .instance()
            .set(&DataKey::PendingAdminExecuteAfter, &execute_after);

        env.events().publish(
            ("TIMELOCK", "admin_rotation_proposed"),
            AdminRotationProposedEvent {
                current_admin: caller,
                pending_admin: new_admin,
                execute_after,
            },
        );
    }

    /// Accept a previously proposed admin rotation once the delay has elapsed.
    pub fn accept_rotate_admin(env: Env, caller: Address) {
        caller.require_auth();

        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .expect("No pending admin rotation");
        let execute_after: u32 = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdminExecuteAfter)
            .expect("No pending admin rotation");

        if caller != pending_admin {
            panic!("Caller is not the pending admin");
        }
        if env.ledger().sequence() < execute_after {
            panic!("Delay period has not elapsed");
        }

        let old_admin = Self::get_admin(env.clone());

        env.storage().instance().set(&DataKey::Admin, &pending_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.storage()
            .instance()
            .remove(&DataKey::PendingAdminExecuteAfter);

        env.events().publish(
            ("TIMELOCK", "admin_rotated"),
            AdminRotatedEvent {
                old_admin,
                new_admin: pending_admin,
            },
        );
    }

    /// Cancel a pending admin rotation.
    pub fn cancel_rotate_admin(env: Env, caller: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .expect("No pending admin rotation");

        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.storage()
            .instance()
            .remove(&DataKey::PendingAdminExecuteAfter);

        env.events().publish(
            ("TIMELOCK", "admin_rotation_cancelled"),
            AdminRotationCancelledEvent { pending_admin },
        );
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        if caller != &admin {
            panic!("Caller is not admin");
        }
    }

    fn remove_from_pending(env: &Env, change_id: &BytesN<32>) {
        let pending: Vec<BytesN<32>> = env
            .storage()
            .instance()
            .get(&DataKey::PendingList)
            .unwrap_or_else(|| Vec::new(env));
        let mut updated = Vec::new(env);
        for id in pending.iter() {
            if &id != change_id {
                updated.push_back(id);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::PendingList, &updated);
    }
}
