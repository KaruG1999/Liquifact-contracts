use super::{LiquifactEscrow, LiquifactEscrowClient, MAX_INVESTORS_PER_ESCROW};
use soroban_sdk::{symbol_short, testutils::Address as _, xdr::ToXdr, Address, Env};

const DEFAULT_AMOUNT: i128 = 10_000_0000000;
const INVESTOR_MAP_XDR_SIZE_LIMIT_BYTES: u32 = 8_192;
const ESCROW_XDR_SIZE_LIMIT_BYTES: u32 = 9_216;
const FINAL_INSERT_WRITE_BYTES_LIMIT: u32 = 10_240;

fn setup_client(
    env: &Env,
    invoice_id: soroban_sdk::Symbol,
    amount: i128,
) -> LiquifactEscrowClient<'_> {
    env.mock_all_auths();
    let client = deploy(env);
    let admin = Address::generate(env);
    let sme = Address::generate(env);
    (client, admin, sme)
}

    let sme = Address::generate(env);
    let contract_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(env, &contract_id);
    client.init(&invoice_id, &sme, &amount, &800i64, &1000u64);
    client
}

#[test]
fn test_init_and_get_escrow() {
    let env = Env::default();
    let client = setup_client(&env, symbol_short!("INV001"), DEFAULT_AMOUNT);

    let escrow = client.get_escrow();
    assert_eq!(escrow.invoice_id, symbol_short!("INV001"));
    assert_eq!(escrow.amount, DEFAULT_AMOUNT);
    assert_eq!(escrow.funded_amount, 0);
    assert_eq!(escrow.yield_bps, 800);
    assert_eq!(escrow.maturity, 1000);
    assert_eq!(escrow.status, 0);
    assert_eq!(escrow.investor_contributions.len(), 0);
    assert_eq!(client.get_investor_count(), 0);
    assert_eq!(client.max_investors(), MAX_INVESTORS_PER_ESCROW);
}

// --- fund ---

#[test]
fn test_fund_tracks_investor_balances_and_settles() {
    let env = Env::default();
    let client = setup_client(&env, symbol_short!("INV002"), DEFAULT_AMOUNT);
    let investor = Address::generate(&env);

    let escrow1 = client.fund(&investor, &5_000_0000000i128);
    assert_eq!(escrow1.funded_amount, 5_000_0000000i128);
    assert_eq!(escrow1.status, 0);
    assert_eq!(escrow1.investor_contributions.len(), 1);
    assert_eq!(client.get_investor_count(), 1);
    assert_eq!(
        client.get_investor_contribution(&investor),
        5_000_0000000i128
    );

    let escrow2 = client.fund(&investor, &5_000_0000000i128);
    assert_eq!(escrow2.funded_amount, DEFAULT_AMOUNT);
    assert_eq!(escrow2.status, 1);
    assert_eq!(escrow2.investor_contributions.len(), 1);
    assert_eq!(client.get_investor_contribution(&investor), DEFAULT_AMOUNT);

    let escrow3 = client.settle();
    assert_eq!(escrow3.status, 2);
}

#[test]
#[should_panic(expected = "Funding amount must be positive")]
fn test_rejects_zero_amount_funding() {
    let env = Env::default();
    let client = setup_client(&env, symbol_short!("INV003"), DEFAULT_AMOUNT);
    let investor = Address::generate(&env);

    client.fund(&investor, &0i128);
}

#[test]
fn test_existing_investor_can_top_up_after_cardinality_cap() {
    let env = Env::default();
    let client = setup_client(
        &env,
        symbol_short!("INV004"),
        i128::from(MAX_INVESTORS_PER_ESCROW) + 5,
    );

    let first_investor = Address::generate(&env);
    client.fund(&first_investor, &1i128);

    for _ in 1..MAX_INVESTORS_PER_ESCROW {
        let investor = Address::generate(&env);
        client.fund(&investor, &1i128);
    }

    assert_eq!(client.get_investor_count(), MAX_INVESTORS_PER_ESCROW);

    let escrow = client.fund(&first_investor, &5i128);
    assert_eq!(
        escrow.investor_contributions.len(),
        MAX_INVESTORS_PER_ESCROW
    );
    assert_eq!(client.get_investor_contribution(&first_investor), 6i128);
}

#[test]
#[should_panic(expected = "Investor limit exceeded")]
fn test_rejects_new_investor_beyond_supported_cardinality() {
    let env = Env::default();
    let client = setup_client(
        &env,
        symbol_short!("INV005"),
        i128::from(MAX_INVESTORS_PER_ESCROW) + 1,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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
        &None,
        &None,
        &None,
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

// --- funding close snapshot (#117), tiered yield (#110), ledger boundaries (#106), external wrapper (#108) ---

#[test]
fn test_funding_close_snapshot_captures_overfunded_total_once() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "SNAP001"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    assert_eq!(client.get_funding_close_snapshot(), None);
    client.fund(&inv, &(TARGET + 5_000_0000000i128));
    let snap = client.get_funding_close_snapshot().expect("snapshot");
    assert_eq!(snap.total_principal, TARGET + 5_000_0000000i128);
    assert_eq!(snap.funding_target, TARGET);
    assert_eq!(snap.closed_at_ledger_timestamp, env.ledger().timestamp());
    assert_eq!(snap.closed_at_ledger_sequence, env.ledger().sequence());
}

#[test]
fn test_funding_snapshot_immutable_across_second_fund_after_funded() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "SNAP002"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    client.fund(&a, &(TARGET / 2));
    assert_eq!(client.get_funding_close_snapshot(), None);
    client.fund(&b, &(TARGET / 2));
    let s1 = client.get_funding_close_snapshot().unwrap();
    assert_eq!(s1.total_principal, TARGET);
    let s2 = client.get_funding_close_snapshot().unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn test_pro_rata_weight_ratio_from_snapshot() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "SNAP003"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    client.fund(&a, &(2_000_0000000i128));
    client.fund(&b, &(8_000_0000000i128));
    let snap = client.get_funding_close_snapshot().unwrap();
    assert_eq!(snap.total_principal, TARGET);
    let ca = client.get_contribution(&a);
    let cb = client.get_contribution(&b);
    assert_eq!(ca + cb, snap.total_principal);
}

#[test]
fn test_tiered_yield_and_follow_on_fund() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    let mut tiers = SorobanVec::new(&env);
    tiers.push_back(YieldTier {
        min_lock_secs: 100,
        yield_bps: 900,
    });
    tiers.push_back(YieldTier {
        min_lock_secs: 500,
        yield_bps: 1100,
    });
    client.init(
        &admin,
        &String::from_str(&env, "TIER001"),
        &sme,
        &10_000i128,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &Some(tiers),
        &None,
        &None,
    );
    client.fund_with_commitment(&inv, &5_000i128, &200u64);
    assert_eq!(client.get_investor_yield_bps(&inv), 900);
    assert_eq!(client.get_investor_claim_not_before(&inv), 200);
    client.fund(&inv, &5_000i128);
    assert_eq!(client.get_investor_yield_bps(&inv), 900);
    assert_eq!(client.get_escrow().status, 1);
}

#[test]
fn test_tier_selection_edges_base_vs_high_bucket() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let i_short = Address::generate(&env);
    let i_long = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    let mut tiers = SorobanVec::new(&env);
    tiers.push_back(YieldTier {
        min_lock_secs: 50,
        yield_bps: 850,
    });
    client.init(
        &admin,
        &String::from_str(&env, "TIER002"),
        &sme,
        &20_000i128,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &Some(tiers),
        &None,
        &None,
    );
    client.fund_with_commitment(&i_short, &10_000i128, &40u64);
    assert_eq!(client.get_investor_yield_bps(&i_short), 800);
    client.fund_with_commitment(&i_long, &10_000i128, &50u64);
    assert_eq!(client.get_investor_yield_bps(&i_long), 850);
}

#[test]
#[should_panic(expected = "Additional principal after a tiered first deposit")]
fn test_fund_with_commitment_twice_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    let mut tiers = SorobanVec::new(&env);
    tiers.push_back(YieldTier {
        min_lock_secs: 1,
        yield_bps: 810,
    });
    client.init(
        &admin,
        &String::from_str(&env, "TIER003"),
        &sme,
        &10_000i128,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &Some(tiers),
        &None,
        &None,
    );
    client.fund_with_commitment(&inv, &5_000i128, &10u64);
    client.fund_with_commitment(&inv, &5_000i128, &10u64);
}

#[test]
#[should_panic(expected = "Investor commitment lock not expired")]
fn test_claim_blocked_until_commitment_ledger_time() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "LOCK001"),
        &sme,
        &1_000i128,
        &400i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    client.fund_with_commitment(&inv, &1_000i128, &500u64);
    client.settle();
    client.claim_investor_payout(&inv);
}

#[test]
fn test_claim_succeeds_after_commitment_and_settle() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "LOCK002"),
        &sme,
        &1_000i128,
        &400i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    client.fund_with_commitment(&inv, &1_000i128, &100u64);
    client.settle();
    env.ledger().set_timestamp(150);
    client.claim_investor_payout(&inv);
    assert!(client.is_investor_claimed(&inv));
}

#[test]
#[should_panic(expected = "strictly increasing min_lock_secs")]
fn test_init_bad_tier_order_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    let mut tiers = SorobanVec::new(&env);
    tiers.push_back(YieldTier {
        min_lock_secs: 200,
        yield_bps: 900,
    });
    tiers.push_back(YieldTier {
        min_lock_secs: 100,
        yield_bps: 950,
    });
    client.init(
        &admin,
        &String::from_str(&env, "BADTIER"),
        &sme,
        &1_000i128,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &Some(tiers),
        &None,
        &None,
    );
}

#[test]
#[should_panic(expected = "tier yield_bps must be >= base yield_bps")]
fn test_init_tier_yield_below_base_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    let mut tiers = SorobanVec::new(&env);
    tiers.push_back(YieldTier {
        min_lock_secs: 10,
        yield_bps: 700,
    });
    client.init(
        &admin,
        &String::from_str(&env, "BADT2"),
        &sme,
        &1_000i128,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &Some(tiers),
        &None,
        &None,
    );
}

#[test]
fn test_external_transfer_wrapper_balance_deltas() {
    let env = Env::default();
    env.mock_all_auths();
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = sac.address();
    let holder = env.register(LiquifactEscrow, ());
    let treasury = Address::generate(&env);
    let stellar = StellarAssetClient::new(&env, &token);
    stellar.mint(&holder, &777i128);
    external_calls::transfer_funding_token_with_balance_checks(
        &env, &token, &holder, &treasury, 777i128,
    );
    assert_eq!(stellar.balance(&holder), 0);
    assert_eq!(stellar.balance(&treasury), 777i128);
}

#[test]
#[should_panic(expected = "insufficient token balance before transfer")]
fn test_external_wrapper_panics_when_undercollateralized() {
    let env = Env::default();
    env.mock_all_auths();
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token = sac.address();
    let holder = env.register(LiquifactEscrow, ());
    let treasury = Address::generate(&env);
    let stellar = StellarAssetClient::new(&env, &token);
    stellar.mint(&holder, &1i128);
    external_calls::transfer_funding_token_with_balance_checks(
        &env, &token, &holder, &treasury, 10i128,
    );
}

/// Ledger time trust model (#106): all gates compare [`Env::ledger`] timestamp (and sequence only in
/// snapshot metadata). There is no wall-clock oracle; **maturity**, **investor claim locks**, and
/// **funding snapshot metadata** are all ledger-observed values—test “skew” as one-ledger boundaries.
#[test]
fn test_differential_settle_maturity_minus_one_vs_exact() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    const M: u64 = 10_000;
    client.init(
        &admin,
        &String::from_str(&env, "DIFF001"),
        &sme,
        &1_000i128,
        &300i64,
        &M,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    client.fund(&inv, &1_000i128);
    env.ledger().set_timestamp(M - 1);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.settle();
    }))
    .is_err());
    env.ledger().set_timestamp(M);
    let settled = client.settle();
    assert_eq!(settled.status, 2);
}

#[test]
fn test_differential_funding_target_eq_exact_cross() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    let t = 5_000i128;
    client.init(
        &admin,
        &String::from_str(&env, "DIFF002"),
        &sme,
        &t,
        &100i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    let escrow = client.fund(&inv, &t);
    assert_eq!(escrow.funded_amount, t);
    assert_eq!(escrow.status, 1);
    let snap = client.get_funding_close_snapshot().unwrap();
    assert_eq!(snap.total_principal, t);
    assert_eq!(snap.funding_target, t);
}

#[test]
fn test_ledger_sequence_recorded_in_snapshot_with_tick() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "DIFF003"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    let seq = env.ledger().sequence();
    client.fund(&inv, &1_000i128);
    let snap = client.get_funding_close_snapshot().unwrap();
    assert_eq!(snap.closed_at_ledger_sequence, seq);
}

// --- attestation hash (#103), min contribution (#104), max investors (#105) ---

fn sample_digest(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

#[test]
fn test_bind_primary_attestation_single_set_and_get() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "ATT001"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    assert_eq!(client.get_primary_attestation_hash(), None);
    let d = sample_digest(&env, 3);
    client.bind_primary_attestation_hash(&d);
    assert_eq!(client.get_primary_attestation_hash(), Some(d));
    let log = client.get_attestation_append_log();
    assert_eq!(log.len(), 0);
}

#[test]
#[should_panic(expected = "primary attestation already bound")]
fn test_bind_primary_attestation_twice_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "ATT002"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    let d = sample_digest(&env, 9);
    client.bind_primary_attestation_hash(&d);
    client.bind_primary_attestation_hash(&sample_digest(&env, 8));
}

#[test]
fn test_append_attestation_digest_log_and_primary_coexist() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "ATT003"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    let p = sample_digest(&env, 1);
    client.bind_primary_attestation_hash(&p);
    let a = sample_digest(&env, 2);
    client.append_attestation_digest(&a);
    let log = client.get_attestation_append_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log.get(0).unwrap(), a);
}

#[test]
#[should_panic]
fn test_bind_attestation_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "ATT004"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    env.mock_auths(&[]);
    client.bind_primary_attestation_hash(&sample_digest(&env, 5));
}

#[test]
fn test_min_contribution_floor_rejects_below_and_accepts_equal() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    let floor = 500i128;
    let target = 2_000i128;
    client.init(
        &admin,
        &String::from_str(&env, "MIN001"),
        &sme,
        &target,
        &100i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &Some(floor),
        &None,
    );
    assert_eq!(client.get_min_contribution_floor(), floor);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.fund(&inv, &400i128);
    }))
    .is_err());
    client.fund(&inv, &floor);
    client.fund(&inv, &(target - floor));
    assert_eq!(client.get_escrow().status, 1);
}

#[test]
#[should_panic(expected = "funding amount below min_contribution floor")]
fn test_min_floor_applies_to_follow_on_fund() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "MIN002"),
        &sme,
        &10_000i128,
        &100i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &Some(1_000i128),
        &None,
    );
    client.fund(&inv, &3_000i128);
    client.fund(&inv, &500i128);
}

#[test]
#[should_panic(expected = "min_contribution must be positive when configured")]
fn test_init_min_contribution_zero_some_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "MINBAD"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &Some(0i128),
        &None,
    );
}

#[test]
#[should_panic(expected = "min_contribution cannot exceed initial invoice amount")]
fn test_init_min_contribution_above_amount_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "MINBAD2"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &Some(2_000i128),
        &None,
    );
}

#[test]
fn test_max_unique_investors_cap_enforced() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let i1 = Address::generate(&env);
    let i2 = Address::generate(&env);
    let i3 = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "CAP001"),
        &sme,
        &10_000i128,
        &100i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &Some(2u32),
    );
    assert_eq!(client.get_max_unique_investors_cap(), Some(2u32));
    client.fund(&i1, &3_000i128);
    assert_eq!(client.get_unique_funder_count(), 1u32);
    client.fund(&i2, &3_000i128);
    assert_eq!(client.get_unique_funder_count(), 2u32);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.fund(&i3, &1i128);
    }))
    .is_err());
    client.fund(&i1, &4_000i128);
    assert_eq!(client.get_unique_funder_count(), 2u32);
    assert_eq!(client.get_escrow().funded_amount, 10_000i128);
}

#[test]
#[should_panic(expected = "max_unique_investors must be positive when configured")]
fn test_init_max_unique_zero_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "CAPBAD"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &Some(0u32),
    );
}

#[test]
fn test_append_attestation_respects_max_length() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "ATTMAX"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
        &None,
        &None,
    );
    for i in 0..super::MAX_ATTESTATION_APPEND_ENTRIES {
        client.append_attestation_digest(&sample_digest(&env, i as u8));
    }
    assert_eq!(
        client.get_attestation_append_log().len(),
        super::MAX_ATTESTATION_APPEND_ENTRIES
    );
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.append_attestation_digest(&sample_digest(&env, 99));
    }))
    .is_err());
}

// --- property-based tests ---

use proptest::prelude::*;

// --- Deterministic Yield Calculation Tests ---

#[test]
fn test_calculate_principal_plus_yield_zero_yield() {
    // Edge case: 0 bps yield should return principal unchanged
    let result = LiquifactEscrow::calculate_principal_plus_yield(1000i128, 0i64);
    assert_eq!(result, 1000i128);
}

#[test]
fn test_calculate_principal_plus_yield_hundred_percent() {
    // Edge case: 10_000 bps (100%) should double the principal
    let result = LiquifactEscrow::calculate_principal_plus_yield(5000i128, 10_000i64);
    assert_eq!(result, 10_000i128);
}

#[test]
fn test_calculate_principal_plus_yield_typical_case() {
    // Typical case: 800 bps (8%) on 10,000 = 10,000 + 800 = 10,800
    let result = LiquifactEscrow::calculate_principal_plus_yield(10_000i128, 800i64);
    assert_eq!(result, 10_800i128);
}

#[test]
fn test_calculate_principal_plus_yield_rounding_down() {
    // Rounding test: 100 at 1 bps = 0.1, rounds down to 0
    let result = LiquifactEscrow::calculate_principal_plus_yield(100i128, 1i64);
    assert_eq!(result, 100i128);

    // 1000 at 1 bps = 0.1, rounds down to 0
    let result = LiquifactEscrow::calculate_principal_plus_yield(1000i128, 1i64);
    assert_eq!(result, 1000i128);

    // 10000 at 1 bps = 1.0, exact
    let result = LiquifactEscrow::calculate_principal_plus_yield(10_000i128, 1i64);
    assert_eq!(result, 10_001i128);
}

#[test]
fn test_calculate_principal_plus_yield_large_amounts() {
    // Large amount: 1M at 500 bps (5%) = 1M + 50k = 1.05M
    let result = LiquifactEscrow::calculate_principal_plus_yield(1_000_000i128, 500i64);
    assert_eq!(result, 1_050_000i128);

    // Very large amount within i128 range
    let principal = 1_000_000_000_000i128; // 1 trillion
    let yield_bps = 100i64; // 1%
    let expected = principal + (principal / 100); // 1% yield
    let result = LiquifactEscrow::calculate_principal_plus_yield(principal, yield_bps);
    assert_eq!(result, expected);
}

#[test]
fn test_calculate_principal_plus_yield_fractional_bps() {
    // Test various basis point percentages
    let principal = 100_000i128;

    // 100 bps = 1%
    let result = LiquifactEscrow::calculate_principal_plus_yield(principal, 100i64);
    assert_eq!(result, 101_000i128);

    // 250 bps = 2.5%
    let result = LiquifactEscrow::calculate_principal_plus_yield(principal, 250i64);
    assert_eq!(result, 102_500i128);

    // 750 bps = 7.5%
    let result = LiquifactEscrow::calculate_principal_plus_yield(principal, 750i64);
    assert_eq!(result, 107_500i128);
}

#[test]
fn test_calculate_principal_plus_yield_zero_principal() {
    // Edge case: zero principal should return zero
    let result = LiquifactEscrow::calculate_principal_plus_yield(0i128, 800i64);
    assert_eq!(result, 0i128);
}

#[test]
#[should_panic(expected = "principal must be non-negative")]
fn test_calculate_principal_plus_yield_negative_principal_panics() {
    // Should panic with negative principal
    LiquifactEscrow::calculate_principal_plus_yield(-1i128, 800i64);
}

#[test]
#[should_panic(expected = "yield_bps must be between 0 and BPS_DENOMINATOR")]
fn test_calculate_principal_plus_yield_negative_bps_panics() {
    // Should panic with negative bps
    LiquifactEscrow::calculate_principal_plus_yield(1000i128, -1i64);
}

#[test]
#[should_panic(expected = "yield_bps must be between 0 and BPS_DENOMINATOR")]
fn test_calculate_principal_plus_yield_excessive_bps_panics() {
    // Should panic with bps > 10_000
    LiquifactEscrow::calculate_principal_plus_yield(1000i128, 10_001i64);
}

#[test]
fn test_calculate_principal_plus_yield_boundary_values() {
    // Test at boundary: exactly 10_000 bps (100%)
    let result = LiquifactEscrow::calculate_principal_plus_yield(500i128, 10_000i64);
    assert_eq!(result, 1000i128);

    // Test at boundary: exactly 0 bps
    let result = LiquifactEscrow::calculate_principal_plus_yield(500i128, 0i64);
    assert_eq!(result, 500i128);
}

#[test]
fn test_calculate_principal_plus_yield_deterministic() {
    // Verify deterministic behavior: same inputs always produce same outputs
    for _ in 0..10 {
        let result1 = LiquifactEscrow::calculate_principal_plus_yield(12_345i128, 678i64);
        let result2 = LiquifactEscrow::calculate_principal_plus_yield(12_345i128, 678i64);
        assert_eq!(result1, result2);
    }
}

proptest! {
    #[test]
    fn prop_yield_calculation_monotonic(
        principal in 0i128..1_000_000_000i128,
        yield_bps_low in 0i64..5_000i64,
        delta in 1i64..5_000i64,
    ) {
        // Higher yield_bps should always produce higher or equal payout
        let yield_bps_high = yield_bps_low.saturating_add(delta).min(10_000);

        let payout_low = LiquifactEscrow::calculate_principal_plus_yield(principal, yield_bps_low);
        let payout_high = LiquifactEscrow::calculate_principal_plus_yield(principal, yield_bps_high);

        prop_assert!(payout_high >= payout_low, "higher yield_bps should produce higher or equal payout");
    }

    #[test]
    fn prop_yield_calculation_linear(
        principal in 1i128..1_000_000i128,
        yield_bps in 0i64..10_000i64,
        scalar in 1i128..100i128,
    ) {
        // Scaling principal by scalar should scale payout proportionally (accounting for rounding)
        let scaled_principal = principal.saturating_mul(scalar);
        if scaled_principal < 0 {
            // Skip on overflow
            return Ok(());
        }

        let payout_base = LiquifactEscrow::calculate_principal_plus_yield(principal, yield_bps);
        let payout_scaled = LiquifactEscrow::calculate_principal_plus_yield(scaled_principal, yield_bps);

        // Allow small rounding error due to integer division truncation
        let expected_scaled = payout_base.saturating_mul(scalar);
        let diff = if payout_scaled > expected_scaled {
            payout_scaled - expected_scaled
        } else {
            expected_scaled - payout_scaled
        };

        // Rounding error should be less than scalar (one per multiplication)
        prop_assert!(diff < scalar as i128, "scaling should be approximately linear");
    }

    #[test]
    fn prop_yield_calculation_never_reduces_principal(
        principal in 0i128..10_000_000i128,
        yield_bps in 0i64..10_000i64,
    ) {
        // Payout should always be >= principal (yield is non-negative)
        let payout = LiquifactEscrow::calculate_principal_plus_yield(principal, yield_bps);
        prop_assert!(payout >= principal, "payout should never be less than principal");
    }
}

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
            &None,
            &None,
            &None,
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
            &None,
            &None,
            &None,
        );
        prop_assert_eq!(escrow.status, 0);

    for _ in 0..MAX_INVESTORS_PER_ESCROW {
        let investor = Address::generate(&env);
        client.fund(&investor, &1i128);
    }

    let overflow_investor = Address::generate(&env);
    client.fund(&overflow_investor, &1i128);
}

#[test]
fn test_storage_growth_regression_at_investor_cap() {
    let env = Env::default();
    let client = setup_client(
        &env,
        symbol_short!("INV006"),
        i128::from(MAX_INVESTORS_PER_ESCROW),
    );

    for _ in 0..MAX_INVESTORS_PER_ESCROW {
        let investor = Address::generate(&env);
        client.fund(&investor, &1i128);
    }

    let resources = env.cost_estimate().resources();
    let escrow = client.get_escrow();
    let investor_map_xdr_len = escrow.investor_contributions.clone().to_xdr(&env).len();
    let escrow_xdr_len = escrow.clone().to_xdr(&env).len();

    assert_eq!(
        escrow.investor_contributions.len(),
        MAX_INVESTORS_PER_ESCROW
    );
    assert!(
        investor_map_xdr_len <= INVESTOR_MAP_XDR_SIZE_LIMIT_BYTES,
        "investor map XDR footprint regressed: {} > {} bytes",
        investor_map_xdr_len,
        INVESTOR_MAP_XDR_SIZE_LIMIT_BYTES
    );
    assert!(
        escrow_xdr_len <= ESCROW_XDR_SIZE_LIMIT_BYTES,
        "escrow entry XDR footprint regressed: {} > {} bytes",
        escrow_xdr_len,
        ESCROW_XDR_SIZE_LIMIT_BYTES
    );
    assert_eq!(resources.write_entries, 1);
    assert!(
        resources.write_bytes <= FINAL_INSERT_WRITE_BYTES_LIMIT,
        "final investor insert write footprint regressed: {} > {} bytes",
        resources.write_bytes,
        FINAL_INSERT_WRITE_BYTES_LIMIT
    );
}

// --- coverage gap tests ---

#[test]
#[should_panic(expected = "Funding token not set")]
fn test_get_funding_token_uninitialized_panics() {
    let env = Env::default();
    let client = deploy(&env);
    client.get_funding_token();
}

#[test]
#[should_panic(expected = "Treasury not set")]
fn test_get_treasury_uninitialized_panics() {
    let env = Env::default();
    let client = deploy(&env);
    client.get_treasury();
}

#[test]
fn test_fund_follow_on_does_not_reset_yield() {
    // Exercises the simple_fund=true branch where prev > 0 (yield already stored, skip re-set).
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    let mut tiers = SorobanVec::new(&env);
    tiers.push_back(YieldTier {
        min_lock_secs: 10,
        yield_bps: 950,
    });
    client.init(
        &admin,
        &String::from_str(&env, "FOLO001"),
        &sme,
        &20_000i128,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &Some(tiers),
    );
    // First deposit via fund_with_commitment — sets effective yield to 950.
    client.fund_with_commitment(&inv, &5_000i128, &10u64);
    assert_eq!(client.get_investor_yield_bps(&inv), 950);
    // Follow-on via fund — prev > 0, so the if-prev==0 block is skipped.
    client.fund(&inv, &5_000i128);
    // Yield must remain 950, not reset to base 800.
    assert_eq!(client.get_investor_yield_bps(&inv), 950);
}

#[test]
fn test_effective_yield_empty_tier_table_returns_base() {
    // Exercises the tiers.len() == 0 early-return in effective_yield_for_commitment.
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    // Pass an empty Vec — validate_yield_tiers_table returns early, YieldTierTable not stored.
    let empty: SorobanVec<YieldTier> = SorobanVec::new(&env);
    client.init(
        &admin,
        &String::from_str(&env, "EMPT001"),
        &sme,
        &1_000i128,
        &800i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &Some(empty),
    );
    // fund_with_commitment with a lock — no table stored, so base yield is returned.
    client.fund_with_commitment(&inv, &1_000i128, &100u64);
    assert_eq!(client.get_investor_yield_bps(&inv), 800);
}

#[test]
#[should_panic(expected = "no funding token balance to sweep")]
fn test_sweep_panics_when_balance_zero() {
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
        &String::from_str(&env, "SWZ001"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &token,
        &None,
        &treasury,
        &None,
    );
    let investor = Address::generate(&env);
    client.fund(&investor, &1_000i128);
    client.settle();
    // No mint — balance is zero, should panic.
    client.sweep_terminal_dust(&1i128);
}

#[test]
#[should_panic(expected = "sweep amount must be positive")]
fn test_sweep_zero_amount_panics() {
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
        &String::from_str(&env, "SWZ002"),
        &sme,
        &1_000i128,
        &100i64,
        &0u64,
        &token,
        &None,
        &treasury,
        &None,
    );
    let investor = Address::generate(&env);
    client.fund(&investor, &1_000i128);
    client.settle();
    client.sweep_terminal_dust(&0i128);
}

#[test]
fn test_get_version_returns_schema_version() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    default_init(&client, &env, &admin, &sme);
    assert_eq!(client.get_version(), SCHEMA_VERSION);
}

#[test]
fn test_get_version_uninitialized_returns_zero() {
    let env = Env::default();
    let client = deploy(&env);
    assert_eq!(client.get_version(), 0u32);
}

#[test]
fn test_withdraw_requires_sme_auth() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "WDAUTH01"),
        &sme,
        &1_000i128,
        &500i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
        &None,
    );
    client.fund(&investor, &1_000i128);
    client.withdraw();
    assert!(
        env.auths().iter().any(|(addr, _)| *addr == sme),
        "sme auth was not recorded for withdraw"
    );
}

#[test]
#[should_panic(expected = "Escrow must be funded before withdrawal")]
fn test_withdraw_before_funded_panics() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    client.init(
        &admin,
        &String::from_str(&env, "WDPANIC1"),
        &sme,
        &1_000i128,
        &500i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
        &None,
    );
    client.withdraw();
}

#[test]
#[should_panic(expected = "Legal hold blocks SME withdrawal")]
fn test_withdraw_blocked_under_legal_hold() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "WDHOLD01"),
        &sme,
        &1_000i128,
        &500i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
        &None,
    );
    client.fund(&investor, &1_000i128);
    client.set_legal_hold(&true);
    client.withdraw();
}

#[test]
#[should_panic(expected = "Legal hold blocks settlement finalization")]
fn test_settle_blocked_under_legal_hold() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "SDHOLD01"),
        &sme,
        &1_000i128,
        &500i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
        &None,
    );
    client.fund(&investor, &1_000i128);
    client.set_legal_hold(&true);
    client.settle();
}

#[test]
#[should_panic(expected = "Legal hold blocks investor claims")]
fn test_claim_blocked_under_legal_hold_explicit() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);
    let investor = Address::generate(&env);
    client.init(
        &admin,
        &String::from_str(&env, "CLHOLD01"),
        &sme,
        &1_000i128,
        &500i64,
        &0u64,
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
        &None,
    );
    client.fund(&investor, &1_000i128);
    client.settle();
    client.set_legal_hold(&true);
    client.claim_investor_payout(&investor);
}

#[test]
fn test_fund_with_commitment_zero_lock_sets_no_claim_gate() {
    // committed_lock_secs == 0 → claim_nb = 0, no time gate.
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let inv = Address::generate(&env);
    let (tok, tre) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "ZLOCK001"),
        &sme,
        &1_000i128,
        &400i64,
        &0u64,
        &tok,
        &None,
        &tre,
        &None,
    );
    client.fund_with_commitment(&inv, &1_000i128, &0u64);
    assert_eq!(client.get_investor_claim_not_before(&inv), 0u64);
    client.settle();
    client.claim_investor_payout(&inv);
    assert!(client.is_investor_claimed(&inv));
}
