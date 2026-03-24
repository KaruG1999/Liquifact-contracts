use super::{FundEvent, InitEvent, LiquifactEscrow, LiquifactEscrowClient, SettleEvent};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events},
    vec, Address, Env, IntoVal, TryFromVal, Val,
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn deploy(env: &Env) -> (LiquifactEscrowClient<'_>, Address) {
    let id = env.register(LiquifactEscrow, ());
    (LiquifactEscrowClient::new(env, &id), id)
}

/// Extract the typed data payload from the Nth event (0-indexed).
fn event_data<T: TryFromVal<Env, Val>>(env: &Env, n: usize) -> T {
    let all = env.events().all();
    let xdr_event = &all.events()[n];
    let data_xdr = match &xdr_event.body {
        soroban_sdk::xdr::ContractEventBody::V0(v0) => v0.data.clone(),
    };
    let raw: Val = Val::try_from_val(env, &data_xdr).unwrap();
    T::try_from_val(env, &raw).unwrap()
}

/// Extract topic[0] (the action symbol) from the Nth event.
fn event_topic0(env: &Env, n: usize) -> soroban_sdk::Symbol {
    let all = env.events().all();
    let xdr_event = &all.events()[n];
    let topics = match &xdr_event.body {
        soroban_sdk::xdr::ContractEventBody::V0(v0) => &v0.topics,
    };
    let raw: Val = Val::try_from_val(env, &topics[0]).unwrap();
    soroban_sdk::Symbol::try_from_val(env, &raw).unwrap()
}

// ── existing behaviour ────────────────────────────────────────────────────────

#[test]
fn test_init_and_get_escrow() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let (client, _) = deploy(&env);

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

#[test]
fn test_fund_and_settle() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let (client, _) = deploy(&env);

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

// ── event: init ───────────────────────────────────────────────────────────────

#[test]
fn test_init_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let (client, contract_id) = deploy(&env);

    client.init(
        &symbol_short!("INV001"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );

    assert_eq!(env.events().all().events().len(), 1);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id,
                vec![
                    &env,
                    symbol_short!("init").into_val(&env),
                    symbol_short!("INV001").into_val(&env),
                ],
                InitEvent {
                    sme_address: sme.clone(),
                    amount: 10_000_0000000i128,
                    yield_bps: 800i64,
                    maturity: 1000u64,
                }
                .into_val(&env),
            )
        ]
    );
}

// ── event: fund (partial) ─────────────────────────────────────────────────────

#[test]
fn test_fund_partial_emits_event_status_open() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let (client, _) = deploy(&env);

    client.init(
        &symbol_short!("INV003"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );
    client.fund(&investor, &4_000_0000000i128);

    // events().all() captures only the most recent invocation's events
    assert_eq!(env.events().all().events().len(), 1);

    let payload: FundEvent = event_data(&env, 0);
    assert_eq!(payload.investor, investor);
    assert_eq!(payload.amount, 4_000_0000000i128);
    assert_eq!(payload.funded_amount, 4_000_0000000i128);
    assert_eq!(payload.status, 0); // still open
}

// ── event: fund (fully funded) ────────────────────────────────────────────────

#[test]
fn test_fund_full_emits_event_status_funded() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let (client, _) = deploy(&env);

    client.init(
        &symbol_short!("INV004"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );
    client.fund(&investor, &10_000_0000000i128);

    let payload: FundEvent = event_data(&env, 0);
    assert_eq!(payload.status, 1); // fully funded
    assert_eq!(payload.funded_amount, 10_000_0000000i128);
}

// ── event: settle ─────────────────────────────────────────────────────────────

#[test]
fn test_settle_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let (client, _) = deploy(&env);

    client.init(
        &symbol_short!("INV005"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );
    client.fund(&investor, &10_000_0000000i128);
    client.settle();

    // settle is the only event from the last invocation
    assert_eq!(env.events().all().events().len(), 1);

    let payload: SettleEvent = event_data(&env, 0);
    assert_eq!(payload.sme_address, sme);
    assert_eq!(payload.amount, 10_000_0000000i128);
    assert_eq!(payload.yield_bps, 800i64);
}

// ── event topic correctness ───────────────────────────────────────────────────

#[test]
fn test_event_topics_are_correct() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let (client, _) = deploy(&env);

    client.init(
        &symbol_short!("INV006"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );
    assert_eq!(event_topic0(&env, 0), symbol_short!("init"));

    client.fund(&investor, &10_000_0000000i128);
    assert_eq!(event_topic0(&env, 0), symbol_short!("fund"));

    client.settle();
    // each call emits exactly one event; check the last one (settle)
    assert_eq!(env.events().all().events().len(), 1);
    assert_eq!(event_topic0(&env, 0), symbol_short!("settle"));
}

// ── edge cases ────────────────────────────────────────────────────────────────

/// Two partial tranches emit two fund events with cumulative funded_amount.
#[test]
fn test_two_partial_funds_emit_two_events() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let investor = Address::generate(&env);
    let (client, _) = deploy(&env);

    client.init(
        &symbol_short!("INV007"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );
    client.fund(&investor, &3_000_0000000i128);
    // first fund event
    assert_eq!(env.events().all().events().len(), 1);
    let first: FundEvent = event_data(&env, 0);
    assert_eq!(first.funded_amount, 3_000_0000000i128);
    assert_eq!(first.status, 0);

    client.fund(&investor, &7_000_0000000i128);
    // second fund event
    assert_eq!(env.events().all().events().len(), 1);
    let second: FundEvent = event_data(&env, 0);
    assert_eq!(second.funded_amount, 10_000_0000000i128);
    assert_eq!(second.status, 1); // fully funded on second tranche
}

/// Settling before funded must panic — no settle event emitted.
#[test]
#[should_panic(expected = "Escrow must be funded before settlement")]
fn test_settle_before_funded_no_event() {
    let env = Env::default();
    env.mock_all_auths();
    let sme = Address::generate(&env);
    let (client, _) = deploy(&env);

    client.init(
        &symbol_short!("INV008"),
        &sme,
        &10_000_0000000i128,
        &800i64,
        &1000u64,
    );
    client.settle();
}
