use super::{LiquifactEscrow, LiquifactEscrowClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

#[test]
fn test_init_and_get_escrow() {
    let env = Env::default();
    env.mock_all_auths();

    let sme = Address::generate(&env);
    let admin = Address::generate(&env);
    let contract_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &contract_id);

    let escrow = client.init(
        &symbol_short!("INV001"),
        &sme,
        &admin,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    assert_eq!(escrow.invoice_id, symbol_short!("INV001"));
    assert_eq!(escrow.admin, admin);
    assert_eq!(escrow.amount, 10_000_0000000i128);
    assert_eq!(escrow.funded_amount, 0);
    assert_eq!(escrow.status, 0);

    let got = client.get_escrow();
    assert_eq!(got.invoice_id, escrow.invoice_id);
}

#[test]
fn test_fund_and_settle() {
    let env = Env::default();
    env.mock_all_auths();

    let sme = Address::generate(&env);
    let admin = Address::generate(&env);
    let investor = Address::generate(&env);
    let contract_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &contract_id);

    client.init(
        &symbol_short!("INV002"),
        &sme,
        &admin,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    let escrow1 = client.fund(&investor, &10_000_0000000i128);
    assert_eq!(escrow1.funded_amount, 10_000_0000000i128);
    assert_eq!(escrow1.status, 1);

    let interest = (10_000_0000000i128 * 800) / 10000;
    let total_due = 10_000_0000000i128 + interest;

    let escrow2 = client.settle(&total_due);
    assert_eq!(escrow2.status, 2);
}

#[test]
fn test_partial_settlement_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let sme = Address::generate(&env);
    let admin = Address::generate(&env);
    let investor = Address::generate(&env);
    let contract_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &contract_id);

    client.init(
        &symbol_short!("INV_P1"),
        &sme,
        &admin,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    client.fund(&investor, &10_000_0000000i128);

    let interest = (10_000_0000000i128 * 800) / 10000;
    let total_due = 10_000_0000000i128 + interest; // 10,800,000,000

    // First partial: 5,000,000,000
    let e1 = client.settle(&5_000_0000000i128);
    assert_eq!(e1.settled_amount, 5_000_0000000i128);
    assert_eq!(e1.status, 1);

    // Second partial: 5,000,000,000
    let e2 = client.settle(&5_000_0000000i128);
    assert_eq!(e2.settled_amount, 10_000_0000000i128);
    assert_eq!(e2.status, 1);

    // Final settlement: 800,000,000
    let e3 = client.settle(&800_0000000i128);
    assert_eq!(e3.settled_amount, total_due);
    assert_eq!(e3.status, 2);
}

#[test]
#[should_panic(expected = "Settlement amount exceeds total due")]
fn test_over_settlement_failure() {
    let env = Env::default();
    env.mock_all_auths();

    let sme = Address::generate(&env);
    let admin = Address::generate(&env);
    let investor = Address::generate(&env);
    let contract_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &contract_id);

    client.init(
        &symbol_short!("INV_O1"),
        &sme,
        &admin,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    client.fund(&investor, &10_000_0000000i128);

    let interest = (10_000_0000000i128 * 800) / 10000;
    let total_due = 10_000_0000000i128 + interest;

    // Try to settle more than due
    client.settle(&(total_due + 1));
}

#[test]
#[should_panic(expected = "Escrow must be funded before settlement")]
fn test_settle_not_funded() {
    let env = Env::default();
    env.mock_all_auths();

    let sme = Address::generate(&env);
    let admin = Address::generate(&env);
    let contract_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &contract_id);

    client.init(
        &symbol_short!("INV_NF"),
        &sme,
        &admin,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    // Not funded, should panic
    client.settle(&1000i128);
}

#[test]
fn test_update_maturity_success() {
    let env = Env::default();
    env.mock_all_auths();

    let sme = Address::generate(&env);
    let admin = Address::generate(&env);
    let contract_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &contract_id);

    client.init(
        &symbol_short!("INV003"),
        &sme,
        &admin,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    let new_maturity = 2000u64;
    let escrow = client.update_maturity(&new_maturity);
    assert_eq!(escrow.maturity, new_maturity);

    // Verify state is still Open
    assert_eq!(escrow.status, 0);
}

#[test]
#[should_panic]
fn test_update_maturity_unauthorized() {
    let env = Env::default();
    // No mock_all_auths() here to manually set auths if needed, 
    // or use mock_all_auths and then try to call from a different address if the client allows it.
    // In Soroban tests, client.update_maturity() will use the address that registered the contract or default.
    // Actually, client calls in tests usually don't have a "caller" unless specified.
    
    let sme = Address::generate(&env);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let contract_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &contract_id);

    client.init(
        &symbol_short!("INV004"),
        &sme,
        &admin,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    // Attempt to call from attacker. 
    // In Soroban SDK tests, you can switch the address using set_auths or similar, 
    // but a simpler way is to use `env.as_contract(&attacker, || client.update_maturity(&2000))`
    // Wait, the client is bound to the contract, not the caller.
    
    env.as_contract(&attacker, || {
        client.update_maturity(&2000u64);
    });
}

#[test]
#[should_panic(expected = "Maturity can only be updated in Open state")]
fn test_update_maturity_wrong_state() {
    let env = Env::default();
    env.mock_all_auths();

    let sme = Address::generate(&env);
    let admin = Address::generate(&env);
    let investor = Address::generate(&env);
    let contract_id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(&env, &contract_id);

    client.init(
        &symbol_short!("INV005"),
        &sme,
        &admin,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    // Fund the escrow to change state to 1 (Funded)
    client.fund(&investor, &10_000_0000000i128);
    
    let escrow = client.get_escrow();
    assert_eq!(escrow.status, 1);

    // This should panic
    client.update_maturity(&2000u64);
}
