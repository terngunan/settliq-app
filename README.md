# Settliq Stellar Contracts

> Soroban smart contracts powering the Settliq crypto-to-fiat settlement platform on Stellar.

## Overview

This repository contains the Soroban (Stellar smart contract platform) contracts that back Settliq's on-chain settlement flow — payment escrow, merchant registry, fee handling, and related infrastructure. It does **not** contain the backend API or frontend application; those live in their own repositories:

- **Backend (Settlement API)** — `settliq-backend`
- **Frontend (Merchant & Customer Portal)** — `settliq-frontend`

## Contracts

All contracts live under [`settliq_contracts/contracts/`](settliq_contracts/contracts/):

| Contract | Purpose |
|---|---|
| `admin_timelock` | Timelocked admin actions |
| `batch_payments` | Batched payment processing |
| `fee_calculator` | Fee computation logic |
| `fee_distributor` | Fee distribution to recipients |
| `liquidity_router` | Liquidity routing for settlements |
| `merchant_registry` | Merchant registration and metadata |
| `multisig_admin` | Multi-signature admin control |
| `payment_escrow` | Escrow for customer payments (approve → deposit → settle) |
| `payment_request` | Payment request lifecycle |
| `rbac_access` | Role-based access control |
| `reconciliation` | Settlement reconciliation |
| `settlement_ledger` | On-chain settlement record-keeping |
| `slippage_protection` | Slippage guards for conversions |
| `stellar_confirmations` | Confirmation tracking for Stellar transactions |

## Development

### Prerequisites

- Rust with the `wasm32v1-none` target
- [`stellar-cli`](https://developers.stellar.org/docs/tools/developer-tools) (for deployment)
- `wasm-opt` (binaryen), for optimised release builds

```bash
rustup target add wasm32v1-none
```

### Build & test

```bash
make build              # debug build of all contracts
make build-optimised    # release build + wasm-opt -Oz
make check-wasm-size    # enforce the 64 KB per-contract WASM limit
make test               # cargo test across the workspace
```

Or directly with cargo, from `settliq_contracts/`:

```bash
cd settliq_contracts
cargo build --target wasm32v1-none --release
cargo test
```

## CI/CD

- **CI** ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) — runs `cargo test`, coverage (tarpaulin), and a WASM build on every push/PR.
- **Deploy** ([`.github/workflows/deploy.yml`](.github/workflows/deploy.yml)) — builds and optimises WASMs, then deploys to Stellar testnet on merges to `main` and to mainnet on version tags (manual approval required).

## Monitoring

Grafana/Loki/Promtail configs under [`grafana/`](grafana/) and [`docker-compose.yml`](docker-compose.yml) are provided for local observability tooling.

## Docs

- [Escrow upgrade procedure](docs/escrow_upgrade_procedure.md)
- [Security docs](docs/security/)

## Links

- **Website**: [https://settliq.xyz](https://settliq.xyz)
- **Documentation**: [https://docs.settliq.xyz](https://docs.settliq.xyz)

## Handsoff notes

<!-- handsoff-issue-105 -->
- #105: merchant_registry: register_merchant allows empty business name strings

<!-- handsoff-issue-106 -->
- #106: merchant_registry: Unbounded page_size parameter in merchants query risks gas budget exhaustion

<!-- handsoff-issue-241 -->
- #241: [Smart Contract] platform_stats: stats function references undeclared identifiers total_merchants and non-existent current_active
