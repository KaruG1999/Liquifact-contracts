use super::{LiquifactEscrow, LiquifactEscrowClient, SCHEMA_VERSION};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

// ── helpers ──────────────────────────────────────────────────────────────────

fn deploy(env: &Env) -> LiquifactEscrowClient<'_> {
    let id = env.register(LiquifactEscrow, ());
    LiquifactEscrowClient::new(env, &id)
}

fn default_init(client: &LiquifactEscrowClient, sme: &Address) {
    client.init(
        &symbol_short!("INV001"),
        sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );
}

// ── init ─────────────────────────────────────────────────────────────────────

#[test]
fn test_init_sets_version() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let client = deploy(&env);

    let escrow = client.init(
        &symbol_short!("INV001"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    assert_eq!(escrow.version, SCHEMA_VERSION);
    assert_eq!(client.get_version(), SCHEMA_VERSION);
}

#[test]
fn test_init_and_get_escrow() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let client = deploy(&env);

    let escrow = client.init(
        &symbol_short!("INV001"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    assert_eq!(escrow.invoice_id, symbol_short!("INV001"));
    assert_eq!(escrow.amount, 10_000_0000000i128);
    assert_eq!(escrow.funded_amount, 0);
    assert_eq!(escrow.status, 0);

    let got = client.get_escrow();
    assert_eq!(got.invoice_id, escrow.invoice_id);
}

/// Re-initializing an already-initialized escrow must panic.
#[test]
#[should_panic(expected = "Escrow already initialized")]
fn test_reinit_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let client = deploy(&env);

    default_init(&client, &sme);
    // Second init on the same contract instance must be rejected.
    default_init(&client, &sme);
}

// ── fund & settle ─────────────────────────────────────────────────────────────

#[test]
fn test_fund_and_settle() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let client = deploy(&env);

    client.init(
        &symbol_short!("INV002"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    let escrow1 = client.fund(&investor, &10_000_0000000i128);
    assert_eq!(escrow1.funded_amount, 10_000_0000000i128);
    assert_eq!(escrow1.status, 1);

    let escrow2 = client.settle();
    assert_eq!(escrow2.status, 2);
}

/// Partial funding must not flip status to funded.
#[test]
fn test_partial_fund_stays_open() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let client = deploy(&env);

    client.init(
        &symbol_short!("INV003"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    let escrow = client.fund(&investor, &5_000_0000000i128);
    assert_eq!(escrow.status, 0, "Should still be open after partial fund");
    assert_eq!(escrow.funded_amount, 5_000_0000000i128);
}

/// Funding a closed (non-open) escrow must panic.
#[test]
#[should_panic(expected = "Escrow not open for funding")]
fn test_fund_after_funded_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let client = deploy(&env);

    client.init(
        &symbol_short!("INV004"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );
    client.fund(&investor, &10_000_0000000i128); // status -> 1
    client.fund(&investor, &1i128);              // must panic
}

/// Settling an open (not yet funded) escrow must panic.
#[test]
#[should_panic(expected = "Escrow must be funded before settlement")]
fn test_settle_before_funded_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let client = deploy(&env);

    default_init(&client, &sme);
    client.settle(); // must panic — status is still 0
}

// ── migration guards ──────────────────────────────────────────────────────────

/// Calling migrate when already at the current version must panic.
#[test]
#[should_panic(expected = "Already at current schema version")]
fn test_migrate_at_current_version_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let client = deploy(&env);

    default_init(&client, &sme);
    // SCHEMA_VERSION is 1; passing 1 as from_version should be rejected.
    client.migrate(&SCHEMA_VERSION);
}

/// Calling migrate with a mismatched from_version must panic.
#[test]
#[should_panic(expected = "from_version does not match stored version")]
fn test_migrate_wrong_from_version_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let client = deploy(&env);

    default_init(&client, &sme);
    // Stored version is 1; claiming it's 99 must be rejected.
    client.migrate(&99u32);
}
