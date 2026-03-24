//! LiquiFact Escrow Contract
//!
//! Holds investor funds for an invoice until settlement.
//! - SME receives stablecoin when funding target is met
//! - Investors receive principal + yield when buyer pays at maturity
//!
//! # Events
//!
//! The contract emits the following Soroban events for off-chain indexers:
//!
//! | Topic                  | Data fields                                                      |
//! |------------------------|------------------------------------------------------------------|
//! | `("init", invoice_id)` | `{ sme_address, amount, yield_bps, maturity }`                   |
//! | `("fund", invoice_id)` | `{ investor, amount, funded_amount, status }`                    |
//! | `("settle", invoice_id)` | `{ sme_address, amount, yield_bps }`                           |

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, vec, Address, Env, Symbol};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoiceEscrow {
    /// Unique invoice identifier (e.g. INV-1023)
    pub invoice_id: Symbol,
    /// SME wallet that receives liquidity
    pub sme_address: Address,
    /// Total amount in smallest unit (e.g. stroops for XLM)
    pub amount: i128,
    /// Funding target must be met to release to SME
    pub funding_target: i128,
    /// Total funded so far by investors
    pub funded_amount: i128,
    /// Yield basis points (e.g. 800 = 8%)
    pub yield_bps: i64,
    /// Maturity timestamp (ledger time)
    pub maturity: u64,
    /// Escrow status: 0 = open, 1 = funded, 2 = settled
    pub status: u32,
}

/// Event payload emitted by [`LiquifactEscrow::init`].
///
/// Topics: `["init", invoice_id]`
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitEvent {
    pub sme_address: Address,
    pub amount: i128,
    pub yield_bps: i64,
    pub maturity: u64,
}

/// Event payload emitted by [`LiquifactEscrow::fund`].
///
/// Topics: `["fund", invoice_id]`
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FundEvent {
    pub investor: Address,
    pub amount: i128,
    pub funded_amount: i128,
    /// Status after this funding call: 0 = still open, 1 = fully funded
    pub status: u32,
}

/// Event payload emitted by [`LiquifactEscrow::settle`].
///
/// Topics: `["settle", invoice_id]`
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettleEvent {
    pub sme_address: Address,
    pub amount: i128,
    pub yield_bps: i64,
}

#[contract]
pub struct LiquifactEscrow;

#[contractimpl]
impl LiquifactEscrow {
    /// Initialize a new invoice escrow.
    ///
    /// Emits an `init` event with topics `["init", invoice_id]` and
    /// payload [`InitEvent`].
    pub fn init(
        env: Env,
        invoice_id: Symbol,
        sme_address: Address,
        amount: i128,
        yield_bps: i64,
        maturity: u64,
    ) -> InvoiceEscrow {
        let escrow = InvoiceEscrow {
            invoice_id: invoice_id.clone(),
            sme_address: sme_address.clone(),
            amount,
            funding_target: amount,
            funded_amount: 0,
            yield_bps,
            maturity,
            status: 0,
        };
        env.storage()
            .instance()
            .set(&symbol_short!("escrow"), &escrow);

        env.events().publish(
            (symbol_short!("init"), invoice_id),
            InitEvent {
                sme_address,
                amount,
                yield_bps,
                maturity,
            },
        );

        escrow
    }

    /// Get current escrow state.
    pub fn get_escrow(env: Env) -> InvoiceEscrow {
        env.storage()
            .instance()
            .get(&symbol_short!("escrow"))
            .unwrap_or_else(|| panic!("Escrow not initialized"))
    }

    /// Record investor funding. In production, this would be called with token transfer.
    ///
    /// Emits a `fund` event with topics `["fund", invoice_id]` and
    /// payload [`FundEvent`].
    pub fn fund(env: Env, investor: Address, amount: i128) -> InvoiceEscrow {
        let mut escrow = Self::get_escrow(env.clone());
        assert!(escrow.status == 0, "Escrow not open for funding");
        escrow.funded_amount += amount;
        if escrow.funded_amount >= escrow.funding_target {
            escrow.status = 1;
        }
        env.storage()
            .instance()
            .set(&symbol_short!("escrow"), &escrow);

        env.events().publish(
            (symbol_short!("fund"), escrow.invoice_id.clone()),
            FundEvent {
                investor,
                amount,
                funded_amount: escrow.funded_amount,
                status: escrow.status,
            },
        );

        escrow
    }

    /// Mark escrow as settled (buyer paid). Releases principal + yield to investors.
    ///
    /// Emits a `settle` event with topics `["settle", invoice_id]` and
    /// payload [`SettleEvent`].
    pub fn settle(env: Env) -> InvoiceEscrow {
        let mut escrow = Self::get_escrow(env.clone());
        assert!(
            escrow.status == 1,
            "Escrow must be funded before settlement"
        );
        escrow.status = 2;
        env.storage()
            .instance()
            .set(&symbol_short!("escrow"), &escrow);

        env.events().publish(
            (symbol_short!("settle"), escrow.invoice_id.clone()),
            SettleEvent {
                sme_address: escrow.sme_address.clone(),
                amount: escrow.amount,
                yield_bps: escrow.yield_bps,
            },
        );

        escrow
    }
}

#[cfg(test)]
mod test;
