use super::{LiquifactEscrow, LiquifactEscrowClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

fn deploy(env: &Env) -> LiquifactEscrowClient<'_> {
    let id = env.register(LiquifactEscrow, ());
    LiquifactEscrowClient::new(env, &id)
}

fn init_escrow(env: &Env, client: &LiquifactEscrowClient, admin: &Address, sme: &Address, amount: i128) {
    client.init(admin, &symbol_short!("INV001"), sme, &amount, &800u32, &3000u64, &5000u64);
}

// ---------------------------------------------------------------------------
// update_funding_target tests (Issue #49)
// ---------------------------------------------------------------------------

/// Admin can raise the funding target while the escrow is still open.
#[test]
fn test_update_funding_target_by_admin_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let client = deploy(&env);

    init_escrow(&env, &client, &admin, &sme, 5_000i128);
    let updated = client.update_funding_target(&10_000i128);
    assert_eq!(updated.funding_target, 10_000i128, "funding_target should be updated");
    assert_eq!(updated.status, 0, "status must remain Open after target update");
}

/// A caller other than admin must not be able to update the funding target.
#[test]
#[should_panic]
fn test_update_funding_target_by_non_admin_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let client = deploy(&env);
    init_escrow(&env, &client, &admin, &sme, 5_000i128);

    // New env without mocked auths — auth check fires
    let env2 = Env::default();
    let id = env.register(LiquifactEscrow, ());
    let client2 = LiquifactEscrowClient::new(&env2, &id);
    client2.update_funding_target(&10_000i128);
}

/// Funding target cannot be updated once the escrow reaches Funded state (status = 1).
#[test]
#[should_panic(expected = "Target can only be updated in Open state")]
fn test_update_funding_target_fails_when_funded() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let client = deploy(&env);

    init_escrow(&env, &client, &admin, &sme, 5_000i128);
    client.fund(&investor, &5_000i128); // status -> 1 (Funded)
    client.update_funding_target(&10_000i128); // must panic
}

/// Funding target cannot be lowered below the already funded amount.
#[test]
#[should_panic(expected = "Target cannot be less than already funded amount")]
fn test_update_funding_target_below_funded_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let client = deploy(&env);

    init_escrow(&env, &client, &admin, &sme, 10_000i128);
    client.fund(&investor, &4_000i128); // funded_amount = 4_000, still open
    client.update_funding_target(&3_000i128); // 3_000 < 4_000 — must panic
}

/// Funding target cannot be set to zero or negative.
#[test]
#[should_panic(expected = "Target must be strictly positive")]
fn test_update_funding_target_zero_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let client = deploy(&env);

    init_escrow(&env, &client, &admin, &sme, 5_000i128);
    client.update_funding_target(&0i128); // must panic
}
