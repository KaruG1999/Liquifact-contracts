//! LiquiFact Escrow Contract
//!
//! Holds investor funds for an invoice until settlement.
//! - SME receives stablecoin when funding target is met
//! - Investors receive principal + yield when buyer pays at maturity
//!
//! # Storage Schema Versioning
//!
//! The escrow state is stored under two keys:
//! - `"escrow"` — the [`InvoiceEscrow`] struct (current schema)
//! - `"version"` — a `u32` schema version number
//!
//! ## Version history
//!
//! | Version | Changes |
//! |---------|---------|
//! | 1       | Initial schema: invoice_id, sme_address, amount, funding_target, funded_amount, yield_bps, maturity, status |
//!
//! When a new field is added or the struct layout changes, bump `SCHEMA_VERSION`,
//! add a migration arm in [`LiquifactEscrow::migrate`], and add a corresponding test.

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

/// Current storage schema version. Bump this with every breaking struct change.
pub const SCHEMA_VERSION: u32 = 1;

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
    /// Storage schema version — must equal [`SCHEMA_VERSION`] after any migration
    pub version: u32,
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
        amount: i128,
        yield_bps: i64,
        maturity: u64,
    ) -> InvoiceEscrow {
        // Prevent re-initialization
        assert!(
            !env.storage()
                .instance()
                .has(&symbol_short!("escrow")),
            "Escrow already initialized"
        );
        let escrow = InvoiceEscrow {
            invoice_id: invoice_id.clone(),
            sme_address: sme_address.clone(),
            amount,
            funding_target: amount,
            funded_amount: 0,
            yield_bps,
            maturity,
            status: 0, // open
            version: SCHEMA_VERSION,
        };
        env.storage()
            .instance()
            .set(&symbol_short!("escrow"), &escrow);
        env.storage()
            .instance()
            .set(&symbol_short!("version"), &SCHEMA_VERSION);
        escrow
    }

    /// Get current escrow state.
    pub fn get_escrow(env: Env) -> InvoiceEscrow {
        env.storage()
            .instance()
            .get(&symbol_short!("escrow"))
            .unwrap_or_else(|| panic!("Escrow not initialized"))
    }

    /// Returns the stored schema version.
    pub fn get_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&symbol_short!("version"))
            .unwrap_or(0)
    }

    /// Migrate storage from an older schema version to the current one.
    ///
    /// # Security
    /// In production this MUST be gated behind admin/owner authorization
    /// (e.g. `admin.require_auth()`) so only the contract deployer can trigger it.
    ///
    /// # How to add a new migration
    /// 1. Bump [`SCHEMA_VERSION`].
    /// 2. Add a `from_version == N` arm below that reads the old struct
    ///    (keep the old type alias in a `legacy` module), transforms it, and
    ///    writes the new struct.
    /// 3. Add a test in `test.rs` that simulates the old state and calls `migrate`.
    pub fn migrate(env: Env, from_version: u32) -> u32 {
        let stored: u32 = env
            .storage()
            .instance()
            .get(&symbol_short!("version"))
            .unwrap_or(0);

        assert!(
            stored == from_version,
            "from_version does not match stored version"
        );
        assert!(
            from_version < SCHEMA_VERSION,
            "Already at current schema version"
        );

        // --- Migration arms ---
        // Add a new `if from_version == N` block for each future version bump.
        // Example (not yet needed — shown for illustration):
        //
        // if from_version == 1 {
        //     // Read old struct (V1), write new struct (V2) with new fields defaulted.
        //     let old: InvoiceEscrowV1 = env.storage().instance()
        //         .get(&symbol_short!("escrow")).unwrap();
        //     let new = InvoiceEscrow { ...old, new_field: default_value, version: 2 };
        //     env.storage().instance().set(&symbol_short!("escrow"), &new);
        //     env.storage().instance().set(&symbol_short!("version"), &2u32);
        // }

        // No migrations needed yet (current version is 1, no prior versions exist).
        panic!("No migration path from version {}", from_version);
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

    /// Mark escrow as settled (buyer paid). Releases principal + yield to investors.
    pub fn settle(env: Env) -> InvoiceEscrow {
        let mut escrow = Self::get_escrow(env.clone());
        assert!(
            escrow.status == 1,
            "Escrow must be funded before settlement"
        );
        escrow.status = 2; // settled
        env.storage()
            .instance()
            .set(&symbol_short!("escrow"), &escrow);
        escrow
    }
}

#[cfg(test)]
mod test;
