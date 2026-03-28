use super::{LiquifactEscrow, LiquifactEscrowClient, MAX_DUST_SWEEP_AMOUNT, SCHEMA_VERSION};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    token::StellarAssetClient,
    Address, Env, String,
};

fn deploy(env: &Env) -> LiquifactEscrowClient<'_> {
    let id = env.register(LiquifactEscrow, ());
    LiquifactEscrowClient::new(env, &id)
}

fn setup(env: &Env) -> (LiquifactEscrowClient<'_>, Address, Address) {
    env.mock_all_auths();
    let client = deploy(env);
    let admin = Address::generate(env);
    let sme = Address::generate(env);
    (client, admin, sme)
}

fn free_addresses(env: &Env) -> (Address, Address) {
    (Address::generate(env), Address::generate(env))
}

fn default_init(client: &LiquifactEscrowClient<'_>, env: &Env, admin: &Address, sme: &Address) {
    let (token, treasury) = free_addresses(env);
    client.init(
        admin,
        &String::from_str(env, "INV001"),
        sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
        &token,
        &None,
        &treasury,
    );
}

const TARGET: i128 = 10_000_0000000i128;

#[test]
fn test_init_stores_escrow() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let escrow = client.init(
        &admin,
        &String::from_str(&env, "INV001"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    assert_eq!(escrow.invoice_id, symbol_short!("INV001"));
    assert_eq!(escrow.admin, admin);
    assert_eq!(escrow.sme_address, sme);
    assert_eq!(escrow.amount, TARGET);
    assert_eq!(escrow.funding_target, TARGET);
    assert_eq!(escrow.funded_amount, 0);
    assert_eq!(escrow.yield_bps, 800);
    assert_eq!(escrow.maturity, 1000);
    assert_eq!(escrow.status, 0);
}

#[test]
fn test_init_stores_keyed_invoice_and_lists_it() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let escrow = client.init(
        &admin,
        &String::from_str(&env, "INV001"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    let got = client.get_escrow();
    assert_eq!(got, escrow);
}

#[test]
fn test_init_requires_admin_auth() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INVB"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    assert!(
        env.auths().iter().any(|(addr, _)| *addr == admin),
        "admin auth was not recorded for init"
    );
}

#[test]
fn test_init_unauthorized_panics() {
    let env = Env::default();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.init(
            &admin,
            &String::from_str(&env, "INV001"),
            &sme,
            &1_000i128,
            &800i64,
            &1000u64,
            &Address::generate(&env),
            &None,
            &Address::generate(&env),
        );
    }));
    assert!(result.is_err(), "Expected panic without auth");
}

#[test]
#[should_panic(expected = "Escrow already initialized")]
fn test_double_init_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    default_init(&client, &env, &admin, &sme);
}

#[test]
#[should_panic(expected = "Escrow not initialized")]
fn test_get_escrow_uninitialized_panics() {
    let env = Env::default();
    let client = deploy(&env);
    client.get_escrow();
}

// --- fund ---

#[test]
fn test_fund_and_settle() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INVMETA"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    let funded = client.fund(&investor, &TARGET);
    assert_eq!(funded.funded_amount, TARGET);
    assert_eq!(funded.status, 1);
    let settled = client.settle();
    assert_eq!(settled.status, 2);
}

#[test]
fn test_fund_partial_then_full() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV002"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    let partial = client.fund(&investor, &(TARGET / 2));
    assert_eq!(partial.status, 0);
    assert_eq!(partial.funded_amount, TARGET / 2);
    let full = client.fund(&investor, &(TARGET / 2));
    assert_eq!(full.status, 1);
    assert_eq!(full.funded_amount, TARGET);
}

#[test]
#[should_panic(expected = "Funding amount must be positive")]
fn test_fund_zero_amount_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    default_init(&client, &env, &admin, &sme);
    client.fund(&investor, &0i128);
}

#[test]
#[should_panic(expected = "Escrow not open for funding")]
fn test_fund_after_funded_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    default_init(&client, &env, &admin, &sme);
    client.fund(&investor, &TARGET);
    client.fund(&investor, &1i128);
}

#[test]
fn test_fund_requires_investor_auth() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    default_init(&client, &env, &admin, &sme);
    client.fund(&investor, &TARGET);
    assert!(
        env.auths().iter().any(|(addr, _)| *addr == investor),
        "investor auth was not recorded for fund"
    );
}

#[test]
fn test_single_investor_contribution_tracked() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV020"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &(3_000_0000000i128));
    let contribution = client.get_contribution(&investor);
    assert_eq!(contribution, 3_000_0000000i128);
}

#[test]
fn test_unknown_investor_contribution_is_zero() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    let stranger = Address::generate(&env);
    default_init(&client, &env, &admin, &sme);
    client.fund(&investor, &1_000i128);
    assert_eq!(client.get_contribution(&stranger), 0i128);
}

#[test]
fn test_repeated_funding_accumulates_contribution() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV021"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &(2_000_0000000i128));
    client.fund(&investor, &(3_000_0000000i128));
    assert_eq!(client.get_contribution(&investor), 5_000_0000000i128);
}

#[test]
fn test_multiple_investors_tracked_independently() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let inv_a = Address::generate(&env);
    let inv_b = Address::generate(&env);
    let inv_c = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV023"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&inv_a, &(2_000_0000000i128));
    client.fund(&inv_b, &(5_000_0000000i128));
    client.fund(&inv_c, &(3_000_0000000i128));
    assert_eq!(client.get_contribution(&inv_a), 2_000_0000000i128);
    assert_eq!(client.get_contribution(&inv_b), 5_000_0000000i128);
    assert_eq!(client.get_contribution(&inv_c), 3_000_0000000i128);
    let sum = client.get_contribution(&inv_a)
        + client.get_contribution(&inv_b)
        + client.get_contribution(&inv_c);
    assert_eq!(sum, client.get_escrow().funded_amount);
}

#[test]
fn test_contributions_sum_equals_funded_amount() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let inv_a = Address::generate(&env);
    let inv_b = Address::generate(&env);
    let inv_c = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV023b"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&inv_a, &(2_000_0000000i128));
    client.fund(&inv_b, &(5_000_0000000i128));
    client.fund(&inv_c, &(3_000_0000000i128));
    let sum = client.get_contribution(&inv_a)
        + client.get_contribution(&inv_b)
        + client.get_contribution(&inv_c);
    assert_eq!(sum, client.get_escrow().funded_amount);
}

// --- settle ---

#[test]
fn test_settle_after_full_funding() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV003"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &TARGET);
    let settled = client.settle();
    assert_eq!(settled.status, 2);
}

#[test]
#[should_panic(expected = "Escrow must be funded before settlement")]
fn test_settle_before_funded_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV011"),
        &sme,
        &1_000i128,
        &500i64,
        &2000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.settle();
}

#[test]
fn test_settle_requires_sme_auth() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV006"),
        &sme,
        &1_000i128,
        &500i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &1_000i128);
    client.settle();
    assert!(
        env.auths().iter().any(|(addr, _)| *addr == sme),
        "sme auth was not recorded for settle"
    );
}

#[test]
#[should_panic]
fn test_settle_unauthorized_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let client = deploy(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV008"),
        &sme,
        &1_000i128,
        &500i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &1_000i128);
    env.mock_auths(&[]);
    client.settle();
}

#[test]
#[should_panic(expected = "Escrow has not yet reached maturity")]
fn test_settle_before_maturity_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV032"),
        &sme,
        &1_000i128,
        &500i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &1_000i128);
    client.settle();
}

#[test]
fn test_settle_after_maturity_succeeds() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV033"),
        &sme,
        &1_000i128,
        &500i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &1_000i128);
    env.ledger().set_timestamp(1001);
    let settled = client.settle();
    assert_eq!(settled.status, 2);
}

#[test]
fn test_settle_at_exact_maturity_succeeds() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV034"),
        &sme,
        &1_000i128,
        &500i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &1_000i128);
    env.ledger().set_timestamp(1000);
    let settled = client.settle();
    assert_eq!(settled.status, 2);
}

#[test]
fn test_settle_with_zero_maturity_succeeds_immediately() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV035"),
        &sme,
        &1_000i128,
        &500i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &1_000i128);
    let settled = client.settle();
    assert_eq!(settled.status, 2);
}

#[test]
fn test_settle_at_timestamp_zero_before_maturity_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV036"),
        &sme,
        &1_000i128,
        &500i64,
        &500u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &1_000i128);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.settle();
    }));
    assert!(
        result.is_err(),
        "Expected panic when settling before maturity"
    );
}

// --- update_maturity ---

#[test]
fn test_update_maturity_success() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV006b"),
        &sme,
        &1_000i128,
        &500i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    let updated = client.update_maturity(&2000u64);
    assert_eq!(updated.maturity, 2000u64);
    assert_eq!(updated.status, 0);
}

#[test]
#[should_panic(expected = "Maturity can only be updated in Open state")]
fn test_update_maturity_wrong_state() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV007"),
        &sme,
        &1_000i128,
        &500i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &1_000i128);
    client.update_maturity(&2000u64);
}

#[test]
#[should_panic]
fn test_update_maturity_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let client = deploy(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV009"),
        &sme,
        &1_000i128,
        &500i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    env.mock_auths(&[]);
    client.update_maturity(&2000u64);
}

// --- transfer_admin ---

#[test]
fn test_transfer_admin_updates_admin() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let new_admin = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "T001"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    let updated = client.transfer_admin(&new_admin);
    assert_eq!(updated.admin, new_admin);
    assert_eq!(client.get_escrow().admin, new_admin);
}

#[test]
#[should_panic(expected = "New admin must differ from current admin")]
fn test_transfer_admin_same_address_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    client.init(
        &admin,
        &String::from_str(&env, "T002"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.transfer_admin(&admin);
}

#[test]
#[should_panic(expected = "Escrow not initialized")]
fn test_transfer_admin_uninitialized_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let new_admin = Address::generate(&env);
    client.transfer_admin(&new_admin);
}

// --- migrate ---

#[test]
#[should_panic(expected = "Already at current schema version")]
fn test_migrate_at_current_version_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    client.migrate(&SCHEMA_VERSION);
}

#[test]
#[should_panic(expected = "from_version does not match stored version")]
fn test_migrate_wrong_from_version_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    client.migrate(&99u32);
}

#[test]
#[should_panic(expected = "No migration path from version 0")]
fn test_migrate_from_zero_uninitialized_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    client.migrate(&0u32);
}

// --- SME collateral (record-only) ---

#[test]
fn test_record_collateral_stored_and_does_not_block_settle() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "COL001"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    let c = client.record_sme_collateral_commitment(&symbol_short!("USDC"), &5000i128);
    assert_eq!(c.amount, 5000i128);
    assert_eq!(c.asset, symbol_short!("USDC"));
    assert_eq!(client.get_sme_collateral_commitment(), Some(c));

    client.fund(&investor, &TARGET);
    let settled = client.settle();
    assert_eq!(settled.status, 2);
}

#[test]
#[should_panic(expected = "Collateral amount must be positive")]
fn test_collateral_zero_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    client.init(
        &admin,
        &String::from_str(&env, "COL002"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.record_sme_collateral_commitment(&symbol_short!("XLM"), &0i128);
}

#[test]
#[should_panic]
fn test_collateral_requires_sme_auth() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    client.init(
        &admin,
        &String::from_str(&env, "COL003"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    env.mock_auths(&[]);
    client.record_sme_collateral_commitment(&symbol_short!("XLM"), &100i128);
}

// --- legal hold ---

#[test]
fn test_legal_hold_blocks_settle_withdraw_claim_and_fund() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "LH001"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &TARGET);
    client.set_legal_hold(&true);
    assert!(client.get_legal_hold());

    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.settle();
    }))
    .is_err());

    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.withdraw();
    }))
    .is_err());

    client.clear_legal_hold();
    assert!(!client.get_legal_hold());
    let settled = client.settle();
    assert_eq!(settled.status, 2);

    client.set_legal_hold(&true);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.claim_investor_payout(&investor);
    }))
    .is_err());

    client.clear_legal_hold();
    client.claim_investor_payout(&investor);
    assert!(client.is_investor_claimed(&investor));
}

#[test]
#[should_panic(expected = "Legal hold blocks new funding while active")]
fn test_legal_hold_blocks_new_funds_when_open() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "LH002"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.set_legal_hold(&true);
    client.fund(&investor, &1i128);
}

#[test]
fn test_withdraw_funded_then_cannot_settle() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "WD001"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &TARGET);
    let wd = client.withdraw();
    assert_eq!(wd.status, 3);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.settle();
    }))
    .is_err());
}

#[test]
#[should_panic(expected = "Investor already claimed")]
fn test_claim_investor_twice_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "CL001"),
        &sme,
        &1_000i128,
        &400i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &1_000i128);
    client.settle();
    client.claim_investor_payout(&investor);
    client.claim_investor_payout(&investor);
}

#[test]
#[should_panic(expected = "Escrow must be settled before investor claim")]
fn test_claim_before_settle_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "CL002"),
        &sme,
        &1_000i128,
        &400i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &1_000i128);
    client.claim_investor_payout(&investor);
}

// --- cost baselines ---

#[test]
fn test_cost_baseline_init() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV100"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
}

#[test]
fn test_cost_baseline_init_zero_maturity() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV101"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
}

#[test]
fn test_cost_baseline_init_max_amount() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV102"),
        &sme,
        &i128::MAX,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
}

#[test]
fn test_cost_baseline_fund_partial() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV103"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &(1_000_0000000i128));
}

#[test]
fn test_cost_baseline_fund_full() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV104"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &TARGET);
}

#[test]
fn test_cost_baseline_fund_overshoot() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV105"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &(15_000_0000000i128));
    assert_eq!(client.get_escrow().status, 1);
}

#[test]
fn test_cost_baseline_fund_two_step_completion() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV106"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &(TARGET / 2));
    client.fund(&investor, &(TARGET / 2));
    assert_eq!(client.get_escrow().status, 1);
}

#[test]
fn test_cost_baseline_settle() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV103b"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &TARGET);
    env.ledger().set_timestamp(1001);
    let settled = client.settle();
    assert_eq!(settled.status, 2);
}

#[test]
fn test_cost_baseline_full_lifecycle() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV110"),
        &sme,
        &TARGET,
        &800i64,
        &1000u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
    );
    client.fund(&investor, &TARGET);
    env.ledger().set_timestamp(1000);
    let settled = client.settle();
    assert_eq!(settled.status, 2);
}

// --- invoice id validation (#118) ---

#[test]
#[should_panic(expected = "invoice_id length")]
fn test_init_invoice_id_empty_string_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (t, tr) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, ""),
        &sme,
        &1000i128,
        &500i64,
        &0u64,
        &t,
        &None,
        &tr,
    );
}

#[test]
#[should_panic(expected = "invoice_id must be [A-Za-z0-9_]")]
fn test_init_invoice_id_whitespace_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (t, tr) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV BAD"),
        &sme,
        &1000i128,
        &500i64,
        &0u64,
        &t,
        &None,
        &tr,
    );
}

#[test]
#[should_panic(expected = "invoice_id length")]
fn test_init_invoice_id_too_long_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (t, tr) = free_addresses(&env);
    // 33 bytes — exceeds Soroban Symbol / our max of 32.
    let thirty_three = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456";
    client.init(
        &admin,
        &String::from_str(&env, thirty_three),
        &sme,
        &1000i128,
        &500i64,
        &0u64,
        &t,
        &None,
        &tr,
    );
}

#[test]
#[should_panic(expected = "invoice_id must be [A-Za-z0-9_]")]
fn test_init_invoice_id_bad_charset_hyphen_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (t, tr) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "INV-DASH"),
        &sme,
        &1000i128,
        &500i64,
        &0u64,
        &t,
        &None,
        &tr,
    );
}

// --- registry & funding token getters (#113, #116) ---

#[test]
fn test_init_stores_registry_some_and_getters() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let reg = Address::generate(&env);
    let token = Address::generate(&env);
    let treasury = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "REG001"),
        &sme,
        &5000i128,
        &100i64,
        &0u64,
        &token,
        &Some(reg.clone()),
        &treasury,
    );
    assert_eq!(client.get_registry_ref(), Some(reg));
    assert_eq!(client.get_funding_token(), token);
    assert_eq!(client.get_treasury(), treasury);
}

#[test]
fn test_init_registry_none_roundtrip() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let token = Address::generate(&env);
    let treasury = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "REG002"),
        &sme,
        &5000i128,
        &100i64,
        &0u64,
        &token,
        &None,
        &treasury,
    );
    assert_eq!(client.get_registry_ref(), None);
}

// --- treasury dust sweep (#107) ---

#[test]
fn test_sweep_terminal_dust_after_settle_transfers_to_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = sac.address();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);
    let escrow_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &escrow_id);
    client.init(
        &admin,
        &String::from_str(&env, "SW001"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &token,
        &None,
        &treasury,
    );
    let investor = Address::generate(&env);
    client.fund(&investor, &1_000i128);
    client.settle();

    let stellar = StellarAssetClient::new(&env, &token);
    stellar.mint(&escrow_id, &5_000i128);
    let before_t = stellar.balance(&treasury);
    let swept = client.sweep_terminal_dust(&5_000i128);
    assert_eq!(swept, 5_000i128);
    assert_eq!(stellar.balance(&treasury), before_t + 5_000i128);
}

#[test]
fn test_sweep_terminal_dust_after_withdraw_and_ledger_tick() {
    let env = Env::default();
    env.mock_all_auths();
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = sac.address();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);
    let escrow_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &escrow_id);
    client.init(
        &admin,
        &String::from_str(&env, "SW002"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &token,
        &None,
        &treasury,
    );
    let investor = Address::generate(&env);
    client.fund(&investor, &1_000i128);
    client.withdraw();

    env.ledger()
        .set_sequence_number(env.ledger().sequence() + 10);

    let stellar = StellarAssetClient::new(&env, &token);
    stellar.mint(&escrow_id, &333i128);
    let swept = client.sweep_terminal_dust(&333i128);
    assert_eq!(swept, 333i128);
}

#[test]
#[should_panic(expected = "dust sweep only in terminal states")]
fn test_sweep_rejected_when_open() {
    let env = Env::default();
    env.mock_all_auths();
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = sac.address();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);
    let escrow_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &escrow_id);
    client.init(
        &admin,
        &String::from_str(&env, "SW003"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &token,
        &None,
        &treasury,
    );
    let stellar = StellarAssetClient::new(&env, &token);
    stellar.mint(&escrow_id, &100i128);
    client.sweep_terminal_dust(&100i128);
}

#[test]
#[should_panic(expected = "Legal hold blocks treasury dust sweep")]
fn test_sweep_blocked_under_legal_hold() {
    let env = Env::default();
    env.mock_all_auths();
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = sac.address();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);
    let escrow_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &escrow_id);
    client.init(
        &admin,
        &String::from_str(&env, "SW004"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &token,
        &None,
        &treasury,
    );
    let investor = Address::generate(&env);
    client.fund(&investor, &1_000i128);
    client.settle();
    client.set_legal_hold(&true);
    client.sweep_terminal_dust(&1i128);
}

#[test]
#[should_panic(expected = "sweep amount exceeds MAX_DUST_SWEEP_AMOUNT")]
fn test_sweep_rejects_amount_above_dust_cap() {
    let env = Env::default();
    env.mock_all_auths();
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = sac.address();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);
    let escrow_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &escrow_id);
    client.init(
        &admin,
        &String::from_str(&env, "SW005"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &token,
        &None,
        &treasury,
    );
    let investor = Address::generate(&env);
    client.fund(&investor, &1_000i128);
    client.settle();
    client.sweep_terminal_dust(&(MAX_DUST_SWEEP_AMOUNT + 1));
}

#[test]
fn test_sweep_caps_at_contract_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = sac.address();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);
    let escrow_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &escrow_id);
    client.init(
        &admin,
        &String::from_str(&env, "SW006"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &token,
        &None,
        &treasury,
    );
    let investor = Address::generate(&env);
    client.fund(&investor, &1_000i128);
    client.settle();

    let stellar = StellarAssetClient::new(&env, &token);
    stellar.mint(&escrow_id, &50i128);
    let swept = client.sweep_terminal_dust(&100i128);
    assert_eq!(swept, 50i128);
}

#[test]
fn test_sweep_requires_treasury_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = sac.address();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);
    let escrow_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &escrow_id);
    client.init(
        &admin,
        &String::from_str(&env, "SW007"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &token,
        &None,
        &treasury,
    );
    let investor = Address::generate(&env);
    client.fund(&investor, &1_000i128);
    client.settle();
    let stellar = StellarAssetClient::new(&env, &token);
    stellar.mint(&escrow_id, &10i128);

    env.mock_auths(&[]);
    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.sweep_terminal_dust(&10i128);
    }));
    assert!(err.is_err(), "sweep without treasury auth must fail");
}

// --- property-based tests ---

use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_funded_amount_non_decreasing(
        amount1 in 1i128..5_000_0000000i128,
        amount2 in 1i128..5_000_0000000i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let sme = Address::generate(&env);
        let investor1 = Address::generate(&env);
        let investor2 = Address::generate(&env);
        let client = deploy(&env);

        let target = 20_000_0000000i128;
        client.init(
            &admin,
            &String::from_str(&env, "INVTST"),
            &sme,
            &target,
            &800i64,
            &0u64,
            &Address::generate(&env),
            &None,
            &Address::generate(&env),
        );

        let before = client.get_escrow().funded_amount;
        client.fund(&investor1, &amount1);
        let after1 = client.get_escrow().funded_amount;
        prop_assert!(after1 >= before, "funded_amount must be non-decreasing");

        if client.get_escrow().status == 0 {
            client.fund(&investor2, &amount2);
            let after2 = client.get_escrow().funded_amount;
            prop_assert!(after2 >= after1, "funded_amount must be non-decreasing on successive funds");
        }
    }

    #[test]
    fn prop_status_only_increases(
        amount in 1i128..10_000_0000000i128,
        target in 1i128..10_000_0000000i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let sme = Address::generate(&env);
        let investor = Address::generate(&env);
        let client = deploy(&env);

        let escrow = client.init(
            &admin,
            &String::from_str(&env, "INVSTA"),
            &sme,
            &target,
            &800i64,
            &0u64,
            &Address::generate(&env),
            &None,
            &Address::generate(&env),
        );
        prop_assert_eq!(escrow.status, 0);

        let after_fund = client.fund(&investor, &amount);
        prop_assert!(after_fund.status >= escrow.status, "status must not decrease");
        prop_assert!(after_fund.status <= 3, "status must be in valid range");

        if amount >= target {
            prop_assert_eq!(after_fund.status, 1);
            let after_settle = client.settle();
            prop_assert_eq!(after_settle.status, 2);
        } else {
            prop_assert_eq!(after_fund.status, 0);
        }
    }
}
