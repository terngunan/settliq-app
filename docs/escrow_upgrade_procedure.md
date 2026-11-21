# Upgrade the payment escrow contract

Use the existing escrow contract ID and a storage-compatible replacement WASM. The upgrade preserves the instance and its stored data. Check any required migration before scheduling.

## Build and upload

From the repository root, set `NETWORK` to the configured Stellar network, `ADMIN` to the signing identity for the escrow administrator, and `ESCROW_ID` to the deployed contract ID. The example uses an account administrator. For a multisig administrator, authorize the calls through its approval process.

```bash
cd settliq_contracts
cargo build --release --target wasm32v1-none -p payment-escrow
ADMIN_ADDRESS=$(stellar keys address "$ADMIN")
WASM_HASH=$(stellar contract upload --network "$NETWORK" --source-account "$ADMIN" \
  --wasm target/wasm32v1-none/release/payment_escrow.wasm)

escrow() {
  stellar contract invoke --id "$ESCROW_ID" --network "$NETWORK" \
    --source-account "$ADMIN" -- "$@"
}
```

## Schedule and inspect

Read the current proposal first. Scheduling again replaces the pending hash and restarts the cooldown.

```bash
escrow get_pending_upgrade
escrow schedule_upgrade --caller "$ADMIN_ADDRESS" --new_wasm_hash "$WASM_HASH"
escrow get_pending_upgrade
```

Verify that the returned pair contains the uploaded hash. Its second value is the first ledger sequence at which the admin can apply it. The default delay is 17,280 ledgers (about 24 hours at five seconds per ledger). Use the returned ledger as the deadline for the cooldown.

## Apply after the cooldown

Once the network reaches the apply ledger, check the pending hash and ledger again before submitting the transaction.

```bash
escrow get_pending_upgrade
escrow apply_upgrade --caller "$ADMIN_ADDRESS"
escrow get_pending_upgrade
```

Confirm that the apply transaction succeeded and that the pending upgrade is now empty (`null`).

## Emergency cancellation

If the hash is unexpected or the release must be withdrawn, the admin can cancel any time before application, including after the cooldown expires.

```bash
escrow cancel_upgrade --caller "$ADMIN_ADDRESS"
escrow get_pending_upgrade
```

Confirm the pending upgrade is empty. To retry, schedule the intended hash and wait for its new apply ledger.
