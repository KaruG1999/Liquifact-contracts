//! LiquiFact Escrow Contract
//!
//! Holds investor funds for an invoice until settlement.
//! - SME receives stablecoin when funding target is met
//! - Investors receive principal + yield when buyer pays at maturity

use soroban_sdk::{
    contract, contractevent, contractimpl, contracttype, symbol_short, vec, Address, Env, Symbol,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoiceEscrow {
    /// Unique invoice identifier (e.g. INV-1023)
    pub invoice_id: Symbol,
    /// SME wallet that receives liquidity
    pub sme_address: Address,
    /// Administrator authorized to update maturity
    pub admin: Address,
    /// Total amount in smallest unit (e.g. stroops for XLM)
    pub amount: i128,
    /// Funding target must be met to release to SME
    pub funding_target: i128,
    /// Total funded so far by investors
    pub funded_amount: i128,
    /// Total settled (paid by buyer) so far
    pub settled_amount: i128,
    /// Yield basis points (e.g. 800 = 8%)
    pub yield_bps: i64,
    /// Maturity timestamp (ledger time)
    pub maturity: u64,
    /// Escrow status: 0 = open, 1 = funded, 2 = settled
    pub status: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaturityUpdatedEvent {
    pub invoice_id: Symbol,
    pub old_maturity: u64,
    pub new_maturity: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialSettlementEvent {
    pub invoice_id: Symbol,
    pub amount: i128,
    pub settled_amount: i128,
    pub total_due: i128,
}

#[contract]
pub struct LiquifactEscrow;

#[contractimpl]
impl LiquifactEscrow {
    /// Initialize a new invoice escrow.
    pub fn init(
        env: Env,
        invoice_id: Symbol,
        sme_address: Address,
        admin: Address,
        amount: i128,
        yield_bps: i64,
        maturity: u64,
    ) -> InvoiceEscrow {
        let escrow = InvoiceEscrow {
            invoice_id: invoice_id.clone(),
            sme_address: sme_address.clone(),
            admin: admin.clone(),
            amount,
            funding_target: amount,
            funded_amount: 0,
            settled_amount: 0,
            yield_bps,
            maturity,
            status: 0, // open
        };
        env.storage()
            .instance()
            .set(&symbol_short!("escrow"), &escrow);
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
    pub fn fund(env: Env, _investor: Address, amount: i128) -> InvoiceEscrow {
        let mut escrow = Self::get_escrow(env.clone());
        assert!(escrow.status == 0, "Escrow not open for funding");
        escrow.funded_amount += amount;
        if escrow.funded_amount >= escrow.funding_target {
            escrow.status = 1; // funded - ready to release to SME
        }
        env.storage()
            .instance()
            .set(&symbol_short!("escrow"), &escrow);
        escrow
    }

    /// Mark escrow as settled (buyer paid partial or full). 
    /// Releases principal + yield to investors.
    pub fn settle(env: Env, amount: i128) -> InvoiceEscrow {
        let mut escrow = Self::get_escrow(env.clone());
        assert!(
            escrow.status == 1 || escrow.status == 2,
            "Escrow must be funded before settlement"
        );
        
        // Final status 2 means already fully settled, but we allow 
        // calling this as long as it doesn't exceed total_due
        
        let interest = (escrow.amount * (escrow.yield_bps as i128)) / 10000;
        let total_due = escrow.amount + interest;
        
        escrow.settled_amount += amount;
        
        assert!(
            escrow.settled_amount <= total_due,
            "Settlement amount exceeds total due"
        );
        
        if escrow.settled_amount == total_due {
            escrow.status = 2; // fully settled
        }

        env.storage()
            .instance()
            .set(&symbol_short!("escrow"), &escrow);

        // Audit event
        let topics = vec![&env, symbol_short!("settle"), symbol_short!("partial")];
        env.events().publish(
            topics,
            PartialSettlementEvent {
                invoice_id: escrow.invoice_id.clone(),
                amount,
                settled_amount: escrow.settled_amount,
                total_due,
            },
        );

        escrow
    }

    /// Update maturity timestamp. Only allowed by admin in Open state.
    pub fn update_maturity(env: Env, new_maturity: u64) -> InvoiceEscrow {
        let mut escrow = Self::get_escrow(env.clone());

        // Strict authorization check
        escrow.admin.require_auth();

        // Validation: preventing post-funding tampering
        assert!(escrow.status == 0, "Maturity can only be updated in Open state");

        let old_maturity = escrow.maturity;
        escrow.maturity = new_maturity;

        env.storage()
            .instance()
            .set(&symbol_short!("escrow"), &escrow);

        // Audit event
        let topics = vec![&env, symbol_short!("maturity"), symbol_short!("updated")];
        env.events().publish(
            topics,
            MaturityUpdatedEvent {
                invoice_id: escrow.invoice_id.clone(),
                old_maturity,
                new_maturity,
            },
        );

        escrow
    }
}

#[cfg(test)]
mod test;
