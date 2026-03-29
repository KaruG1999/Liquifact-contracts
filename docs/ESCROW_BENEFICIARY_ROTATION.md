# Beneficiary Rotation System

This document describes the timelocked beneficiary rotation system implemented in the LiquiFact Escrow contract.

## Overview

The beneficiary rotation system allows the admin to propose a new SME (Small Medium Enterprise) beneficiary with a timelock, ensuring secure and transparent transitions of fund control rights.

## Key Features

- **Timelocked Proposals**: Admin can propose new beneficiaries with a minimum delay before acceptance
- **Authorization Control**: Only the proposed address can accept the role after timelock expires
- **Event Transparency**: All proposal, acceptance, and cancellation actions emit events
- **State Management**: Current active SME address is tracked separately from original SME
- **Admin Controls**: Admin can cancel proposals and manage the rotation process

## Data Structures

### BeneficiaryProposal

```rust
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct BeneficiaryProposal {
    /// Address that will become the new SME after acceptance and timelock expiry
    pub proposed_address: Address,
    /// Ledger timestamp when the proposal was created
    pub proposed_at: u64,
    /// Minimum delay in seconds before the proposal can be accepted
    pub timelock_duration_secs: u64,
}
```

### CurrentSmeAddress

```rust
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CurrentSmeAddress {
    /// The currently active SME address that can withdraw funds
    pub address: Address,
}
```

## Storage Keys

- `DataKey::BeneficiaryProposal` - Stores the current proposal (if any)
- `DataKey::CurrentSmeAddress` - Stores the currently active SME address

## Core Functions

### `propose_beneficiary`

Proposes a new SME beneficiary with a timelock duration.

**Parameters:**

- `proposed_address`: The address that will become the new SME
- `timelock_duration_secs`: Minimum delay before the proposal can be accepted

**Requirements:**

- Admin authorization required
- Proposed address cannot be the same as current SME
- Timelock duration must be greater than zero
- No existing proposal can be active

**Events:**

- `BeneficiaryProposed` - Emitted when proposal is created

### `accept_beneficiary`

Accepts a proposed beneficiary role after timelock expires.

**Parameters:**

- `caller`: The proposed address (must match the proposal)

**Requirements:**

- Caller must be the proposed address
- Proposal must exist
- Timelock must have expired

**Effects:**

- Updates the current SME address
- Removes the proposal
- Emits `BeneficiaryAccepted` event

### `cancel_beneficiary_proposal`

Cancels an active beneficiary proposal.

**Requirements:**

- Admin authorization required
- Proposal must exist

**Effects:**

- Removes the proposal
- Emits `BeneficiaryCancelled` event

### `get_current_sme_address`

Returns the currently active SME address.

**Logic:**

- If `CurrentSmeAddress` exists, returns that address
- Otherwise, returns the original SME address from the escrow

### `can_accept_beneficiary`

Checks if a beneficiary proposal is active and timelock has expired.

**Returns:**

- `true` if proposal exists and timelock has expired
- `false` otherwise

## Event Flow

### Successful Rotation

1. Admin calls `propose_beneficiary`
2. `BeneficiaryProposed` event emitted
3. Wait for timelock duration
4. New SME calls `accept_beneficiary`
5. `BeneficiaryAccepted` event emitted
6. New SME can now call `withdraw` and `settle`

### Cancellation Flow

1. Admin calls `cancel_beneficiary_proposal`
2. `BeneficiaryCancelled` event emitted
3. Original SME retains control

## Security Considerations

### Timelock Security

- Minimum timelock duration of 1 second prevents immediate transfers
- Timelock expiration is based on ledger timestamp, not wall-clock time
- Prevents rushed or coerced transfers

### Authorization Security

- Only admin can propose new beneficiaries
- Only proposed address can accept the role
- Admin cannot force acceptance or bypass timelock

### State Consistency

- Current SME address is updated atomically with proposal acceptance
- Original SME address remains unchanged in escrow storage
- Clear separation between proposal state and active beneficiary state

## Usage Examples

### Proposing a New Beneficiary

```rust
// Admin proposes new SME with 24-hour timelock
let proposal = client.propose_beneficiary(&new_sme_address, &86400u64);
assert_eq!(proposal.timelock_duration_secs, 86400);
```

### Accepting Beneficiary Role

```rust
// After timelock expires, new SME accepts
let updated_escrow = client.accept_beneficiary(&new_sme_address);
assert_eq!(updated_escrow.sme_address, new_sme_address);
```

### Checking Rotation Status

```rust
// Check if rotation is possible
let can_accept = client.can_accept_beneficiary();
if can_accept {
    // New SME can accept the role
}
```

## Integration with Existing Functions

### `withdraw` and `settle`

These functions now use `get_current_sme_address()` instead of the original SME address, ensuring that the current active beneficiary can control funds.

### Legal Hold Compatibility

Beneficiary rotation is not blocked by legal holds, as it's an administrative function that doesn't move funds.

## Testing

The implementation includes comprehensive tests covering:

- Successful proposal and acceptance
- Timelock enforcement
- Authorization requirements
- State transitions
- Event emission
- Edge cases and error conditions

## Migration Considerations

- Existing escrow contracts will continue to work unchanged
- New contracts will have the rotation functionality available
- No migration is required for existing deployments
