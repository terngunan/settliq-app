//! Reconciliation contract: Merkle-root based settlement verification.
//!
//! The admin submits a Merkle root per reconciliation cycle. Each submission is
//! retained as a historical batch so proofs remain verifiable after newer
//! cycles land. Batches can be archived (pruned) once they are no longer needed.

#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Vec};

/// A single sibling node on the path from a leaf to the Merkle root.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerkleProofNode {
    pub sibling: BytesN<32>,
    /// `true` when `sibling` sits on the left of the current hash.
    pub is_left: bool,
}

/// A stored reconciliation batch.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchRecord {
    pub merkle_root: BytesN<32>,
    pub submitted_ledger: u32,
    pub submitted_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationSubmittedEvent {
    pub batch_id: u32,
    pub merkle_root: BytesN<32>,
    pub submitted_ledger: u32,
    pub submitted_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminTransferredEvent {
    pub previous_admin: Address,
    pub new_admin: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchesArchivedEvent {
    pub batches_pruned: u32,
    pub upper_batch_id: u32,
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotAdmin = 1,
    BatchNotFound = 2,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    BatchCount,
    CurrentBatch,
    Batch(u32),
}

#[contract]
pub struct ReconciliationContract;

#[contractimpl]
impl ReconciliationContract {
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::BatchCount, &0u32);
    }

    /// Transfers reconciliation authority to `new_admin`.
    ///
    /// # Panics
    /// If `new_admin` equals the current admin (self-transfer) or the contract's own address
    /// (would brick the contract since it cannot sign transactions).
    pub fn transfer_admin(env: Env, caller: Address, new_admin: Address) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        // Prevent self-transfer
        if caller == new_admin {
            panic!("Cannot transfer admin to self");
        }

        // Prevent transferring to the contract itself (would be unrecoverable)
        let contract_address = env.current_contract_address();
        if new_admin == contract_address {
            panic!("Cannot transfer admin to contract address");
        }

        env.storage().instance().set(&DataKey::Admin, &new_admin);

        env.events().publish(
            ("RECONCILIATION", "admin_transferred"),
            AdminTransferredEvent {
                previous_admin: current_admin,
                new_admin,
            },
        );
    }

    pub fn submit_merkle_root(env: Env, admin: Address, merkle_root: BytesN<32>) -> u32 {
        admin.require_auth();
        let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if stored != admin {
            panic!("Not admin");
        }

        let batch_id: u32 = env.storage().instance().get(&DataKey::BatchCount).unwrap_or(0);
        let record = BatchRecord {
            merkle_root: merkle_root.clone(),
            submitted_ledger: env.ledger().sequence(),
            submitted_at: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&DataKey::Batch(batch_id), &record);
        env.storage().instance().set(&DataKey::CurrentBatch, &batch_id);
        env.storage().instance().set(&DataKey::BatchCount, &(batch_id + 1));

        env.events().publish(
            ("RECONCILIATION", "submitted"),
            ReconciliationSubmittedEvent {
                batch_id,
                merkle_root,
                submitted_ledger: record.submitted_ledger,
                submitted_at: record.submitted_at,
            },
        );

        batch_id
    }

    pub fn get_current_batch(env: Env) -> Option<BatchRecord> {
        let id: u32 = env.storage().instance().get(&DataKey::CurrentBatch)?;
        env.storage().persistent().get(&DataKey::Batch(id))
    }

    pub fn get_batch(env: Env, batch_id: u32) -> Option<BatchRecord> {
        env.storage().persistent().get(&DataKey::Batch(batch_id))
    }

    pub fn batch_count(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::BatchCount).unwrap_or(0)
    }

    pub fn verify_settlement(env: Env, payment_id: BytesN<32>, proof: Vec<MerkleProofNode>) -> bool {
        let current = match Self::get_current_batch(env.clone()) {
            Some(b) => b,
            None => return true,
        };
        !Self::verify_against_root(&env, &current.merkle_root, &payment_id, &proof)
    }

    pub fn verify_settlement_proof(
        env: Env,
        batch_id: u32,
        payment_id: BytesN<32>,
        proof: Vec<MerkleProofNode>,
    ) -> bool {
        let batch = match Self::get_batch(env.clone(), batch_id) {
            Some(b) => b,
            None => return false,
        };
        Self::verify_against_root(&env, &batch.merkle_root, &payment_id, &proof)
    }

    /// Permanently archives batches with IDs below `before_batch_id`, bounded
    /// by `max_batches` so retention can be applied incrementally. Current and
    /// newer batches remain available for proof verification. The latest batch
    /// is never archived, even if before_batch_id >= batch_count.
    pub fn archive_batches(env: Env, caller: Address, before_batch_id: u32, max_batches: u32) -> u32 {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        // Ensure we never delete batches at or after the latest batch. The latest
        // batch ID is batch_count - 1, so we clamp before_batch_id to latest_batch_id
        // to avoid deleting it or newer batches.
        let latest_batch_id = Self::latest_batch_id(&env).unwrap_or(0);
        let safe_upper = before_batch_id.min(latest_batch_id);
        let mut archived = 0;
        for batch_id in 0..safe_upper {
            if archived >= max_batches { break; }
            let key = DataKey::Batch(batch_id);
            if env.storage().persistent().has(&key) {
                env.storage().persistent().remove(&key);
                archived += 1;
            }
            id += 1;
        }

        env.events().publish(
            ("RECONCILIATION", "batches_archived"),
            BatchesArchivedEvent {
                batches_pruned: pruned,
                upper_batch_id,
            },
        );
    }

    fn verify_against_root(
        env: &Env,
        root: &BytesN<32>,
        payment_id: &BytesN<32>,
        proof: &Vec<MerkleProofNode>,
    ) -> bool {
        let mut computed = Self::hash_leaf(env, payment_id);
        for node in proof.iter() {
            computed = if node.is_left {
                Self::hash_pair(env, &node.sibling, &computed)
            } else {
                Self::hash_pair(env, &computed, &node.sibling)
            };
        }
        &computed == root
    }

    pub fn hash_leaf(env: &Env, payment_id: &BytesN<32>) -> BytesN<32> {
        let mut buf = [0u8; 32];
        payment_id.copy_into_slice(&mut buf);
        env.crypto().sha256(&soroban_sdk::Bytes::from_array(env, &buf)).to_bytes()
    }

    pub fn hash_pair(env: &Env, left: &BytesN<32>, right: &BytesN<32>) -> BytesN<32> {
        let mut buf = [0u8; 64];
        left.copy_into_slice(&mut buf[..32]);
        right.copy_into_slice(&mut buf[32..]);
        env.crypto().sha256(&soroban_sdk::Bytes::from_array(env, &buf)).to_bytes()
    }
}
