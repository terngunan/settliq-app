# Deployment configuration

`.github/workflows/deploy.yml` deploys all 16 workspace contracts to testnet on
every merge to `main`, and to mainnet on a `v*` tag. Ten of those contracts take
`__constructor` arguments, so the workflow reads them from GitHub Actions
**variables** (Settings → Secrets and variables → Actions → Variables).

Values are per-network, prefixed `TESTNET_` or `MAINNET_`. The deploy step
checks every required variable up front and fails naming the missing one, rather
than letting an opaque CLI error surface halfway through a partial deploy.

> Addresses and tuning values are configuration, not credentials, so they are
> variables rather than secrets. The only secrets involved remain
> `TESTNET_DEPLOY_SECRET` and `MAINNET_DEPLOY_SECRET`, the deployer keypairs.

## Workspace contracts

The `settliq_contracts` workspace contains 16 contracts, all deployed by the
workflow:

`admin_auth`, `admin_timelock`, `batch_payments`, `fee_calculator`,
`fee_distributor`, `liquidity_router`, `merchant_registry`, `multisig_admin`,
`payment_escrow`, `payment_request`, `platform_stats`, `rbac_access`,
`reconciliation`, `settlement_ledger`, `slippage_protection` and
`stellar_confirmations`.

## Required variables

| Variable (per-network prefix) | Used by | Example |
| --- | --- | --- |
| `…_DEPLOY_ADMIN` | `admin_timelock`, `merchant_registry`, `reconciliation`, `settlement_ledger`, `slippage_protection`, `stellar_confirmations`, `fee_calculator`, `fee_distributor`, `payment_escrow`, `batch_payments`, `liquidity_router` | `GA…` |
| `…_RBAC_SUPER_ADMIN` | `rbac_access` (`super_admin`) | `GA…` |
| `…_XLM_TOKEN` | `payment_escrow` | `CA…` |
| `…_USDC_TOKEN` | `payment_escrow`, `fee_distributor` | `CA…` |
| `…_TREASURY` | `fee_distributor` | `GA…` |
| `…_LP_ADDRESS` | `fee_distributor` | `GA…` |
| `…_LP_SHARE_BPS` | `fee_distributor` | `2000` |
| `…_FEE_TIERS` | `fee_calculator` | JSON array, see below |
| `…_MULTISIG_ADMIN_1/2/3` | `multisig_admin` | `GA…` |
| `…_ESCROW_DEFAULT_TTL_LEDGERS` | `payment_escrow` | `518400` |
| `…_EMERGENCY_SIGNERS` | `payment_escrow` | JSON array, see below |
| `…_EMERGENCY_TREASURY` | `payment_escrow` | `GA…` |
| `…_EMERGENCY_COOLDOWN_LEDGERS` | `payment_escrow` | `17280` |
| `…_CONFIRMATION_COUNT` | `stellar_confirmations` | `3` |
| `…_BATCH_MAX_BATCH_SIZE` | `batch_payments` (`max_batch_size`) | `100` |
| `…_BATCH_MAX_AMOUNT_PER_PAYMENT` | `batch_payments` (`max_amount_per_payment`) | `1000000000` |
| `…_BATCH_MAX_TOTAL_BATCH_AMOUNT` | `batch_payments` (`max_total_batch_amount`) | `10000000000` |

### Optional

| Variable | Effect |
| --- | --- |
| `…_REGISTRY_ADDRESS` | `payment_escrow`'s `registry` argument. Left unset, it defaults to the `merchant_registry` deployed in the same run, so the two are wired together without a follow-up call. Set it to point at an already-deployed registry instead. |

## Structured values

Two constructors take container types, which the Stellar CLI expects as JSON.

`…_EMERGENCY_SIGNERS` — `Vec<Address>`:

```json
["GA…", "GB…", "GC…"]
```

`…_FEE_TIERS` — `Vec<FeeTier>`. Field names must match the struct in
`settliq_contracts/contracts/fee_calculator/src/lib.rs`:

```json
[
  { "threshold_usdc": "0",         "fee_bps": 100 },
  { "threshold_usdc": "10000001",  "fee_bps": 50  }
]
```

## Contracts with constructor arguments

`batch_payments` takes five constructor arguments: `admin` (from
`…_DEPLOY_ADMIN`), `max_batch_size` (`…_BATCH_MAX_BATCH_SIZE`),
`max_amount_per_payment` (`…_BATCH_MAX_AMOUNT_PER_PAYMENT`) and
`max_total_batch_amount` (`…_BATCH_MAX_TOTAL_BATCH_AMOUNT`).

`liquidity_router` takes a single `admin` argument, supplied from
`…_DEPLOY_ADMIN`.

## Contracts with no constructor

`payment_request` takes no constructor arguments and needs no configuration.

## Deploy order

The workflow deploys constructor-free contracts first, then admin-only ones,
then the multi-argument ones. `merchant_registry` is deployed before
`payment_escrow` so the registry address is available to it.

Contract IDs for every deployed contract are written to the job summary and
exposed as job outputs named `<contract>_id`.

## Related

- [`escrow_upgrade_procedure.md`](escrow_upgrade_procedure.md) — upgrading
  `payment_escrow` in place.
- [`../settliq_contracts/contracts/payment_escrow/EMERGENCY_RUNBOOK.md`](../settliq_contracts/contracts/payment_escrow/EMERGENCY_RUNBOOK.md)
  — incident response, including rollback.
