//! LiquiFact Escrow Contract
//!
//! Holds investor funds for an invoice until settlement.
//! - SME receives stablecoin when funding target is met ([`LiquifactEscrow::withdraw`])
//! - SME records optional **collateral commitments** ([`LiquifactEscrow::record_sme_collateral_commitment`]) —
//!   these are **ledger records only**; they do **not** move tokens or trigger liquidation.
//! - [`LiquifactEscrow::settle`] finalizes the escrow after maturity (when configured).
//!
//! ## Compliance hold (legal hold)
//!
//! An admin may set [`DataKey::LegalHold`] to block risk-bearing transitions until cleared:
//! [`LiquifactEscrow::settle`], SME [`LiquifactEscrow::withdraw`], and
//! [`LiquifactEscrow::claim_investor_payout`]. **Clearing** requires the same governance admin
//! to call [`LiquifactEscrow::set_legal_hold`] with `active = false`. This contract does not
//! embed a timelock or council multisig: production deployments should treat `admin` as a
//! governed contract or multisig so holds cannot be used for indefinite fund lock **without**
//! off-chain governance recovery (rotation, vote, emergency procedures).
//!
//! ## Invoice identifier (`invoice_id`)
//!
//! At initialization, `invoice_id` is supplied as a Soroban [`String`] and validated for length
//! and charset before conversion to [`Symbol`] for storage. Align off-chain invoice slugs with the
//! same rules (ASCII alphanumeric + `_`, max length [`MAX_INVOICE_ID_STRING_LEN`]) so indexers stay
//! unambiguous.
//!
//! ## Funding token and registry (immutable hints)
//!
//! Each escrow instance binds exactly one **funding token** contract ([`DataKey::FundingToken`])
//! at [`LiquifactEscrow::init`]; it cannot be changed after deploy. An optional **registry**
//! ([`DataKey::RegistryRef`]) is a read-only discoverability hint only — it is **not** an authority
//! for this contract and must not be used on-chain as proof of registry state without calling the
//! registry yourself.
//!
//! ## Terminal dust sweep
//!
//! [`LiquifactEscrow::sweep_terminal_dust`] moves at most [`MAX_DUST_SWEEP_AMOUNT`] units of the
//! bound funding token from this contract to the immutable **treasury** address, only when the
//! escrow has reached a **terminal** [`InvoiceEscrow::status`] (settled or withdrawn). It cannot run
//! during a legal hold. Transfers go through [`crate::external_calls`] so **pre/post token balances**
//! must match the requested amount (standard SEP-41 behavior); fee-on-transfer or malicious tokens
//! are out of scope and should fail safe assertions. This is meant for rounding residue / stray
//! transfers, not for settling live liabilities — integrations that custody principal on-chain must
//! keep token balances reconciled with `funded_amount` so treasury sweeps cannot pull user funds.
//!
//! ## Ledger time trust model
//!
//! [`LiquifactEscrow::settle`] and [`LiquifactEscrow::claim_investor_payout`] compare against
//! [`Env::ledger`] timestamps only (no wall-clock oracle). Maturity, per-investor **claim locks**
//! from [`LiquifactEscrow::fund_with_commitment`], and [`FundingCloseSnapshot`] metadata must be
//! interpreted as **validator-observed ledger time**, including possible skew between simulated and
//! live networks—integrators should treat boundaries as `>=` / `<` tests on integer seconds.
//!
//! ## Optional tiered yield (immutable table at init)
//!
//! Pass `yield_tiers` to [`LiquifactEscrow::init`] as [`Option`] of a Soroban [`Vec`] of [`YieldTier`].
//! The table is **immutable** for the escrow instance. Investors who use [`LiquifactEscrow::fund_with_commitment`]
//! on their **first** deposit select an effective [`DataKey::InvestorEffectiveYield`] from the ladder;
//! further principal from that address must use [`LiquifactEscrow::fund`]. **Fairness:** tiers are
//! validated non-decreasing in both `min_lock_secs` and `yield_bps` relative to the base [`InvoiceEscrow::yield_bps`].
//!
//! ## Funding-close snapshot (pro-rata)
//!
//! When status first becomes **funded**, [`DataKey::FundingCloseSnapshot`] stores total principal
//! (including over-funding past target), the target, and ledger timestamp/sequence. **Immutable** once
//! written; off-chain pro-rata share for an investor is `get_contribution(addr) / snapshot.total_principal`
//! in rational arithmetic (watch integer rounding off-chain).
//!
//! ## Optional investor allowlist
//!
//! When enabled via [`LiquifactEscrow::enable_allowlist`], only addresses explicitly added by the
//! admin may call [`LiquifactEscrow::fund`] or [`LiquifactEscrow::fund_with_commitment`]. This
//! supports regulated or closed funding rounds.
//!
//! - [`LiquifactEscrow::enable_allowlist`] / [`LiquifactEscrow::disable_allowlist`] — admin-only toggle.
//! - [`LiquifactEscrow::add_to_allowlist`] / [`LiquifactEscrow::remove_from_allowlist`] — admin manages entries.
//! - [`LiquifactEscrow::is_allowlisted`] — read whether an address is approved.
//! - [`LiquifactEscrow::is_allowlist_enabled`] — read whether the gate is active.
//!
//! When the allowlist is **disabled** (default), all investors may fund as before — no migration needed.
//! Per-address entries persist across enable/disable cycles; re-enabling restores the same set.
//!
//! **Gas note:** each allowlist check is a single instance-storage lookup (`O(1)`). There is no
//! on-chain iteration over the list, so gas cost does not grow with list size.

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, symbol_short,
    token::TokenClient, Address, BytesN, Env, String, Symbol, Vec,
};

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Map, Symbol};

/// Product guardrail: a single escrow supports at most this many distinct
/// investors so the per-investor contribution map stays well below Soroban's
/// contract-data entry size limits.
pub const MAX_INVESTORS_PER_ESCROW: u32 = 128;

#[contracttype]
#[derive(Clone)]
/// Storage discriminator for all persisted values.
///
/// Derive rationale:
/// - `Clone`: required because keys are passed by reference into storage APIs and reused
///   across lookups/sets in the same execution path.
pub enum DataKey {
    Initialized,
    Escrow,
    Version,
    /// Per-investor contributed principal recorded during [`LiquifactEscrow::fund`].
    InvestorContribution(Address),
    /// When true, compliance/legal hold blocks payouts and settlement finalization.
    LegalHold,
    /// Optional SME collateral pledge metadata (record-only — not an on-chain asset lock).
    SmeCollateralPledge,
    /// Set when an investor has exercised a claim after settlement.
    InvestorClaimed(Address),
    /// SEP-41 funding asset for this invoice instance; set once in [`LiquifactEscrow::init`].
    FundingToken,
    /// Protocol treasury that may receive [`LiquifactEscrow::sweep_terminal_dust`]; set once in init.
    Treasury,
    /// Optional registry contract id for indexers; **hint only**, not authority (see module rustdoc).
    /// Omitted from storage when unset at init.
    RegistryRef,
    /// Immutable tier table when configured at [`LiquifactEscrow::init`]; omitted when tiering is off.
    /// **Trust:** values are protocol-supplied at deploy; the contract never mutates this key after init.
    YieldTierTable,
    /// Set once when status first becomes **funded** (1); immutable thereafter (pro-rata denominator).
    FundingCloseSnapshot,
    /// Effective annualized yield in bps chosen at this investor’s **first** deposit (see tiered yield).
    InvestorEffectiveYield(Address),
    /// Minimum [`Env::ledger`] timestamp before [`LiquifactEscrow::claim_investor_payout`] (0 = no extra gate).
    InvestorClaimNotBefore(Address),
    /// Minimum [`LiquifactEscrow::fund`] / [`LiquifactEscrow::fund_with_commitment`] amount per call (0 = no floor).
    MinContributionFloor,
/// When set at [`LiquifactEscrow::init`], caps distinct investor addresses that may contribute (`prev == 0`).
    MaxUniqueInvestorsCap,
    /// Count of distinct investor addresses that have a non-zero [`DataKey::InvestorContribution`].
    UniqueFunderCount,
    /// Admin-only **single-set** off-chain attestation digest (e.g. SHA-256 of a legal/KYC bundle).
    /// See [`LiquifactEscrow::bind_primary_attestation_hash`].
    PrimaryAttestationHash,
    /// Append-only audit chain of digests (bounded by [`MAX_ATTESTATION_APPEND_ENTRIES`]).
    /// See [`LiquifactEscrow::append_attestation_digest`].
    AttestationAppendLog,
    /// Proposed new SME address with timelock metadata (set by admin, accepted by new SME).
    BeneficiaryProposal,
    /// Current active SME address (may differ from proposal during timelock period).
    CurrentSmeAddress,
}

/// Maximum basis points (100% = 10_000 bps).
pub const BPS_DENOMINATOR: i64 = 10_000;

// --- Data types ---

/// Full state of an invoice escrow persisted in contract storage (`DataKey::Escrow`).
#[contracttype]
#[derive(Debug, PartialEq)]
/// Full escrow snapshot persisted at [`DataKey::Escrow`].
///
/// Derive rationale:
/// - `Debug`: improves failure diagnostics in tests.
/// - `PartialEq`: allows exact state assertions in tests.
///
/// `Clone` is intentionally omitted to avoid accidental full-state copies.
pub struct InvoiceEscrow {
    pub invoice_id: Symbol,
    pub admin: Address,
    pub sme_address: Address,
    pub amount: i128,
    pub funding_target: i128,
    pub funded_amount: i128,
    pub yield_bps: i64,
    pub maturity: u64,
    /// Per-investor principal contributions for this invoice.
    ///
    /// This is intentionally bounded by `MAX_INVESTORS_PER_ESCROW` to prevent
    /// denial-of-storage patterns where attackers create too many distinct
    /// investor keys inside a single escrow instance.
    pub investor_contributions: Map<Address, i128>,
    /// Escrow status: 0 = open, 1 = funded, 2 = settled
    pub status: u32,
    /// Investor-specific effective yield (bps) after this fund; see [`DataKey::InvestorEffectiveYield`].
    pub investor_effective_yield_bps: i64,
}

#[contractevent]
pub struct EscrowSettled {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub funded_amount: i128,
    pub yield_bps: i64,
    pub maturity: u64,
}

#[contractevent]
pub struct MaturityUpdatedEvent {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub old_maturity: u64,
    pub new_maturity: u64,
}

#[contractevent]
pub struct AdminTransferredEvent {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub new_admin: Address,
}

#[contractevent]
pub struct FundingTargetUpdated {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub old_target: i128,
    pub new_target: i128,
}

#[contractevent]
pub struct LegalHoldChanged {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    /// `1` = hold enabled, `0` = cleared.
    pub active: u32,
}

/// Collateral pledge recorded; asset code is read from [`DataKey::SmeCollateralPledge`].
#[contractevent]
pub struct CollateralRecordedEvt {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub amount: i128,
}

#[contractevent]
pub struct SmeWithdrew {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub amount: i128,
}

#[contractevent]
pub struct InvestorPayoutClaimed {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub investor: Address,
}

#[contractevent]
pub struct TreasuryDustSwept {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub token: Address,
    pub amount: i128,
}

#[contractevent]
pub struct PrimaryAttestationBound {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub digest: BytesN<32>,
}

#[contractevent]
pub struct AttestationDigestAppended {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub index: u32,
    pub digest: BytesN<32>,
}

/// Beneficiary rotation events

#[contractevent]
pub struct BeneficiaryProposed {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub proposed_address: Address,
    pub proposed_at: u64,
    pub timelock_duration_secs: u64,
}

#[contractevent]
pub struct BeneficiaryAccepted {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub new_sme_address: Address,
    pub accepted_at: u64,
}

#[contractevent]
pub struct BeneficiaryCancelled {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub cancelled_at: u64,
}

#[contract]
pub struct LiquifactEscrow;

fn validate_invoice_id_string(env: &Env, invoice_id: &String) -> Result<Symbol, Error> {
    let len = invoice_id.len();
    if !(len >= 1 && len <= MAX_INVOICE_ID_STRING_LEN) {
        return Err(Error::InvoiceIdTooLong);
    }
    let len_u = len as usize;
    let mut buf = [0u8; 32];
    invoice_id.copy_into_slice(&mut buf[..len_u]);
    for &b in &buf[..len_u] {
        let ok = (b >= b'A' && b <= b'Z')
            || (b >= b'a' && b <= b'z')
            || (b >= b'0' && b <= b'9')
            || b == b'_';
        if !ok {
            return Err(Error::InvoiceIdInvalidChars);
        }
    }
    let s = core::str::from_utf8(&buf[..len_u]).expect("invoice_id ascii");
    Ok(Symbol::new(env, s))
}

#[contractimpl]
impl LiquifactEscrow {
    fn legal_hold_active(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::LegalHold)
            .unwrap_or(false)
    }

    fn validate_yield_tiers_table(tiers: &Option<Vec<YieldTier>>, base_yield: i64) -> Result<(), Error> {
        let Some(tiers) = tiers else {
            return Ok(());
        };
        if tiers.len() == 0 {
            return Ok(());
        }
        let n = tiers.len();
        for i in 0..n {
            let t = tiers.get(i).unwrap();
            if !(0..=10_000).contains(&t.yield_bps) {
                return Err(Error::TierYieldBpsOutOfRange);
            }
            if t.yield_bps < base_yield {
                return Err(Error::TierYieldBpsBelowBase);
            }
            if i > 0 {
                let p = tiers.get(i - 1).unwrap();
                if !(t.min_lock_secs > p.min_lock_secs) {
                    return Err(Error::TierLockSecsNotIncreasing);
                }
                if !(t.yield_bps >= p.yield_bps) {
                    return Err(Error::TierYieldNotNonDecreasing);
                }
            }
        }
        Ok(())
    }

    fn effective_yield_for_commitment(env: &Env, base_yield: i64, committed_lock_secs: u64) -> i64 {
        if committed_lock_secs == 0 {
            return base_yield;
        }
        let Some(tiers) = env
            .storage()
            .instance()
            .get::<DataKey, Vec<YieldTier>>(&DataKey::YieldTierTable)
        else {
            return base_yield;
        };
        if tiers.len() == 0 {
            return base_yield;
        }
        let mut best = base_yield;
        let n = tiers.len();
        for i in 0..n {
            let t = tiers.get(i).unwrap();
            if committed_lock_secs >= t.min_lock_secs && t.yield_bps > best {
                best = t.yield_bps;
            }
        }
        best
    }

    /// Computes principal plus yield for a given principal amount and yield in basis points.
    ///
    /// # Deterministic Calculation
    /// This function calculates payout amounts using integer arithmetic only:
    /// - Yield = (principal × yield_bps) / BPS_DENOMINATOR
    /// - Total = principal + yield
    ///
    /// # Rounding Strategy
    /// Integer division truncates toward zero (floor for positive values). This is conservative
    /// for the contract (rounds down yield) and deterministic across all platforms.
    ///
    /// # Overflow Protection
    /// Uses `checked_mul` and `checked_add` to prevent overflow. Panics if overflow detected.
    /// Principal must be non-negative; yield_bps should be 0..=10_000.
    ///
    /// # Security Notes
    /// - Input validation: principal ≥ 0, yield_bps within valid range
    /// - Output validated against maximum expected bounds
    /// - Deterministic: no floating point, platform-independent
    ///
    /// # Example
    /// ```ignore
    /// // 1000 principal at 800 bps (8%) = 1000 + 80 = 1080
    /// let payout = calculate_principal_plus_yield(1000, 800); // returns 1080
    /// ```
    fn calculate_principal_plus_yield(principal: i128, yield_bps: i64) -> i128 {
        assert!(principal >= 0, "principal must be non-negative");
        assert!(
            (0..=BPS_DENOMINATOR).contains(&yield_bps),
            "yield_bps must be between 0 and BPS_DENOMINATOR"
        );

        // Calculate yield: (principal * yield_bps) / BPS_DENOMINATOR
        // Use checked arithmetic to prevent overflow
        let yield_amount = principal
            .checked_mul(yield_bps as i128)
            .expect("principal * yield_bps overflow")
            .checked_div(BPS_DENOMINATOR as i128)
            .expect("division by zero impossible (BPS_DENOMINATOR > 0)");

        // Add yield to principal
        principal
            .checked_add(yield_amount)
            .expect("principal + yield overflow")
    }

    /// Initialize escrow. `funding_target` defaults to `amount`.
    ///
    /// Binds **`funding_token`**, **`treasury`**, and optional **`registry`** for this instance only.
    /// The funding token and treasury addresses are **immutable** after this call; the registry id is
    /// optional metadata for off-chain indexers (not an on-chain authority).
    ///
    /// This function implements a **one-time initialization guard**; once [`DataKey::Initialized`] is
    /// set, any subsequent call to `init` will panic.
    ///
    /// `invoice_id` must satisfy [`MAX_INVOICE_ID_STRING_LEN`] and charset rules (see
    /// [`validate_invoice_id_string`]).
    ///
    /// # Panics
    /// If `amount` or implied target is not positive, `yield_bps > 10_000`, invoice id invalid,
    /// or escrow already initialized.
    pub fn init(
        env: Env,
        admin: Address,
        invoice_id: String,
        sme_address: Address,
        amount: i128,
        yield_bps: i64,
        maturity: u64,
        funding_token: Address,
        registry: Option<Address>,
        treasury: Address,
        yield_tiers: Option<Vec<YieldTier>>,
        min_contribution: Option<i128>,
        max_unique_investors: Option<u32>,
    ) -> Result<InvoiceEscrow, Error> {
        admin.require_auth();

        assert!(amount > 0, "Amount must be positive");
        assert!(
            (0..=10_000).contains(&yield_bps),
            "yield_bps must be between 0 and 10_000"
        );
        assert!(
            !env.storage().instance().has(&DataKey::Initialized),
            "Escrow already initialized"
        );

        env.storage().instance().set(&DataKey::Initialized, &true);

        Self::validate_yield_tiers_table(&yield_tiers, yield_bps);

        let invoice_sym = validate_invoice_id_string(&env, &invoice_id)?;

        let escrow = InvoiceEscrow {
            invoice_id: invoice_sym.clone(),
            admin: admin.clone(),
            sme_address: sme_address.clone(),
            amount,
            funding_target: amount,
            funded_amount: 0,
            yield_bps,
            maturity,
            investor_contributions: Map::new(&env),
            status: 0, // open
        };

        env.storage().instance().set(&DataKey::Escrow, &escrow);
        env.storage()
            .instance()
            .set(&DataKey::Version, &SCHEMA_VERSION);
        env.storage()
            .instance()
            .set(&DataKey::FundingToken, &funding_token);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        if let Some(ref r) = registry {
            env.storage().instance().set(&DataKey::RegistryRef, r);
        }
        if let Some(ref tiers) = yield_tiers {
            if tiers.len() > 0 {
                env.storage()
                    .instance()
                    .set(&DataKey::YieldTierTable, tiers);
            }
        }

        let floor = min_contribution.unwrap_or(0);
        if min_contribution.is_some() {
            if floor <= 0 {
                return Err(Error::MinContributionNotPositive);
            }
            if floor > amount {
                return Err(Error::MinContributionExceedsAmount);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::MinContributionFloor, &floor);

        env.storage()
            .instance()
            .set(&DataKey::UniqueFunderCount, &0u32);

        if let Some(cap) = max_unique_investors {
            if cap == 0 {
                return Err(Error::MaxInvestorsNotPositive);
            }
            env.storage()
                .instance()
                .set(&DataKey::MaxUniqueInvestorsCap, &cap);
        }

        EscrowInitialized {
            name: symbol_short!("escrow_ii"),
            // Read the stored value so we do not clone an in-memory escrow snapshot.
            escrow: Self::get_escrow(env.clone())?,
        }
        .publish(&env);

        Ok(escrow)
    }

    /// Bound funding token (immutable after [`LiquifactEscrow::init`]).
    ///
    /// # Errors
    /// Returns [`Error::FundingTokenNotSet`] if init has not been called.
    pub fn get_funding_token(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::FundingToken)
            .ok_or(Error::FundingTokenNotSet)
    }

    /// Treasury that may receive terminal dust sweeps (immutable after init).
    ///
    /// # Errors
    /// Returns [`Error::TreasuryNotSet`] if init has not been called.
    pub fn get_treasury(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Treasury)
            .ok_or(Error::TreasuryNotSet)
    }

    /// Optional registry contract id (**hint only** — not authority for this escrow).
    pub fn get_registry_ref(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::RegistryRef)
    }

    /// Move up to `amount` (capped by balance and [`MAX_DUST_SWEEP_AMOUNT`]) of the **funding token**
    /// from this contract to [`DataKey::Treasury`].
    ///
    /// # Terminal state requirement
    /// Only permitted when [`InvoiceEscrow::status`] is **2 (settled)** or **3 (withdrawn)**.
    /// Open (0) or funded (1) states reject the call so live principal cannot be swept as dust.
    ///
    /// # Authorization
    /// The configured **treasury** account must authorize this call; the admin cannot sweep unless
    /// it is also the treasury.
    ///
    /// Blocked while [`DataKey::LegalHold`] is active.
    ///
    /// # Errors
    /// Returns [`Error::LegalHoldActive`] if legal hold is enabled.
    /// Returns [`Error::SweepAmountNotPositive`] if `amount <= 0`.
    /// Returns [`Error::SweepAmountExceedsMax`] if `amount > MAX_DUST_SWEEP_AMOUNT`.
    /// Returns [`Error::EscrowNotTerminal`] if escrow status is not settled or withdrawn.
    /// Returns [`Error::NoTokenBalanceToSweep`] if contract has no token balance.
    /// Returns [`Error::SweepAmountZero`] if balance is less than requested amount.
    pub fn sweep_terminal_dust(env: Env, amount: i128) -> Result<i128, Error> {
        if Self::legal_hold_active(&env) {
            return Err(Error::LegalHoldActive);
        }
        if amount <= 0 {
            return Err(Error::SweepAmountNotPositive);
        }
        if amount > MAX_DUST_SWEEP_AMOUNT {
            return Err(Error::SweepAmountExceedsMax);
        }

        let escrow = Self::get_escrow(env.clone())?;
        if !(escrow.status == 2 || escrow.status == 3) {
            return Err(Error::EscrowNotTerminal);
        }

        let treasury: Address = env
            .storage()
            .instance()
            .get(&DataKey::Treasury)
            .ok_or(Error::TreasuryNotSet)?;
        treasury.require_auth();

        let token_addr = env
            .storage()
            .instance()
            .get(&DataKey::FundingToken)
            .ok_or(Error::FundingTokenNotSet)?;
        let this = env.current_contract_address();

        let token = TokenClient::new(&env, &token_addr);
        let balance = token.balance(&this);
        if balance <= 0 {
            return Err(Error::NoTokenBalanceToSweep);
        }
        let sweep_amt = amount.min(balance);
        if sweep_amt <= 0 {
            return Err(Error::SweepAmountZero);
        }

        external_calls::transfer_funding_token_with_balance_checks(
            &env,
            &token_addr,
            &this,
            &treasury,
            sweep_amt,
        );

        TreasuryDustSwept {
            name: symbol_short!("dust_sw"),
            invoice_id: escrow.invoice_id.clone(),
            token: token_addr,
            amount: sweep_amt,
        }
        .publish(&env);

        Ok(sweep_amt)
    }

    pub fn get_escrow(env: Env) -> InvoiceEscrow {
        if !env.storage().instance().has(&DataKey::Initialized) {
            panic!("Escrow not initialized");
        }
        env.storage().instance().get(&DataKey::Escrow).unwrap()
    }

    /// Product limit for distinct investors supported by one escrow.
    pub fn max_investors() -> u32 {
        MAX_INVESTORS_PER_ESCROW
    }

    /// Number of distinct investors recorded for this escrow.
    pub fn get_investor_count(env: Env) -> u32 {
        Self::get_escrow(env).investor_contributions.len()
    }

    /// Amount funded by a specific investor.
    pub fn get_investor_contribution(env: Env, investor: Address) -> i128 {
        Self::get_escrow(env)
            .investor_contributions
            .get(investor)
            .unwrap_or(0)
    }

    /// Record investor funding. In production, this would be called with token transfer.
    pub fn fund(env: Env, investor: Address, amount: i128) -> InvoiceEscrow {
        let mut escrow = Self::get_escrow(env.clone());
        assert!(
            !Self::legal_hold_active(&env),
            "Legal hold blocks new funding while active"
        );
        assert!(escrow.status == 0, "Escrow not open for funding");
        assert!(amount > 0, "Funding amount must be positive");

        let previous_contribution = escrow
            .investor_contributions
            .get(investor.clone())
            .unwrap_or(0);
        if previous_contribution == 0 {
            assert!(
                escrow.investor_contributions.len() < MAX_INVESTORS_PER_ESCROW,
                "Investor limit exceeded"
            );
        }

        let updated_contribution = previous_contribution
            .checked_add(amount)
            .unwrap_or_else(|| panic!("Investor contribution overflow"));
        escrow
            .investor_contributions
            .set(investor, updated_contribution);
        escrow.funded_amount = escrow
            .funded_amount
            .checked_add(amount)
            .unwrap_or_else(|| panic!("Escrow funding overflow"));
        if escrow.funded_amount >= escrow.funding_target {
            escrow.status = 1; // funded - ready to release to SME
        }

        escrow.funded_amount = escrow
            .funded_amount
            .checked_add(amount)
            .ok_or(Error::FundedAmountOverflow)?;
        escrow.funded_amount = new_funded;

        if escrow.status == 0 && escrow.funded_amount >= escrow.funding_target {
            escrow.status = 1;
            if !env.storage().instance().has(&DataKey::FundingCloseSnapshot) {
                let snap = FundingCloseSnapshot {
                    total_principal: escrow.funded_amount,
                    funding_target: escrow.funding_target,
                    closed_at_ledger_timestamp: env.ledger().timestamp(),
                    closed_at_ledger_sequence: env.ledger().sequence(),
                };
                env.storage()
                    .instance()
                    .set(&DataKey::FundingCloseSnapshot, &snap);
            }
        }

        env.storage()
            .instance()
            .set(&contribution_key, &(prev + amount));

        if prev == 0 {
            let cur: u32 = env
                .storage()
                .instance()
                .get(&DataKey::UniqueFunderCount)
                .unwrap_or(0);
            env.storage()
                .instance()
                .set(&DataKey::UniqueFunderCount, &(cur + 1));
        }

        env.storage().instance().set(&DataKey::Escrow, &escrow);

        let investor_effective_yield_bps = env
            .storage()
            .instance()
            .get(&DataKey::InvestorEffectiveYield(investor.clone()))
            .unwrap_or(escrow.yield_bps);

        EscrowFunded {
            name: symbol_short!("funded"),
            invoice_id: escrow.invoice_id.clone(),
            investor: investor.clone(),
            amount,
            funded_amount: escrow.funded_amount,
            status: escrow.status,
            investor_effective_yield_bps,
        }
        .publish(&env);

        Ok(escrow)
    }

    /// Settle the escrow, transitioning to settled state.
    ///
    /// # Errors
    /// Returns [`Error::LegalHoldActive`] if legal hold is enabled.
    /// Returns [`Error::EscrowNotInitialized`] if init has not been called.
    /// Returns [`Error::EscrowNotFunded`] if escrow is not funded (status != 1).
    /// Returns [`Error::EscrowNotMature`] if maturity timestamp has not been reached.
    pub fn settle(env: Env) -> Result<InvoiceEscrow, Error> {
        if Self::legal_hold_active(&env) {
            return Err(Error::LegalHoldActive);
        }

        let mut escrow = Self::get_escrow(env.clone());
        let current_sme = Self::get_current_sme_address(env.clone());
        
        current_sme.require_auth();
        assert!(
            escrow.status == 1,
            "Escrow must be funded before settlement"
        );

        if escrow.maturity > 0 {
            let now = env.ledger().timestamp();
            if now < escrow.maturity {
                return Err(Error::EscrowNotMature);
            }
        }

        escrow.status = 2;

        env.storage().instance().set(&DataKey::Escrow, &escrow);

        EscrowSettled {
            name: symbol_short!("escrow_sd"),
            invoice_id: escrow.invoice_id.clone(),
            funded_amount: escrow.funded_amount,
            yield_bps: escrow.yield_bps,
            maturity: escrow.maturity,
        }
        .publish(&env);

        Ok(escrow)
    }

    /// SME pulls funded liquidity (accounting). Blocked when a legal hold is active.
    ///
    /// # Errors
    /// Returns [`Error::LegalHoldActive`] if legal hold is enabled.
    /// Returns [`Error::EscrowNotInitialized`] if init has not been called.
    /// Returns [`Error::EscrowNotFunded`] if escrow is not funded (status != 1).
    pub fn withdraw(env: Env) -> Result<InvoiceEscrow, Error> {
        if Self::legal_hold_active(&env) {
            return Err(Error::LegalHoldActive);
        }

        let mut escrow = Self::get_escrow(env.clone());
        let current_sme = Self::get_current_sme_address(env.clone());
        
        current_sme.require_auth();

        if escrow.status != 1 {
            return Err(Error::EscrowNotFunded);
        }

        let amount = escrow.funded_amount;
        escrow.status = 3;

        env.storage().instance().set(&DataKey::Escrow, &escrow);

        SmeWithdrew {
            name: symbol_short!("sme_wd"),
            invoice_id: escrow.invoice_id.clone(),
            amount,
        }
        .publish(&env);

        Ok(escrow)
    }

    /// Investor records a payout claim after settlement. Idempotent marker per investor.
    ///
    /// # Errors
    /// Returns [`Error::LegalHoldActive`] if legal hold is enabled.
    /// Returns [`Error::EscrowNotInitialized`] if init has not been called.
    /// Returns [`Error::EscrowNotSettled`] if escrow is not settled (status != 2).
    /// Returns [`Error::CommitmentLockNotExpired`] if the investor's claim lock has not expired.
    /// Returns [`Error::InvestorAlreadyClaimed`] if the investor has already claimed.
    pub fn claim_investor_payout(env: Env, investor: Address) -> Result<(), Error> {
        if Self::legal_hold_active(&env) {
            return Err(Error::LegalHoldActive);
        }

        investor.require_auth();

        let escrow = Self::get_escrow(env.clone())?;
        if escrow.status != 2 {
            return Err(Error::EscrowNotSettled);
        }

        let not_before: u64 = env
            .storage()
            .instance()
            .get(&DataKey::InvestorClaimNotBefore(investor.clone()))
            .unwrap_or(0);
        let now = env.ledger().timestamp();
        if now < not_before {
            return Err(Error::CommitmentLockNotExpired);
        }

        let key = DataKey::InvestorClaimed(investor.clone());
        if env.storage().instance().get(&key).unwrap_or(false) {
            return Err(Error::InvestorAlreadyClaimed);
        }

        env.storage().instance().set(&key, &true);

        InvestorPayoutClaimed {
            name: symbol_short!("inv_claim"),
            invoice_id: escrow.invoice_id.clone(),
            investor,
        }
        .publish(&env);
        Ok(())
    }

    /// Update the maturity timestamp.
    ///
    /// # Errors
    /// Returns [`Error::EscrowNotInitialized`] if init has not been called.
    /// Returns [`Error::MaturityUpdateNotOpen`] if escrow status is not open (0).
    pub fn update_maturity(env: Env, new_maturity: u64) -> Result<InvoiceEscrow, Error> {
        let mut escrow = Self::get_escrow(env.clone())?;
        escrow.admin.require_auth();

        if escrow.status != 0 {
            return Err(Error::MaturityUpdateNotOpen);
        }

        let old_maturity = escrow.maturity;
        escrow.maturity = new_maturity;

        env.storage().instance().set(&DataKey::Escrow, &escrow);

        MaturityUpdatedEvent {
            name: symbol_short!("maturity"),
            invoice_id: escrow.invoice_id.clone(),
            old_maturity,
            new_maturity,
        }
        .publish(&env);

        Ok(escrow)
    }

    /// Transfer admin role to a new address.
    ///
    /// # Errors
    /// Returns [`Error::EscrowNotInitialized`] if init has not been called.
    /// Returns [`Error::AdminNotDifferent`] if `new_admin` equals current admin.
    pub fn transfer_admin(env: Env, new_admin: Address) -> Result<InvoiceEscrow, Error> {
        let mut escrow = Self::get_escrow(env.clone())?;

        escrow.admin.require_auth();

        if escrow.admin == new_admin {
            return Err(Error::AdminNotDifferent);
        }

        escrow.admin = new_admin;

        env.storage().instance().set(&DataKey::Escrow, &escrow);

        AdminTransferredEvent {
            name: symbol_short!("admin"),
            invoice_id: escrow.invoice_id.clone(),
            new_admin: escrow.admin.clone(),
        }
        .publish(&env);

        Ok(escrow)
    }

    // --- Investor allowlist ---

    /// Enable the investor allowlist gate. Only admin may call.
    ///
    /// When enabled, [`LiquifactEscrow::fund`] and [`LiquifactEscrow::fund_with_commitment`]
    /// reject any caller not present in the allowlist. Existing contributions are unaffected.
    pub fn enable_allowlist(env: Env) {
        let escrow = Self::get_escrow(env.clone());
        escrow.admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::AllowlistEnabled, &true);
        AllowlistChanged {
            name: symbol_short!("allowlst"),
            invoice_id: escrow.invoice_id.clone(),
            enabled: 1,
        }
        .publish(&env);
    }

    /// Disable the investor allowlist gate. Only admin may call.
    ///
    /// After this call all addresses may fund again (open round). Per-address entries are
    /// preserved so re-enabling restores the same approved set without re-adding entries.
    pub fn disable_allowlist(env: Env) {
        let escrow = Self::get_escrow(env.clone());
        escrow.admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::AllowlistEnabled, &false);
        AllowlistChanged {
            name: symbol_short!("allowlst"),
            invoice_id: escrow.invoice_id.clone(),
            enabled: 0,
        }
        .publish(&env);
    }

    /// Approve `investor` to fund when the allowlist is active. Only admin may call.
    pub fn add_to_allowlist(env: Env, investor: Address) {
        let escrow = Self::get_escrow(env.clone());
        escrow.admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::InvestorAllowed(investor), &true);
    }

    /// Remove `investor` from the allowlist. Only admin may call.
    ///
    /// Has no effect if the address was not previously added.
    pub fn remove_from_allowlist(env: Env, investor: Address) {
        let escrow = Self::get_escrow(env.clone());
        escrow.admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::InvestorAllowed(investor), &false);
    }

    /// Whether `investor` is in the allowlist (regardless of whether the gate is enabled).
    pub fn is_allowlisted(env: Env, investor: Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::InvestorAllowed(investor))
            .unwrap_or(false)
    }

    /// Whether the allowlist gate is currently active.
    pub fn is_allowlist_enabled(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::AllowlistEnabled)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod test;
