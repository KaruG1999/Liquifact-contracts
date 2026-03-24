//! LiquiFact Escrow Contract
//!
//! Holds investor funds for an invoice until settlement.
//! - SME receives stablecoin when funding target is met
//! - Investors receive principal + yield when buyer pays at maturity
//!
//! ## Per-Investor Ledger
//!
//! Each call to [`LiquifactEscrow::fund`] records the investor's cumulative
//! contribution under a dedicated storage key so that payout accounting,
//! auditing, and future partial-settlement logic can query exact amounts
//! without replaying the full event history.
//!
//! ## Authorization Boundaries
//!
//! | Function | Required Signer        | Reason                                      |
//! |----------|------------------------|---------------------------------------------|
//! | `init`   | `admin`                | Only the designated admin may create escrows |
//! | `fund`   | `investor`             | Investor authorizes their own funding action |
//! | `settle` | `sme_address`          | Only the SME (payee) may trigger settlement  |
//!
//! All auth checks are enforced via [`Address::require_auth`], which integrates
//! with Soroban's native authorization framework and is verifiable on-chain.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvoiceEscrow {
    /// Unique invoice identifier (e.g. INV-1023)
    pub invoice_id: Symbol,
    /// Admin address that initialized this escrow
    pub admin: Address,
    /// SME wallet that receives liquidity and authorizes settlement
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

/// Storage key variants used by this contract.
///
/// Using an enum keeps keys type-safe and avoids raw-symbol collisions.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Singleton escrow state.
    Escrow,
    /// Per-investor cumulative contribution.
    /// Maps `Address → i128` (amount in smallest unit).
    InvestorContribution(Address),
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct LiquifactEscrow;

#[contractimpl]
impl LiquifactEscrow {
    /// Initialize a new invoice escrow.
    ///
    /// # Authorization
    /// Requires authorization from `admin`. This prevents any unauthorized
    /// party from creating or overwriting escrow state.
    ///
    /// # Panics
    /// - If an escrow has already been initialized.
    pub fn init(
        env: Env,
        admin: Address,
        invoice_id: Symbol,
        sme_address: Address,
        amount: i128,
        yield_bps: i64,
        maturity: u64,
    ) -> InvoiceEscrow {
        // Auth boundary: only the admin may initialize the escrow.
        admin.require_auth();

        // Prevent re-initialization — escrow must not already exist.
        assert!(
            !env.storage().instance().has(&DataKey::Escrow),
            "Escrow already initialized"
        );

        let escrow = InvoiceEscrow {
            invoice_id: invoice_id.clone(),
            admin: admin.clone(),
            sme_address: sme_address.clone(),
            amount,
            funding_target: amount,
            funded_amount: 0,
            yield_bps,
            maturity,
            status: 0, // open
        };
        env.storage().instance().set(&DataKey::Escrow, &escrow);
        escrow
    }

    /// Get current escrow state.
    pub fn get_escrow(env: Env) -> InvoiceEscrow {
        env.storage()
            .instance()
            .get(&DataKey::Escrow)
            .unwrap_or_else(|| panic!("Escrow not initialized"))
    }

    /// Record investor funding.
    ///
    /// Requires the investor to authorise the call. The investor's cumulative
    /// contribution is stored under [`DataKey::InvestorContribution`] so it
    /// can be queried later for payout calculations.
    ///
    /// In production this would be paired with a token transfer; here we
    /// record the accounting entry only.
    ///
    /// # Authorization
    /// Requires authorization from `investor`. Each investor authorizes their
    /// own funding contribution, preventing third parties from funding on their behalf.
    ///
    /// # Panics
    /// - If the escrow is not in the open (status = 0) state.
    pub fn fund(env: Env, investor: Address, amount: i128) -> InvoiceEscrow {
        // Auth boundary: investor must authorize their own funding action.
        investor.require_auth();

        let mut escrow = Self::get_escrow(env.clone());
        assert!(escrow.status == 0, "Escrow not open for funding");
        assert!(amount > 0, "Funding amount must be positive");

        // Update aggregate funded amount.
        escrow.funded_amount += amount;
        if escrow.funded_amount >= escrow.funding_target {
            escrow.status = 1; // funded – ready to release to SME
        }
        env.storage().instance().set(&DataKey::Escrow, &escrow);

        // Update per-investor ledger entry.
        let key = DataKey::InvestorContribution(investor.clone());
        let prev: i128 = env.storage().instance().get(&key).unwrap_or(0);
        env.storage().instance().set(&key, &(prev + amount));

        escrow
    }

    /// Return the cumulative amount contributed by `investor`.
    ///
    /// Returns `0` if the investor has never funded this escrow.
    pub fn get_contribution(env: Env, investor: Address) -> i128 {
        let key = DataKey::InvestorContribution(investor);
        env.storage().instance().get(&key).unwrap_or(0)
    }

    /// Mark escrow as settled (buyer paid). Releases principal + yield to investors.
    ///
    /// # Authorization
    /// Requires authorization from the `sme_address` stored in the escrow.
    /// Only the SME that is the beneficiary of the escrow may trigger settlement,
    /// preventing unauthorized state transitions to the settled state.
    ///
    /// # Panics
    /// - If the escrow is not in the funded (status = 1) state.
    pub fn settle(env: Env) -> InvoiceEscrow {
        let mut escrow = Self::get_escrow(env.clone());

        // Auth boundary: only the SME (payee) may settle the escrow.
        escrow.sme_address.require_auth();

        assert!(
            escrow.status == 1,
            "Escrow must be funded before settlement"
        );
        let now = env.ledger().timestamp();
        assert!(
            escrow.maturity == 0 || now >= escrow.maturity,
            "Cannot settle before maturity"
        );
        escrow.status = 2; // settled
        env.storage().instance().set(&DataKey::Escrow, &escrow);
        escrow
    }
}

#[cfg(test)]
mod test;
