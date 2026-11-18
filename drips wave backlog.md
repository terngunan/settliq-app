# Settliq App — Drips Wave Issue Backlog

A comprehensive, production-grade backlog of exactly 100 code-grounded, actionable GitHub issue drafts for the **Settliq** repository. Every issue is derived from concrete findings within the smart contracts, backend services, CI/CD pipelines, monitoring stack, and documentation.

---

## Backlog Taxonomy & Conventions

### Priority Framework
- **P0 — Critical:** Severe security vulnerabilities, state corruption vectors, financial loss risks, or total blocking defects requiring immediate resolution.
- **P1 — High:** Significant correctness bugs, authorization bypasses, storage quota threats, or breaking CI/CD failures.
- **P2 — Medium:** Meaningful functional defects, edge-case validation errors, event/audit gaps, observability lapses, or performance regressions.
- **P3 — Low:** Maintainability improvements, developer experience enhancements, minor documentation discrepancies, or cosmetic UI/status page corrections.

### Label Taxonomy
- `bug`: Concrete functional defects, runtime errors, or incorrect logic.
- `security`: Access control, authentication, cryptography, or vulnerability remediations.
- `enhancement`: Performance improvements, robustness guards, or storage optimizations.
- `testing`: Missing unit, integration, property, or end-to-end test suites.
- `documentation`: Runbook, deployment configuration, or API documentation corrections.
- `devex`: Developer tooling, build configurations, or local development workflows.
- `performance`: Algorithmic efficiency, gas optimization, or throughput improvements.
- `area:contracts`: Soroban Rust smart contracts (`settliq_contracts/`).
- `area:backend`: NestJS TypeScript settlement API (`backend/`).
- `area:database`: PostgreSQL entities, migrations, and database connections.
- `area:payments`: Payment escrow, batch settlement, and routing logic.
- `area:infrastructure`: CI/CD workflows, Docker, Grafana, and Cloudflare monitoring.
- `area:frontend`: Status page web client and presentation layer.

---

---

---

## ISSUE-032: platform_stats: Unchecked integer additions on platform counters risk panics in release builds

**Priority:** P2

**Labels:** `bug`, `area:contracts`

### Problem

In `platform_stats`, `record_merchant` and `record_payment` perform raw additions: `&(prev + 1)`, `&(tp + 1)`, `&(vol + amount_usd)`, and `&(count + 1)`. In `settliq_contracts/Cargo.toml`, a load-bearing comment warns that `overflow-checks = true` is required specifically because these contracts use non-checked arithmetic. However, relying on compiler build flags rather than explicit safe math is brittle; if build flags change, overflow could wrap silently. Furthermore, `vol + amount_usd` has no validation that `amount_usd >= 0`, meaning negative amounts could decrease total settled volume.

### Codebase Evidence

In `settliq_contracts/contracts/platform_stats/src/lib.rs`, lines 59-60, 69-70, 79-80, 86-87:
- `env.storage().instance().set(&DataKey::TotalMerchants, &(prev + 1));`
- `env.storage().instance().set(&DataKey::TotalPayments, &(tp + 1));`
- `env.storage().instance().set(&DataKey::TotalSettledVolumeUsd, &(vol + amount_usd));`
- `env.storage().instance().set(&key, &(count + 1));`

### Expected Behavior

All counter increments must use explicit `checked_add` or `saturating_add`, and `record_payment` must require `amount_usd >= 0`.

### Proposed Solution

Refactor all arithmetic operations in `settliq_contracts/contracts/platform_stats/src/lib.rs` to use `.checked_add(...).expect("overflow")` and assert `amount_usd >= 0`.

### Acceptance Criteria

- [ ] All additions in `platform_stats` use explicit checked math methods.
- [ ] Submitting negative `amount_usd` panics with an error.
- [ ] Unit tests verify behavior under large volumes and negative amount inputs.

### Testing Requirements

Add unit tests in `settliq_contracts/contracts/platform_stats/src/test.rs` asserting panic on negative volume and verifying checked arithmetic.

### Dependencies

None identified.

---

---

## ISSUE-082: api-key.strategy: API key extraction strictly inspects lowercase header without case-insensitive fallback or whitespace trimming

**Priority:** P2

**Labels:** `bug`, `area:backend`

### Problem

In `backend/src/auth/strategies/api-key.strategy.ts`, the strategy extracts the API key strictly using `req.headers['x-api-key']`. Certain HTTP proxies and legacy clients format headers as `X-API-KEY` or `X-Api-Key`. Additionally, the extracted key string is passed directly to `AuthService.findMerchantByApiKey` without trimming leading or trailing whitespace. Any copy-paste whitespace causes `bcrypt.compare` to fail, resulting in unexpected authentication rejections.

### Codebase Evidence

In `backend/src/auth/strategies/api-key.strategy.ts:14-22`:
```typescript
const key = req.headers['x-api-key'] as string;
if (!key) throw new UnauthorizedException('Missing API key');
```

### Expected Behavior

API key extraction should inspect header variations case-insensitively and trim any surrounding whitespace before processing.

### Proposed Solution

Update header extraction to `const raw = (req.headers['x-api-key'] || req.headers['X-API-KEY'] || req.headers['X-Api-Key']) as string; const key = raw?.trim();`.

### Acceptance Criteria

- [ ] API keys passed with `X-API-KEY` or `x-api-key` headers are accepted identically.
- [ ] API keys containing accidental leading or trailing whitespace authenticate successfully.
- [ ] Missing or empty keys return HTTP 401 Unauthorized.

### Testing Requirements

Add unit tests verifying API key authentication with mixed-case headers and padded whitespace strings.

### Dependencies

None identified.

---

