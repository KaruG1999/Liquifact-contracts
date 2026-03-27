use super::{LiquifactEscrow, LiquifactEscrowClient, SCHEMA_VERSION};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

fn deploy(env: &Env) -> LiquifactEscrowClient<'_> {
    let id = env.register(LiquifactEscrow, ());
    LiquifactEscrowClient::new(env, &id)
}

#[test]
fn test_init_sets_version_and_is_idempotent_read() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let client = deploy(&env);

    let escrow = client.init(
        &admin,
        &symbol_short!("INV001"),
        &sme,
        &10_000i128,
        &800i64,
        &1000u64,
    );

    assert_eq!(escrow.version, SCHEMA_VERSION);
    assert_eq!(client.get_version(), SCHEMA_VERSION);

    let a = client.get_escrow();
    let b = client.get_escrow();
    assert_eq!(a, b);
}

#[test]
#[should_panic(expected = "Escrow already initialized")]
fn test_init_replay_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let client = deploy(&env);

    client.init(
        &admin,
        &symbol_short!("INV002"),
        &sme,
        &10_000i128,
        &800i64,
        &1000u64,
    );
    client.init(
        &admin,
        &symbol_short!("INV002"),
        &sme,
        &10_000i128,
        &800i64,
        &1000u64,
    );
}

#[test]
fn test_fund_replay_after_funded_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let client = deploy(&env);

    client.init(
        &admin,
        &symbol_short!("INV003"),
        &sme,
        &1_000i128,
        &800i64,
        &1000u64,
    );
    client.fund(&investor, &1_000i128);
    assert_eq!(client.get_escrow().status, 1);

    // Replay a second funding call after funded.
    // This must be rejected (non-idempotent state transition).
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.fund(&investor, &1i128)
    }));
    assert!(result.is_err());
}

#[test]
fn test_withdraw_replay_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let client = deploy(&env);

    client.init(
        &admin,
        &symbol_short!("INV004"),
        &sme,
        &1_000i128,
        &800i64,
        &1000u64,
    );
    client.fund(&investor, &1_000i128);

    let withdrawn = client.withdraw();
    assert_eq!(withdrawn, 1_000i128);

    // Replay withdraw. Must fail because status is now 3.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| client.withdraw()));
    assert!(result.is_err());
}

#[test]
fn test_settle_partial_then_full_and_replay_overpay_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let client = deploy(&env);

    client.init(
        &admin,
        &symbol_short!("INV005"),
        &sme,
        &1_000i128,
        &800i64,
        &1000u64,
    );
    client.fund(&investor, &1_000i128);

    let interest = (1_000i128 * 800i128) / 10_000i128;
    let total_due = 1_000i128 + interest;

    client.settle(&500i128);
    assert_eq!(client.get_escrow().status, 1);

    client.settle(&(total_due - 500i128));
    assert_eq!(client.get_escrow().status, 2);

    // Replay settle with a positive amount after fully settled should fail due to overpay.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| client.settle(&1i128)));
    assert!(result.is_err());
}

#[test]
#[should_panic]
fn test_settle_before_funded_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let client = deploy(&env);

    client.init(
        &admin,
        &symbol_short!("INV006"),
        &sme,
        &1_000i128,
        &800i64,
        &1000u64,
    );
    client.settle(&1i128);
}
