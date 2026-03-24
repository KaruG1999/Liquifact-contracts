# LiquiFact Contracts

Soroban smart contracts for **LiquiFact** — the global invoice liquidity network on Stellar. This repo contains the **escrow** contract that holds investor funds for tokenized invoices until settlement.

Part of the LiquiFact stack: **frontend** (Next.js) | **backend** (Express) | **contracts** (this repo).

---

## Prerequisites

- **Rust** 1.70+ (stable)
- **Soroban CLI** (optional, for deployment): [Stellar Soroban docs](https://developers.stellar.org/docs/smart-contracts/getting-started/soroban-cli)

For CI and local checks you only need Rust and `cargo`.

---

## Setup

1. **Clone the repo**

   ```bash
   git clone <this-repo-url>
   cd liquifact-contracts
   ```

2. **Build**

   ```bash
   cargo build
   ```

3. **Run tests**

   ```bash
   cargo test
   ```

---

## Development

| Command                    | Description                   |
|----------------------------|-------------------------------|
| `cargo build`              | Build all contracts           |
| `cargo test`               | Run unit tests                |
| `cargo fmt`                | Format code                   |
| `cargo fmt -- --check`     | Check formatting (used in CI) |

---

## Project structure

```
liquifact-contracts/
├── Cargo.toml           # Workspace definition
├── escrow/
│   ├── Cargo.toml       # Escrow contract crate
│   └── src/
│       ├── lib.rs       # LiquiFact escrow contract (init, fund, settle, migrate)
│       └── test.rs      # Unit tests
└── .github/workflows/
    └── ci.yml           # CI: fmt, build, test
```

### Escrow contract (high level)

- **init** — Create an invoice escrow (invoice id, SME address, amount, yield bps, maturity).
- **get_escrow** — Read current escrow state.
- **get_version** — Return the stored schema version number.
- **fund** — Record investor funding; status becomes "funded" when target is met.
- **settle** — Mark escrow as settled (buyer paid; investors receive principal + yield).
- **migrate** — Upgrade storage from an older schema version to the current one (see below).

---

## Contract migration strategy

### Overview

The escrow contract stores its state as a single [`InvoiceEscrow`](escrow/src/lib.rs) struct under the instance storage key `"escrow"`, alongside a `"version"` key that holds the current schema version (`u32`).

Any change to the struct layout (adding, removing, or retyping a field) is a **breaking schema change** and requires a version bump and a migration path.

### Version history

| Version | Description |
|---------|-------------|
| 1       | Initial schema — `invoice_id`, `sme_address`, `amount`, `funding_target`, `funded_amount`, `yield_bps`, `maturity`, `status`, `version` |

### How versioning works

- `SCHEMA_VERSION` in `lib.rs` is the source of truth for the current schema.
- Every `init` call writes `SCHEMA_VERSION` into both the struct's `version` field and the `"version"` storage key.
- `get_version()` lets off-chain tooling (indexers, upgrade scripts) read the stored version before deciding whether to call `migrate`.

### Adding a new schema version (step-by-step)

1. **Bump `SCHEMA_VERSION`** in `lib.rs` (e.g. `1` to `2`).
2. **Keep the old struct** — add a `legacy` module (or a type alias like `InvoiceEscrowV1`) so the old bytes can still be deserialized.
3. **Add a migration arm** in `LiquifactEscrow::migrate`:
   ```rust
   if from_version == 1 {
       let old: InvoiceEscrowV1 = env.storage().instance()
           .get(&symbol_short!("escrow")).unwrap();
       let new = InvoiceEscrow {
           // spread old fields, default new ones
           new_field: default_value,
           version: 2,
           ..old.into()
       };
       env.storage().instance().set(&symbol_short!("escrow"), &new);
       env.storage().instance().set(&symbol_short!("version"), &2u32);
   }
   ```
4. **Write a test** in `test.rs` that manually writes the old struct bytes into storage and asserts the migrated state is correct.
5. **Gate `migrate` behind admin auth** before deploying to production (see security notes below).

### Deployment upgrade flow

```
1. Deploy new WASM (bump SCHEMA_VERSION, add migration arm)
2. Call get_version()  ->  confirm stored version == N
3. Call migrate(N)     ->  storage upgraded to N+1
4. Call get_version()  ->  confirm stored version == N+1
5. Resume normal operations
```

The contract rejects `migrate` calls that:
- Pass a `from_version` that does not match the stored version (prevents accidental double-migration).
- Pass a `from_version >= SCHEMA_VERSION` (already up to date).

### Security notes

- **Re-initialization guard** — `init` panics if the escrow is already initialized, preventing state overwrite.
- **`migrate` must be admin-gated in production** — the current implementation is open for testability. Before mainnet deployment, add `admin_address.require_auth()` at the top of `migrate` so only the contract deployer can trigger upgrades.
- **No silent data loss** — migration arms must explicitly handle every field. Defaulting a field to zero/false is intentional and must be documented in the version history table above.
- **Immutable history** — old migration arms should never be removed; they ensure any instance at any historical version can be brought forward step-by-step.

---

## CI/CD

GitHub Actions runs on every push and pull request to `main`:

- **Format** — `cargo fmt --all -- --check`
- **Build** — `cargo build`
- **Tests** — `cargo test`

Keep formatting and tests passing before opening a PR.

---

## Contributing

1. **Fork** the repo and clone your fork.
2. **Create a branch** from `main`: `git checkout -b feature/your-feature` or `fix/your-fix`.
3. **Setup**: ensure Rust stable is installed; run `cargo build` and `cargo test`.
4. **Make changes**:
   - Follow existing patterns in `escrow/src/lib.rs`.
   - Add or update tests in `escrow/src/test.rs`.
   - Format with `cargo fmt`.
5. **Verify locally**:
   - `cargo fmt --all -- --check`
   - `cargo build`
   - `cargo test`
6. **Commit** with clear messages (e.g. `feat(escrow): X`, `test(escrow): Y`).
7. **Push** to your fork and open a **Pull Request** to `main`.
8. Wait for CI and address review feedback.

We welcome new contracts (e.g. settlement, tokenization helpers), tests, and docs that align with LiquiFact's invoice financing flow.

---

## License

MIT (see root LiquiFact project for full license).
