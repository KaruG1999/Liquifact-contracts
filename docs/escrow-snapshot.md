# Funding Close Snapshot

The `FundingCloseSnapshot` is a critical piece of the LiquiFact Escrow contract's audit trail. it captures the exact state of the escrow at the moment it transitions from `Open` (0) to `Funded` (1).

## Purpose

This snapshot serves as the **immutable source of truth** for off-chain pro-rata calculations. When an invoice is over-funded (which is allowed by the contract), the total principal at the moment of funding completion becomes the denominator for investor share calculations.

By capturing this state once and making it immutable, the contract ensures that subsequent actions (like SME withdrawals or settlements) do not shift the relative weight of investor contributions.

## Structure

The snapshot is stored under `DataKey::FundingCloseSnapshot` and contains:

- `total_principal`: The sum of all principal contributed at the moment the funding target was met or exceeded.
- `funding_target`: The original target for the invoice.
- `closed_at_ledger_timestamp`: The ledger timestamp when the snapshot was captured.
- `closed_at_ledger_sequence`: The ledger sequence number when the snapshot was captured.

## Lifecycle and Immutability

1. **Creation**: The snapshot is created during `fund` or `fund_with_commitment` only when `status == 0` and the new `funded_amount >= funding_target`.
2. **Write-Once**: Once the snapshot is written, the contract's logic prevents it from being updated or overwritten.
3. **Persistence**: The snapshot survives all state transitions, including `settle` and `withdraw`.

## Auditing

Integrators can use the `get_funding_close_snapshot` getter to retrieve this metadata. For historical auditing, the `EscrowFunded` event emitted during the snapshot creation contains the `funded_amount` and `status: 1`, allowing off-chain systems to reconcile the snapshot with the event stream.

## Security Considerations

1. **Time and Sequence Bounds**: The snapshot captures `env.ledger().timestamp()` and `env.ledger().sequence()`. In Soroban, these are provided by the host environment and are reliable for on-chain time-based logic. Off-chain systems should treat these as the canonical boundaries for the "funded" state transition.
2. **Token Economics and Assumptions**: As detailed in `escrow/src/external_calls.rs`, this contract strictly assumes standard SEP-41 token mechanics. Malicious, rebasing, or fee-on-transfer (FOT) tokens are **explicitly out of scope** and will trigger safe-failure panics at the balance-check boundaries. This ensures that the `total_principal` captured in the snapshot perfectly matches the real token balance stored in the contract treasury, preserving the integrity of off-chain payout calculations.
