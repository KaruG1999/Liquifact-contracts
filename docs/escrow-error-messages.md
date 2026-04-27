# Liquifact Escrow Error Messages

This document catalogs the panic messages emitted by the Liquifact Escrow contract. These messages are intended for developers and SDK authors to help diagnose transaction failures.

> [!IMPORTANT]
> In Soroban, contract panics abort the entire transaction. The string messages provided in `assert!` and `panic!` calls are visible in transaction simulation and diagnostic events.

## 📋 Message Catalog

### Initialization (`init`)
| Message | Rationale |
|---------|-----------|
| `Amount must be positive` | The base invoice amount must be > 0. |
| `yield_bps must be between 0 and 10_000` | Yield must be in the range 0% - 100%. |
| `Escrow already initialized` | `init` was called on a contract instance that already has state. |
| `min_contribution must be positive when configured` | If a floor is set, it must be > 0. |
| `min_contribution cannot exceed initial invoice amount` | The floor cannot be higher than the target. |
| `max_unique_investors must be positive when configured` | If an investor cap is set, it must be > 0. |
| `invoice_id length must be 1..=32` | The invoice string identifier is too long or empty. |
| `invoice_id must be [A-Za-z0-9_] only` | The identifier contains invalid characters for a Soroban Symbol. |

### Funding (`fund` / `fund_with_commitment`)
| Message | Rationale |
|---------|-----------|
| `Funding amount must be positive` | Investors cannot deposit 0 or negative amounts. |
| `funding amount below min_contribution floor` | The deposit does not meet the minimum required amount. |
| `Legal hold blocks new funding while active` | The admin has frozen the escrow for compliance reasons. |
| `Escrow not open for funding` | The escrow is already funded, settled, or withdrawn. |
| `Investor not on allowlist` | The allowlist is active and the caller is not permitted to fund. |
| `unique investor cap reached` | The maximum number of distinct investor addresses has been reached. |
| `Additional principal after a tiered first deposit must use fund()` | Once a tier is selected via commitment, subsequent deposits must use the simple `fund` method. |

### Settlement & Withdrawal
| Message | Rationale |
|---------|-----------|
| `Legal hold blocks settlement finalization` | Settlement cannot proceed during a compliance hold. |
| `Escrow must be funded before settlement` | Cannot settle an invoice that hasn't met its funding target. |
| `Escrow has not yet reached maturity` | The ledger timestamp is earlier than the configured `maturity`. |
| `Legal hold blocks SME withdrawal` | SME cannot pull funds during a compliance hold. |
| `Escrow must be funded before withdrawal` | SME can only withdraw funds after the status is `Funded`. |

### Payout Claims (`claim_investor_payout`)
| Message | Rationale |
|---------|-----------|
| `Legal hold blocks investor claims` | Payouts are frozen during a compliance hold. |
| `Address has no contribution to claim` | The caller never participated in this escrow. |
| `Escrow must be settled before investor claim` | Payouts only happen after the `Settled` state is reached. |
| `Investor commitment lock not expired` | The investor's specific lock period (from tiered yield) has not elapsed. |

### Administrative & Compliance
| Message | Rationale |
|---------|-----------|
| `Target must be strictly positive` | Funding target update must be > 0. |
| `Target can only be updated in Open state` | Cannot change the target once funding has completed. |
| `Target cannot be less than already funded amount` | Cannot lower the target below what has already been committed. |
| `primary attestation already bound` | The primary audit hash is immutable once set. |
| `attestation append log capacity reached` | The append-only log has reached its limit (32 entries). |
| `New admin must differ from current admin` | Transferring admin to the same address is rejected. |

### Token & External Calls
| Message | Rationale |
|---------|-----------|
| `insufficient token balance before transfer` | The contract does not have enough tokens to perform the transfer. |
| `sender balance delta must equal transfer amount` | Detected a fee-on-transfer or non-compliant token. |
| `recipient balance delta must equal transfer amount` | Detected a malicious or non-compliant token. |

---

## 🛠️ Guidance for SDK Authors

### Error Mapping
SDKs should catch these strings from the `diagnostic_events` or the `error` field of the transaction simulation. It is recommended to map common failures to user-friendly enums:

- `Legal hold active` -> `Error::ComplianceHold`
- `Escrow not open` -> `Error::InvalidState`
- `amount below floor` -> `Error::InsufficientAmount`

### Stability Policy
Panic messages marked with a 📋 in this catalog are considered part of the **Integration Contract**. While logic may evolve, these strings will remain stable within the same major schema version to avoid breaking off-chain error handlers.

### Recovery Suggestions
- **Legal Hold**: Direct the user to contact the platform admin or check the `LegalHoldChanged` event history.
- **Maturity**: Verify the `maturity` field from `get_escrow` against the current ledger time before submitting a `settle` transaction.
