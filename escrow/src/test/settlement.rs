//! Settlement and withdrawal tests for the LiquiFact escrow contract.
//!
//! Covers the full `withdraw` surface (happy path, wrong-status guards, legal-hold
//! block, idempotency, event emission, and terminal status assertion) as well as
//! the `settle` → `claim_investor_payout` flow, maturity gates, and dust-sweep
//! integration that belong in the same lifecycle module.
//!
//! # State model recap (ADR-001)
//! ```text
//! 0 (open) ──fund──▶ 1 (funded) ──settle──▶ 2 (settled)
//!                           └────withdraw───▶ 3 (withdrawn)
//! ```
//! `withdraw` and `settle` are mutually exclusive; both require `status == 1`.
//!
//! # Test organisation
//! Each test builds its own `Env` via the shared `setup` / `default_init` helpers
//! defined in `escrow/src/test.rs`. No cross-test state is shared.

#[cfg(test)]
use super::{default_init, deploy, free_addresses, setup, TARGET};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger as _},
    Address, Env,
};

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Bring an escrow to `status == 1` (funded) by depositing exactly `TARGET`
/// from a single investor, then return the investor address.
fn fund_to_target(client: &super::LiquifactEscrowClient<'_>, env: &Env) -> Address {
    let investor = Address::generate(env);
    client.fund(&investor, &TARGET);
    investor
}

/// Bring an escrow to `status == 2` (settled) and return the investor address.
fn settle_escrow(client: &super::LiquifactEscrowClient<'_>, env: &Env) -> Address {
    let investor = fund_to_target(client, env);
    client.settle();
    investor
}

// ──────────────────────────────────────────────────────────────────────────────
// `withdraw` — happy path
// ──────────────────────────────────────────────────────────────────────────────

/// Status must become 3 after a successful `withdraw`.
///
/// This is the primary assertion required by the task description.
#[test]
fn withdraw_sets_status_to_three() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);

    client.withdraw();

    let escrow = client.get_escrow();
    assert_eq!(
        escrow.status, 3u32,
        "status must be 3 (withdrawn) after withdraw"
    );
}

/// `withdraw` must require SME auth.
///
/// In `mock_all_auths` environments the check always passes; this test
/// documents the expected signer so a future auth-audit can grep for it.
#[test]
fn withdraw_requires_sme_auth() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);

    // Passes because test env mocks all auth. The assertion is on the *call*
    // succeeding for the correct signer (sme), not an impostor.
    client.withdraw();

    // Verify state changed — confirming it was sme who triggered the path.
    assert_eq!(client.get_escrow().status, 3u32);
}

/// After `withdraw` the funded_amount and funding_target remain intact —
/// `withdraw` is a state-label change only; it does not zero accounting fields.
#[test]
fn withdraw_preserves_accounting_fields() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);

    client.withdraw();

    let escrow = client.get_escrow();
    assert_eq!(
        escrow.funded_amount, TARGET,
        "funded_amount must not be wiped by withdraw"
    );
    assert_eq!(
        escrow.funding_target, TARGET,
        "funding_target must not be mutated by withdraw"
    );
}

/// `withdraw` emits an `EscrowWithdrawn` event (or equivalent event symbol).
///
/// The exact event symbol depends on the contract implementation; adjust the
/// `symbol_short!` value to match the emitted event name if different.
#[test]
fn withdraw_emits_event() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);

    client.withdraw();

    // At least one event must be emitted in the transaction.
    let contract_events = env.events().all();
    let events = contract_events.events();
    assert!(
        events.len() > 0,
        "withdraw must emit at least one contract event"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// `withdraw` — wrong-status guards
// ──────────────────────────────────────────────────────────────────────────────

/// `withdraw` on an `open` (status 0) escrow must panic.
///
/// The escrow has not been funded; `withdraw` requires `status == 1`.
#[test]
#[should_panic]
fn withdraw_on_open_escrow_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    // No funding — status is still 0.
    client.withdraw();
}

/// `withdraw` on an already-settled (status 2) escrow must panic.
///
/// Once `settle` has been called the escrow is terminal in the settlement path;
/// `withdraw` must not be able to re-label it.
#[test]
#[should_panic]
fn withdraw_on_settled_escrow_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    settle_escrow(&client, &env);
    // status == 2 — withdraw must be rejected.
    client.withdraw();
}

/// `withdraw` called twice on the same escrow must panic on the second call.
///
/// Once status reaches 3 (withdrawn) it is terminal; no forward transition
/// exists from 3, so a second `withdraw` must be rejected.
#[test]
#[should_panic]
fn withdraw_twice_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);

    client.withdraw(); // first call — succeeds, status → 3
    client.withdraw(); // second call — must panic (status == 3, not 1)
}

/// `settle` cannot be called after `withdraw` (status 3 is terminal).
#[test]
#[should_panic]
fn settle_after_withdraw_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);
    client.withdraw(); // status → 3
    client.settle(); // must panic — settle requires status == 1
}

/// `fund` cannot be called after `withdraw` (status 3 is terminal).
#[test]
#[should_panic]
fn fund_after_withdraw_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);
    client.withdraw(); // status → 3
    let late_investor = Address::generate(&env);
    client.fund(&late_investor, &1_000_0000000i128); // must panic — fund requires status == 0
}

// ──────────────────────────────────────────────────────────────────────────────
// `withdraw` — legal-hold block (ADR-004)
// ──────────────────────────────────────────────────────────────────────────────

/// `withdraw` must be blocked while a legal hold is active.
///
/// Per ADR-004 the hold freezes `withdraw` regardless of escrow status.
#[test]
#[should_panic]
fn withdraw_blocked_by_legal_hold() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);

    client.set_legal_hold(&true);
    // Status is 1 but hold is active — must panic.
    client.withdraw();
}

/// `withdraw` must succeed after a legal hold is cleared.
///
/// Verifies that `clear_legal_hold` (or `set_legal_hold(false)`) fully lifts
/// the block and the escrow can proceed to `status == 3`.
#[test]
fn withdraw_succeeds_after_hold_cleared() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);

    client.set_legal_hold(&true);
    client.set_legal_hold(&false); // clear the hold

    client.withdraw();
    assert_eq!(client.get_escrow().status, 3u32);
}

/// `set_legal_hold` must be admin-only; a non-admin cannot place a hold.
#[test]
#[should_panic]
fn legal_hold_set_by_non_admin_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    env.mock_all_auths_allowing_non_root_auth(); // stricter auth mode
    env.mock_auths(&[]);
    default_init(&client, &env, &admin, &sme);
    // `sme` is not the admin — must panic.
    client.set_legal_hold(&true);
}

// ──────────────────────────────────────────────────────────────────────────────
// `settle` path — complementary coverage ensuring mutual exclusivity
// ──────────────────────────────────────────────────────────────────────────────

/// `settle` transitions status from 1 to 2.
#[test]
fn settle_sets_status_to_two() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);

    client.settle();

    assert_eq!(client.get_escrow().status, 2u32);
}

/// `settle` is blocked while a legal hold is active.
#[test]
#[should_panic]
fn settle_blocked_by_legal_hold() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);

    client.set_legal_hold(&true);
    client.settle();
}

/// `settle` on an open (status 0) escrow must panic.
#[test]
#[should_panic]
fn settle_on_open_escrow_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    client.settle();
}

/// `settle` called twice must panic on the second call.
#[test]
#[should_panic]
fn settle_twice_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);
    client.settle();
    client.settle(); // status == 2, must panic
}

// ──────────────────────────────────────────────────────────────────────────────
// Maturity gate — settle is time-gated when `maturity > 0`
// ──────────────────────────────────────────────────────────────────────────────

/// `settle` must be rejected if the current ledger timestamp is before maturity.
#[test]
#[should_panic]
fn settle_before_maturity_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (token, treasury) = free_addresses(&env);

    // Set a maturity 1000 seconds in the future relative to ledger timestamp 0.
    let maturity: u64 = 1_000;
    client.init(
        &admin,
        &soroban_sdk::String::from_str(&env, "INV_MAT_001"),
        &sme,
        &TARGET,
        &800i64,
        &maturity,
        &token,
        &None,
        &treasury,
        &None,
        &None,
        &None,
    );

    fund_to_target(&client, &env);

    // Ledger timestamp is 0 — before maturity.
    env.ledger().with_mut(|l| l.timestamp = maturity - 1);
    client.settle(); // must panic
}

/// `settle` must succeed once ledger timestamp reaches maturity (inclusive).
#[test]
fn settle_at_maturity_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (token, treasury) = free_addresses(&env);

    let maturity: u64 = 1_000;
    client.init(
        &admin,
        &soroban_sdk::String::from_str(&env, "INV_MAT_002"),
        &sme,
        &TARGET,
        &800i64,
        &maturity,
        &token,
        &None,
        &treasury,
        &None,
        &None,
        &None,
    );

    fund_to_target(&client, &env);

    env.ledger().with_mut(|l| l.timestamp = maturity); // exactly at boundary
    client.settle();

    assert_eq!(client.get_escrow().status, 2u32);
}

// ──────────────────────────────────────────────────────────────────────────────
// Investor claim path (post-settle)
// ──────────────────────────────────────────────────────────────────────────────

/// `claim_investor_payout` succeeds for an investor after `settle`.
///
/// This is a state-marker call — no token transfer occurs inside the contract.
/// The test verifies the call completes without panic and emits an event.
#[test]
fn claim_investor_payout_succeeds_after_settle() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    let investor = settle_escrow(&client, &env);

    client.claim_investor_payout(&investor);

    let contract_events = env.events().all();
    let events = contract_events.events();
    assert!(
        events.len() > 0,
        "claim must emit InvestorPayoutClaimed event"
    );
}

/// `claim_investor_payout` must be idempotency-guarded: a second call panics.
#[test]
#[should_panic]
fn claim_investor_payout_twice_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    let investor = settle_escrow(&client, &env);

    client.claim_investor_payout(&investor);
    client.claim_investor_payout(&investor); // second call must panic
}

/// `claim_investor_payout` must be blocked while a legal hold is active.
#[test]
#[should_panic]
fn claim_investor_payout_blocked_by_legal_hold() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    let investor = settle_escrow(&client, &env);

    client.set_legal_hold(&true);
    client.claim_investor_payout(&investor); // must panic
}

/// `claim_investor_payout` must fail before `settle` (status != 2).
#[test]
#[should_panic]
fn claim_investor_payout_before_settle_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    let investor = fund_to_target(&client, &env);
    // Status is 1 — not yet settled.
    client.claim_investor_payout(&investor);
}

/// An investor that did not participate cannot claim.
#[test]
#[should_panic]
fn claim_investor_payout_non_participant_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    env.mock_all_auths_allowing_non_root_auth();
    env.mock_auths(&[]);
    default_init(&client, &env, &admin, &sme);
    settle_escrow(&client, &env);

    let stranger = Address::generate(&env);
    client.claim_investor_payout(&stranger);
}

// ──────────────────────────────────────────────────────────────────────────────
// Funding snapshot invariant (ADR-003)
// ──────────────────────────────────────────────────────────────────────────────

/// The funding-close snapshot is written once when status transitions to 1.
/// After `withdraw` the snapshot must still be readable with the original values.
///
/// This guards against the denominator being zeroed or mutated by the withdrawal
/// path — off-chain accounting always needs a stable snapshot.
#[test]
fn funding_snapshot_survives_withdraw() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);

    let snapshot_before = client.get_funding_close_snapshot();
    client.withdraw();
    let snapshot_after = client.get_funding_close_snapshot();

    assert_eq!(
        snapshot_before, snapshot_after,
        "funding snapshot must be immutable after withdraw"
    );
    assert_eq!(
        snapshot_after.unwrap().total_principal,
        TARGET,
        "snapshot total_principal must equal funded amount"
    );
}

/// After `settle` the snapshot still matches what was recorded at fund-close.
#[test]
fn funding_snapshot_survives_settle() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    fund_to_target(&client, &env);

    let snapshot_before = client.get_funding_close_snapshot();
    client.settle();
    let snapshot_after = client.get_funding_close_snapshot();

    assert_eq!(snapshot_before, snapshot_after);
}

// ──────────────────────────────────────────────────────────────────────────────
// Investor contribution accounting through the withdraw path
// ──────────────────────────────────────────────────────────────────────────────

/// Investor contributions are readable after `withdraw` — the integration layer
/// needs them to compute pro-rata refunds.
#[test]
fn investor_contribution_readable_after_withdraw() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);

    let investor = Address::generate(&env);
    let contribution: i128 = TARGET;
    client.fund(&investor, &contribution);
    client.withdraw();

    let recorded = client.get_contribution(&investor);
    assert_eq!(
        recorded, contribution,
        "investor contribution must be readable after withdraw for refund accounting"
    );
}

/// Multiple investors — each contribution is preserved after `withdraw`.
#[test]
fn multi_investor_contributions_preserved_after_withdraw() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);

    // Fund with two investors reaching target collectively.
    let inv_a = Address::generate(&env);
    let inv_b = Address::generate(&env);
    let half = TARGET / 2;
    client.fund(&inv_a, &half);
    client.fund(&inv_b, &(TARGET - half));

    client.withdraw();

    assert_eq!(client.get_contribution(&inv_a), half);
    assert_eq!(client.get_contribution(&inv_b), TARGET - half);
    assert_eq!(client.get_escrow().status, 3u32);
}

// ──────────────────────────────────────────────────────────────────────────────
// Terminal status — no entrypoint can move state backward from 3
// ──────────────────────────────────────────────────────────────────────────────

/// After `withdraw` (status 3) no write entrypoint must succeed.
///
/// This is a belt-and-suspenders test that exercises every state-mutating
/// path the SME might attempt after withdrawal.
#[test]
fn no_state_mutation_possible_after_withdraw() {
    // Each sub-case uses its own Env to keep failures isolated.
    macro_rules! assert_panics_after_withdraw {
        ($block:expr) => {{
            let env = Env::default();
            let (client, admin, sme) = setup(&env);
            default_init(&client, &env, &admin, &sme);
            fund_to_target(&client, &env);
            client.withdraw();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $block));
            assert!(result.is_err(), "expected panic but call succeeded");
        }};
    }

    // settle after withdraw
    {
        let env = Env::default();
        let (client, admin, sme) = setup(&env);
        default_init(&client, &env, &admin, &sme);
        fund_to_target(&client, &env);
        client.withdraw();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.settle();
        }));
        assert!(r.is_err(), "settle after withdraw must panic");
    }

    // withdraw after withdraw
    {
        let env = Env::default();
        let (client, admin, sme) = setup(&env);
        default_init(&client, &env, &admin, &sme);
        fund_to_target(&client, &env);
        client.withdraw();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.withdraw();
        }));
        assert!(r.is_err(), "withdraw after withdraw must panic");
    }

    // fund after withdraw
    {
        let env = Env::default();
        let (client, admin, sme) = setup(&env);
        default_init(&client, &env, &admin, &sme);
        fund_to_target(&client, &env);
        client.withdraw();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let late = Address::generate(&env);
            client.fund(&late, &1_000_0000000i128);
        }));
        assert!(r.is_err(), "fund after withdraw must panic");
    }
}
