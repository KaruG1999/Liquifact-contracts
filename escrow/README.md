# LiquiFact Escrow (`liquifact_escrow`)

Soroban escrow for invoice funding, settlement, and investor claims. This README adds **formal invariant stubs** (machine-readable IDs plus math-style properties), **test traceability**, **attestation hashing**, **minimum contribution floors**, and **unique investor caps** (issues #102–#105).

## Deterministic Yield Calculation

The contract provides a dedicated helper function `calculate_principal_plus_yield(principal, yield_bps)` for computing payout amounts:

- **Formula**: `payout = principal + (principal × yield_bps) / 10_000`
- **Rounding**: Integer division truncates toward zero (floor for positive values), conservative for the contract
- **Overflow Protection**: Uses checked arithmetic with explicit panics on overflow
- **Validation**: Asserts principal ≥ 0 and yield_bps ∈ [0, 10_000]
- **Determinism**: Pure integer math ensures identical results across all platforms

### Usage Example

```rust
// 10,000 at 800 bps (8%) = 10,800
let payout = calculate_principal_plus_yield(10_000i128, 800i64);
assert_eq!(payout, 10_800i128);
```

### Security Properties

- No floating-point arithmetic (avoids precision issues)
- Input validation prevents invalid parameters
- Checked multiplication and addition prevent overflow
- Conservative rounding (truncation) protects contract solvency

## Formal invariant specification (stubs)

Intended for auditors, formal-methods tooling, and regression design. Properties are stated over escrow state unless noted. Status codes: `0=open`, `1=funded`, `2=settled`, `3=withdrawn`.

```yaml
schema_version: 5
invariants:
  - id: ESC-FUND-001
    name: funded_amount_monotone
    math: "forall funding txs in open status: funded_amount' = funded_amount + amount ∧ amount > 0"
    tests:
      - test::prop_funded_amount_non_decreasing
      - test::test_repeated_funding_accumulates_contribution

  - id: ESC-FUND-002
    name: funded_amount_upper_implicit
    math: "funded_amount = sum over investors of contribution(investor) while bookkeeping invariants hold"
    tests:
      - test::test_contributions_sum_equals_funded_amount
      - test::test_multiple_investors_tracked_independently

  - id: ESC-STA-001
    name: status_monotone
    math: "status never decreases; valid transitions 0→1→(2|3); 3 and 2 are terminal from 1"
    tests:
      - test::prop_status_only_increases
      - test::test_withdraw_funded_then_cannot_settle

  - id: ESC-CLM-001
    name: investor_claim_once
    math: "forall investor: InvestorClaimed(investor) set at most once after status=2"
    tests:
      - test::test_claim_investor_twice_panics
      - test::test_claim_succeeds_after_commitment_and_settle

  - id: ESC-ATT-001
    name: primary_attestation_single_set
    math: "PrimaryAttestationHash absent ∨ uniquely set; second bind_primary fails"
    tests:
      - test::test_bind_primary_attestation_single_set_and_get
      - test::test_bind_primary_attestation_twice_panics

  - id: ESC-ATT-002
    name: attestation_append_bounded
    math: "len(AttestationAppendLog) ≤ MAX_ATTESTATION_APPEND_ENTRIES"
    tests:
      - test::test_append_attestation_respects_max_length

  - id: ESC-MIN-001
    name: min_contribution_per_call
    math: "if min_floor > 0 then each fund amount ≥ min_floor"
    tests:
      - test::test_min_contribution_floor_rejects_below_and_accepts_equal
      - test::test_min_floor_applies_to_follow_on_fund

  - id: ESC-CAP-001
    name: unique_funder_cap
    math: "if cap = MaxUniqueInvestorsCap then #{investor : contribution(investor) > 0} ≤ cap"
    tests:
      - test::test_max_unique_investors_cap_enforced

  - id: ESC-INI-001
    name: single_initialization_guard
    math: "Initialized key set exactly once; subsequent init calls panic"
    tests:
      - test::test_double_init_panics
      - test::test_init_sets_initialized_flag
```

## New init parameters

`init(..., yield_tiers, min_contribution, max_unique_investors)`:

| Parameter | Type | Meaning |
|-----------|------|---------|
| `min_contribution` | `Option<i128>` | When `Some(x)`, requires every `fund` / `fund_with_commitment` amount `≥ x`, and `x ≤` initial `amount`. `None` disables the floor. |
| `max_unique_investors` | `Option<u32>` | When `Some(n)`, at most `n` distinct investor addresses may make a first deposit. `None` means unlimited. |

## Attestation API (off-chain bundle binding)

- **`bind_primary_attestation_hash(digest: BytesN<32>)`**: admin; **single-set** (immutable once stored).
- **`append_attestation_digest(digest)`**: admin; **append-only** log, capacity `MAX_ATTESTATION_APPEND_ENTRIES` (see `lib.rs`).
- **Frontrunning**: first finalized binding transaction wins for the primary slot; integrators should read on-chain state or events after finality.

## Security review sign-off checklist (pre-deploy)

Use as a human gate; not a substitute for professional audit.

- [ ] `admin` is a multisig or governed contract (legal hold and attestation are admin-gated).
- [ ] Escrow has a **single-initialization guard** to prevent re-initialization after deployment.
- [ ] Funding token is standard SEP-41; fee-on-transfer tokens are out of scope (see module docs and `docs/ESCROW_TOKEN_INTEGRATION_CHECKLIST.md`).
- [ ] `min_contribution` and `max_unique_investors` match the legal offering (floor vs. target; cap is per-address, not KYC’d entity).
- [ ] Attestation digests match the intended off-chain bundle (hash algorithm and canonical encoding documented off-chain).
- [ ] Maturity and claim-lock semantics use ledger time only (see `lib.rs` rustdoc).
- [ ] CI: `cargo fmt --all -- --check`, `cargo test`, `cargo llvm-cov --features testutils --fail-under-lines 95` pass.

## CI / coverage

The GitHub Actions workflow runs format, build, tests, and **≥ 95% line coverage** via `cargo llvm-cov`.

## Test output (local)

Run:

```bash
cargo test -p liquifact_escrow
cargo llvm-cov --features testutils --fail-under-lines 95 --summary-only -p liquifact_escrow
```

All tests should pass; coverage summary should meet the threshold (recent run: total line cover ~99% for this crate).
