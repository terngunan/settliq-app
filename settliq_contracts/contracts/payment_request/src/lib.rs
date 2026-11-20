#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, String,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaymentStatus {
    Pending = 0,
    Confirmed = 1,
    Settling = 2,
    Settled = 3,
    Failed = 4,
    Expired = 5,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Payment {
    pub id: String,
    pub merchant: Address,
    pub amount: i128,
    pub asset: Address,
    pub status: PaymentStatus,
    pub created_at: u64,
    pub expiry: u64,
}

/// TTL policy for payment records (in ledgers, ~5s each).
/// Active payments are bumped on every write so they never lapse mid-flow.
/// Terminal payments (Settled/Failed/Expired) get a shorter TTL and are left to
/// expire naturally, acting as the archival strategy: off-chain indexers should
/// persist the emitted events as the long-term record.
const ACTIVE_TTL_THRESHOLD: u32 = 17_280; // ~1 day
const ACTIVE_TTL_EXTEND: u32 = 518_400; // ~30 days
const TERMINAL_TTL_THRESHOLD: u32 = 17_280; // ~1 day
const TERMINAL_TTL_EXTEND: u32 = 120_960; // ~7 days
const MAX_ID_LEN: u32 = 64;

#[contracttype]
pub enum DataKey {
    Admin,
    Payment(String),
}

/// Emitted when admin authority is rotated to a new address.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminTransferredEvent {
    pub old_admin: Address,
    pub new_admin: Address,
    pub timestamp: u64,
}

/// Rich event payload emitted on every payment lifecycle transition.
///
/// Carries the full payment identity (merchant, amount, asset, expiry) plus the
/// resulting status so off-chain indexers can reconstruct historical payments
/// from event logs alone, without relying on persistent storage retention.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentEvent {
    pub id: String,
    pub merchant: Address,
    pub amount: i128,
    pub asset: Address,
    pub status: PaymentStatus,
    pub expiry: u64,
    pub timestamp: u64,
}

#[contract]
pub struct PaymentRequestContract;

fn is_terminal(status: &PaymentStatus) -> bool {
    matches!(
        status,
        PaymentStatus::Settled | PaymentStatus::Failed | PaymentStatus::Expired
    )
}

/// Persist a payment and set its TTL according to its lifecycle stage.
fn save_payment(env: &Env, key: &DataKey, payment: &Payment) {
    let storage = env.storage().persistent();
    storage.set(key, payment);
    if is_terminal(&payment.status) {
        storage.extend_ttl(key, TERMINAL_TTL_THRESHOLD, TERMINAL_TTL_EXTEND);
    } else {
        storage.extend_ttl(key, ACTIVE_TTL_THRESHOLD, ACTIVE_TTL_EXTEND);
    }
}

/// Load a payment without any authorization check.
fn load_payment(env: &Env, key: &DataKey) -> Payment {
    env.storage().persistent().get(key).expect("Payment not found")
}

/// Require that the caller is the stored admin.
fn require_admin(env: &Env, caller: &Address) {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("Contract not initialized");
    if caller != &admin {
        panic!("Not admin");
    }
}

/// Build the rich event payload for a payment's current state.
fn payment_event(env: &Env, payment: &Payment) -> PaymentEvent {
    PaymentEvent {
        id: payment.id.clone(),
        merchant: payment.merchant.clone(),
        amount: payment.amount,
        asset: payment.asset.clone(),
        status: payment.status.clone(),
        expiry: payment.expiry,
        timestamp: env.ledger().timestamp(),
    }
}

#[contractimpl]
impl PaymentRequestContract {
    /// Atomic constructor: runs once during contract deployment.
    ///
    /// Sets the admin address that is authorized to drive settlement state
    /// transitions (`confirm`, `set_settling`, `settle`, `fail`) without
    /// requiring the merchant's signature each time, enabling backend
    /// settlement daemons to operate autonomously. Because this executes as
    /// part of the deployment transaction, there is no window for a
    /// front-runner to initialize the contract with their own admin.
    pub fn __constructor(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Rotate admin authority to a new address.
    ///
    /// Only the current admin may call this. Enables rotating the admin key to
    /// a multisig or timelock contract if the original key is compromised or
    /// needs to be upgraded.
    pub fn transfer_admin(env: Env, caller: Address, new_admin: Address) {
        caller.require_auth();
        require_admin(&env, &caller);

        let old_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Contract not initialized");

        env.storage().instance().set(&DataKey::Admin, &new_admin);

        env.events().publish(
            (symbol_short!("admin"), symbol_short!("transferred")),
            AdminTransferredEvent {
                old_admin,
                new_admin,
                timestamp: env.ledger().timestamp(),
            },
        );
    }

    /// Initialize a new payment request.
    pub fn create_payment(
        env: Env,
        id: String,
        merchant: Address,
        amount: i128,
        asset: Address,
        expiry: u64,
    ) {
        merchant.require_auth();
        if id.len() == 0 {
            panic!("payment id must not be empty");
        }
        if id.len() > MAX_ID_LEN {
            panic!("payment id too long");
        }
        if amount <= 0 {
            panic!("amount must be > 0");
        }
        // Validate asset token address is not the contract itself (#82)
        let contract_addr = env.current_contract_address();
        if asset == contract_addr {
            panic!("asset cannot be the contract address");
        }
        let key = DataKey::Payment(id.clone());
        if env.storage().persistent().has(&key) {
            panic!("Payment already exists");
        }

        let payment = Payment {
            id: id.clone(),
            merchant,
            amount,
            asset,
            status: PaymentStatus::Pending,
            created_at: env.ledger().timestamp(),
            expiry,
        };

        save_payment(&env, &key, &payment);

        env.events().publish(
            (symbol_short!("payment"), id, symbol_short!("created")),
            payment_event(&env, &payment),
        );
    }

    /// Mark payment as confirmed (user has paid).
    ///
    /// Requires admin authorization — the platform payment monitor drives
    /// confirmations so the merchant's online presence is not required.
    pub fn confirm(env: Env, caller: Address, id: String) {
        caller.require_auth();
        require_admin(&env, &caller);

        let key = DataKey::Payment(id.clone());
        let mut payment = load_payment(&env, &key);

        if payment.status != PaymentStatus::Pending {
            panic!("Invalid transition: can only confirm pending payments");
        }

        if env.ledger().timestamp() > payment.expiry {
            panic!("Payment expired");
        }

        payment.status = PaymentStatus::Confirmed;
        save_payment(&env, &key, &payment);

        env.events().publish(
            (symbol_short!("payment"), id, symbol_short!("confirmed")),
            payment_event(&env, &payment),
        );
    }

    /// Move payment to settling state (initiate payout to merchant).
    ///
    /// Requires admin authorization — the backend settlement daemon drives
    /// this transition autonomously.
    pub fn set_settling(env: Env, caller: Address, id: String) {
        caller.require_auth();
        require_admin(&env, &caller);

        let key = DataKey::Payment(id.clone());
        let mut payment = load_payment(&env, &key);

        if payment.status != PaymentStatus::Confirmed {
            panic!("Invalid transition: can only set settling from confirmed");
        }

        payment.status = PaymentStatus::Settling;
        save_payment(&env, &key, &payment);

        env.events().publish(
            (symbol_short!("payment"), id, symbol_short!("settling")),
            payment_event(&env, &payment),
        );
    }

    /// Mark payment as settled (funds received by merchant).
    ///
    /// Requires admin authorization — the backend settlement relayer confirms
    /// final on-chain settlement.
    pub fn settle(env: Env, caller: Address, id: String) {
        caller.require_auth();
        require_admin(&env, &caller);

        let key = DataKey::Payment(id.clone());
        let mut payment = load_payment(&env, &key);

        // Settling is a mandatory intermediate step: `set_settling` signals that a
        // payout is in flight, so settle may only follow it.
        if payment.status != PaymentStatus::Settling {
            panic!("Invalid transition: can only settle from Settling");
        }

        payment.status = PaymentStatus::Settled;
        save_payment(&env, &key, &payment);

        env.events().publish(
            (symbol_short!("payment"), id, symbol_short!("settled")),
            payment_event(&env, &payment),
        );
    }

    /// Mark payment as failed.
    ///
    /// Requires admin authorization — the backend settlement daemon reports
    /// failed payouts.
    pub fn fail(env: Env, caller: Address, id: String) {
        caller.require_auth();
        require_admin(&env, &caller);

        let key = DataKey::Payment(id.clone());
        let mut payment = load_payment(&env, &key);

        if is_terminal(&payment.status) {
            panic!("Invalid transition: payment already terminal");
        }

        payment.status = PaymentStatus::Failed;
        save_payment(&env, &key, &payment);

        env.events().publish(
            (symbol_short!("payment"), id, symbol_short!("failed")),
            payment_event(&env, &payment),
        );
    }

    /// Mark payment as expired.
    ///
    /// Requires admin authorization — the backend expiry sweeper drives this
    /// transition once the payment's expiry timestamp has passed.
    pub fn expire(env: Env, caller: Address, id: String) {
        caller.require_auth();
        require_admin(&env, &caller);

        let key = DataKey::Payment(id.clone());
        let mut payment = load_payment(&env, &key);

        if is_terminal(&payment.status) {
            panic!("Invalid transition: payment already terminal");
        }

        if env.ledger().timestamp() <= payment.expiry {
            panic!("Payment not yet expired");
        }

        payment.status = PaymentStatus::Expired;
        save_payment(&env, &key, &payment);

        env.events().publish(
            (symbol_short!("payment"), id, symbol_short!("expired")),
            payment_event(&env, &payment),
        );
    }

    /// Read a payment record.
    pub fn get_payment(env: Env, id: String) -> Payment {
        load_payment(&env, &DataKey::Payment(id))
    }
}
