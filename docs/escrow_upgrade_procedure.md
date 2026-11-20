# Escrow Contract Upgrade Procedure

This document describes how to upgrade the `payment_escrow` contract using its
two-step, timelocked upgrade mechanism. The contract does **not** expose a
single `upgrade` function. Instead, an admin must first *schedule* the upgrade,
wait out the timelock delay, and then *apply* it. This gives affected parties
time to notice and react before new code can control escrowed funds.

## Prerequisites

- The new contract WASM has been built and its hash (`<NEW_WASM_HASH>`) is known.
- You control the admin address (`<ADMIN_ADDRESS>`) configured on the contract.
- The Soroban CLI is installed and configured for the target network.

## Step 1 — Schedule the upgrade

Call `schedule_upgrade` with the admin address and the new WASM hash:

```bash
soroban contract invoke \
  --id <EXISTING_CONTRACT_ID> \
  -- \
  schedule_upgrade \
  --caller <ADMIN_ADDRESS> \
  --new_wasm_hash <NEW_WASM_HASH>
```

This records the pending upgrade and emits an `upgrade_scheduled` event
containing the new WASM hash and the ledger at which the upgrade becomes
applicable (`apply_ledger`). The upgrade is **not** applied yet.

## Step 2 — Verify the timelock window

The upgrade cannot be applied until the configured timelock delay has elapsed.
The delay is stored in `UpgradeDelayLedgers` (default `17280` ledgers, roughly
one day). The `apply_ledger` value emitted by `schedule_upgrade` is
`current_ledger + delay`.

Before applying, confirm the current ledger sequence is greater than or equal
to `apply_ledger`. You can read the current ledger sequence from the network
(e.g. via `soroban ledger latest` or your RPC provider) and compare it against
the `apply_ledger` reported in the `upgrade_scheduled` event. If the timelock
has not elapsed, `apply_upgrade` will revert with
`Upgrade timelock has not elapsed`.

## Step 3 — Apply the upgrade

Once the timelock has elapsed, call `apply_upgrade` with the admin address:

```bash
soroban contract invoke \
  --id <EXISTING_CONTRACT_ID> \
  -- \
  apply_upgrade \
  --caller <ADMIN_ADDRESS>
```

This clears the pending upgrade, increments the stored contract `Version`, and
installs the new WASM via `update_current_contract_wasm`. It reverts with
`No pending upgrade` if nothing was scheduled.

## Cancelling a scheduled upgrade

If the upgrade should not proceed (e.g. a defect was found during the timelock
window), cancel it before applying by calling `cancel_upgrade` with the admin
address:

```bash
soroban contract invoke \
  --id <EXISTING_CONTRACT_ID> \
  -- \
  cancel_upgrade \
  --caller <ADMIN_ADDRESS>
```

This removes the pending upgrade and emits an `upgrade_cancelled` event. It
reverts with `No pending upgrade` if nothing was scheduled. After cancelling,
you may schedule a new upgrade from Step 1.
