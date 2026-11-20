#![no_std]

mod test;

use soroban_sdk::{contract, contractimpl, contracttype, vec, Address, Bytes, Env, String, Vec};

/// Minimum number of approvals required before a proposal auto-executes.
///
/// SECURITY INVARIANT: `THRESHOLD` must NEVER be less than 2. Both `propose`
/// and `approve` call `maybe_execute`, which auto-executes as soon as
/// `proposal.approvals.len() >= THRESHOLD`. A fresh proposal already carries a
/// single approval (the proposer), so a threshold of 1 would let a lone proposer
/// immediately self-execute every proposal, silently defeating the entire
/// multisig premise. If this constant is ever made configurable, the setter
/// MUST enforce `assert!(new_threshold >= 2)`.
const THRESHOLD: u32 = 2;
const EXPIRY_SECONDS: u64 = 24 * 60 * 60; // 24 hours

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub operation: String,
    pub args: Bytes,
    pub approvals: Vec<Address>,
    pub created_at: u64,
    pub expires_at: u64,
    pub executed: bool,
    pub rejected: bool,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admins,
    Threshold,
    NextProposalId,
    Proposal(u64),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalCreatedEvent {
    pub proposal_id: u64,
    pub proposer: Address,
    pub operation: String,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalApprovedEvent {
    pub proposal_id: u64,
    pub approver: Address,
    pub approvals: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalRejectedEvent {
    pub proposal_id: u64,
    pub rejecter: Address,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalExecutedEvent {
    pub proposal_id: u64,
    pub operation: String,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalPrunedEvent {
    pub proposal_id: u64,
}

/// Multisig admin **attestation / signaling** contract.
///
/// IMPORTANT: This contract does NOT execute proposals against any real
/// contract state. The `operation` and `args` fields carried by a [`Proposal`]
/// are opaque, caller-supplied metadata: they are stored and echoed back in
/// events, but they are NEVER interpreted, decoded, or dispatched as a
/// cross-contract call anywhere in this contract. Marking a proposal as
/// `executed` (see [`MultisigAdminContract::maybe_execute`]) only flips a
/// signaling flag and emits a `proposal_executed` event.
///
/// Applying the real effect described by `operation`/`args` is the
/// responsibility of an external, off-chain relayer that watches for the
/// `proposal_executed` event and performs the corresponding action itself.
/// Integrators MUST NOT assume that an `executed` proposal has already changed
/// any on-chain state.
#[contract]
pub struct MultisigAdminContract;

#[contractimpl]
impl MultisigAdminContract {
    pub fn __constructor(env: Env, admin1: Address, admin2: Address, admin3: Address) {
        if admin1 == admin2 || admin1 == admin3 || admin2 == admin3 {
            panic!("admins must be unique");
        }

        let admins = vec![&env, admin1, admin2, admin3];
        env.storage().instance().set(&DataKey::Admins, &admins);
        env.storage().instance().set(&DataKey::Threshold, &THRESHOLD);
        env.storage().instance().set(&DataKey::NextProposalId, &0u64);
    }

    pub fn propose(env: Env, caller: Address, operation: String, args: Bytes) -> u64 {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        if operation.len() == 0 {
            panic!("operation must not be empty");
        }

        let mut next_id: u64 = env.storage().instance().get(&DataKey::NextProposalId).unwrap_or(0);
        let proposal_id = next_id;
        // Use checked arithmetic so that exhausting the u64 id space fails
        // loudly instead of silently reusing an id (which would overwrite an
        // existing stored Proposal at the same key).
        next_id = next_id.checked_add(1).expect("proposal id overflow");
        env.storage().instance().set(&DataKey::NextProposalId, &next_id);

        let now = env.ledger().timestamp();
        let mut approvals = vec![&env];
        approvals.push_back(caller.clone());

        let mut proposal = Proposal {
            id: proposal_id,
            proposer: caller.clone(),
            operation: operation.clone(),
            args,
            approvals,
            created_at: now,
            expires_at: now.saturating_add(EXPIRY_SECONDS),
            executed: false,
            rejected: false,
        };

        env.events().publish(
            ("MULTISIG", "proposal_created"),
            ProposalCreatedEvent {
                proposal_id,
                proposer: caller,
                operation: operation.clone(),
                expires_at: proposal.expires_at,
            },
        );

        // A fresh proposal holds only the proposer's approval, so with the
        // required THRESHOLD >= 2 it cannot auto-execute here. See the
        // THRESHOLD invariant above.
        Self::maybe_execute(&env, &mut proposal);
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);
        proposal_id
    }

    pub fn approve(env: Env, caller: Address, proposal_id: u64) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::Proposal(proposal_id);
        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&key)
            .expect("proposal not found");

        if env.ledger().timestamp() > proposal.expires_at {
            panic!("proposal expired");
        }
        if proposal.executed {
            panic!("proposal already executed");
        }
        if proposal.rejected {
            panic!("proposal already rejected");
        }
        if caller == proposal.proposer {
            panic!("proposer cannot approve twice");
        }
        if Self::has_approved(&proposal.approvals, &caller) {
            panic!("already approved");
        }

        proposal.approvals.push_back(caller.clone());
        env.events().publish(
            ("MULTISIG", "proposal_approved"),
            ProposalApprovedEvent {
                proposal_id,
                approver: caller,
                approvals: proposal.approvals.len(),
            },
        );

        // Auto-executes only once approvals reach THRESHOLD (>= 2), which
        // requires at least one distinct approver beyond the proposer.
        Self::maybe_execute(&env, &mut proposal);
        env.storage().persistent().set(&key, &proposal);
    }

    /// Reject a proposal to prevent it from being executed. Admin only. (#91)
    /// Once rejected, a proposal cannot be approved or executed.
    pub fn reject_proposal(env: Env, caller: Address, proposal_id: u64) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::Proposal(proposal_id);
        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&key)
            .expect("proposal not found");

        if proposal.executed {
            panic!("proposal already executed");
        }
        if proposal.rejected {
            panic!("proposal already rejected");
        }

        proposal.rejected = true;
        env.events().publish(
            ("MULTISIG", "proposal_rejected"),
            ProposalRejectedEvent { proposal_id },
        );
        env.storage().persistent().set(&key, &proposal);
    }

    /// Removes a rejected proposal from persistent storage immediately,
    /// without waiting for its `expires_at` window to elapse. (#228)
    ///
    /// A rejected proposal can never be executed, so it is safe to reclaim its
    /// storage rent right away. Permissionless: rejection is objectively
    /// checkable on-chain via `proposal.rejected`. Panics if the proposal does
    /// not exist or has not been rejected.
    pub fn prune_rejected_proposal(env: Env, proposal_id: u64) {
        let key = DataKey::Proposal(proposal_id);
        let proposal: Proposal = env
            .storage()
            .persistent()
            .get(&key)
            .expect("proposal not found");

        if !proposal.rejected {
            panic!("proposal not rejected");
        }

        env.storage().persistent().remove(&key);
        env.events().publish(
            ("MULTISIG", "proposal_pruned"),
            ProposalPrunedEvent { proposal_id },
        );
    }

    /// Removes an expired, never-executed proposal from persistent storage.
    ///
    /// Permissionless: expiry is objectively checkable on-chain via
    /// `env.ledger().timestamp() > proposal.expires_at`, matching the check
    /// used by `approve`/`maybe_execute`. This lets anyone reclaim the rent
    /// and 

/* … truncated 5079 chars — edit only what you need near the top … */
