# Escrow Attestations: KYC/KYB Operational Flows

This document describes how the two attestation entrypoints on the LiquiFact escrow contract
are used in KYC (Know Your Customer) and KYB (Know Your Business) compliance workflows.

## What this is — and what it is not

Both entrypoints store a **32-byte digest** (e.g. SHA-256 of an IPFS CID or a document bundle
hash) on-chain. This is a **chain anchor**: a tamper-evident pointer that lets any observer
confirm that a specific document set existed at a specific ledger sequence.

**This is not a ZK claim.** The contract does not:
- verify the contents of the referenced document
- prove any property about the document (e.g. "this person passed KYC")
- execute any on-chain logic based on the attestation value
- interact with an oracle, identity registry, or ZK verifier

The on-chain record is a hash. Off-chain verifiers must retrieve the referenced document
independently and recompute the hash to confirm the anchor matches.

---

## Entrypoints

### `bind_primary_attestation_hash(digest: BytesN<32>)`

| Property | Value |
|---|---|
| Auth | `InvoiceEscrow::admin` |
| Write policy | **Single-set** — panics if already bound |
| Storage key | `DataKey::PrimaryAttestationHash` |
| Event | `PrimaryAttestationBound { invoice_id, digest }` |

Binds the canonical compliance document digest for this escrow instance. Intended for the
initial KYC/KYB bundle that covers the SME and the invoice at origination.

**Frontrunning note:** whichever transaction lands first wins. Observers must read on-chain
state (or parse events) after ledger finality — there is no replay lock or commit-reveal scheme.
In practice, the admin key is governance-controlled, so frontrunning is only a concern if the
admin key is compromised.

### `append_attestation_digest(digest: BytesN<32>)`

| Property | Value |
|---|---|
| Auth | `InvoiceEscrow::admin` |
| Write policy | **Append-only**, bounded by `MAX_ATTESTATION_APPEND_ENTRIES` (32) |
| Storage key | `DataKey::AttestationAppendLog` |
| Event | `AttestationDigestAppended { invoice_id, index, digest }` |

Appends a digest to a bounded audit log. Intended for incremental compliance updates: re-KYC
cycles, updated KYB documents, AML screening refreshes, or legal hold evidence bundles.

The log is an ordered sequence, not a set — duplicate digests are allowed (e.g. re-confirming
an unchanged document at a new ledger timestamp via the event).

The 33rd append panics with `"attestation append log capacity reached"`. If more than 32
incremental anchors are needed, deploy a new escrow instance or extend the log off-chain using
the event stream.

---

## KYC/KYB operational flows

### Flow 1 — SME onboarding (KYB at origination)

```
Off-chain                              On-chain
─────────────────────────────────────────────────────────────────────
1. Compliance team collects KYB docs
   (company registration, UBO list,
   bank statements, AML screening).

2. Bundle is hashed:
   digest = SHA-256(canonical_bundle)

3. Bundle uploaded to IPFS or
   internal document store.
                                       4. Admin calls:
                                          bind_primary_attestation_hash(digest)
                                          → PrimaryAttestationBound event emitted
                                          → DataKey::PrimaryAttestationHash set (immutable)

5. Indexer reads PrimaryAttestationBound.
   Off-chain verifier fetches bundle,
   recomputes SHA-256, confirms match.
```

The primary hash is the canonical anchor for the escrow. It cannot be replaced — if the
origination bundle is superseded, use the append log (Flow 2).

---

### Flow 2 — Periodic re-KYC / KYB refresh (append log)

```
Off-chain                              On-chain
─────────────────────────────────────────────────────────────────────
1. Annual re-KYC cycle: compliance
   team collects updated docs.

2. New bundle hashed:
   digest = SHA-256(updated_bundle_v2)

3. Bundle stored with version tag.
                                       4. Admin calls:
                                          append_attestation_digest(digest)
                                          → AttestationDigestAppended { index: 0, digest }

   (Repeat for each refresh cycle,
    up to index 31.)
```

Each append is timestamped by the ledger sequence in the event. Off-chain systems can build a
full compliance timeline by replaying `AttestationDigestAppended` events in order.

---

### Flow 3 — Investor KYC (off-chain, referenced by append log)

Investor KYC is **not stored per-investor** in this contract. The escrow tracks investor
addresses and principal amounts; it does not custody identity documents.

The recommended pattern:

```
Off-chain                              On-chain
─────────────────────────────────────────────────────────────────────
1. Compliance platform runs KYC for
   each investor address.

2. Platform produces a Merkle root
   over (address, kyc_status, expiry)
   tuples for all approved investors.

3. Root hashed:
   digest = SHA-256(merkle_root || timestamp)
                                       4. Admin calls:
                                          append_attestation_digest(digest)
                                          → AttestationDigestAppended { index: N, digest }

5. Investor submits Merkle proof
   off-chain to compliance platform.
   Platform verifies proof against
   the on-chain anchor.
```

This keeps investor PII off-chain while providing a tamper-evident on-chain commitment that
a specific set of addresses was approved at a specific time.

---

### Flow 4 — Legal hold with evidence anchor

When a legal hold is set (`set_legal_hold(true)`), the admin may optionally anchor the
evidence bundle that triggered the hold:

```
Off-chain                              On-chain
─────────────────────────────────────────────────────────────────────
1. Legal team assembles hold evidence
   (court order, regulator notice, etc.)

2. digest = SHA-256(evidence_bundle)
                                       3. Admin calls:
                                          set_legal_hold(true)
                                          append_attestation_digest(digest)

4. Evidence bundle stored in legal
   document management system.
   On-chain digest provides audit trail.
```

Clearing the hold follows the same pattern in reverse: anchor the clearance document, then
call `clear_legal_hold()`.

---

## Security notes

- **Admin key custody:** both entrypoints require `InvoiceEscrow::admin` auth. Production
  deployments should use a multisig or governed contract as admin so no single key can bind
  an arbitrary digest. See [ADR-002](adr/ADR-002-auth-boundaries.md).

- **No on-chain verification:** the contract stores bytes. It does not fetch the referenced
  document, verify a signature, or enforce any property of the digest content. Verification
  is entirely off-chain.

- **Collision resistance:** SHA-256 is assumed collision-resistant for operational purposes.
  If a weaker hash is used off-chain, the on-chain anchor provides no stronger guarantee.

- **Append log is not a set:** duplicate digests are accepted. Off-chain consumers should
  deduplicate by digest value if uniqueness matters for their use case.

- **Capacity:** `MAX_ATTESTATION_APPEND_ENTRIES = 32`. This is a storage-growth guardrail,
  not a compliance limit. If 32 entries are insufficient, the operational playbook should
  define a rotation policy (e.g. new escrow instance per compliance period).

- **Token economics:** attestation entrypoints do not interact with token balances, funding
  state, or settlement flows. They are metadata-only. See
  [`external_calls.rs`](../escrow/src/external_calls.rs) for token transfer boundaries.

- **Out of scope:** ZK proofs, on-chain identity verification, cross-contract KYC registry
  lookups, and automated compliance enforcement are all out of scope for this contract version.

---

## Test coverage

Attestation behavior is covered in [`escrow/src/test/attestations.rs`](../escrow/src/test/attestations.rs):

| Test | What it proves |
|---|---|
| `test_bind_primary_hash_stores_and_reads` | Happy path: bind succeeds, getter returns digest |
| `test_get_primary_hash_none_before_bind` | Getter returns `None` before any bind |
| `test_bind_primary_hash_same_digest_panics` | Second bind (same digest) panics |
| `test_bind_primary_hash_different_digest_panics` | Second bind (different digest) panics |
| `test_bind_primary_hash_non_admin_panics` | Non-admin bind is rejected |
| `test_append_log_empty_before_first_append` | Log is empty before first append |
| `test_append_single_entry_stored` | Single append stored at index 0 |
| `test_append_multiple_entries_ordered` | Insertion order preserved |
| `test_append_exactly_max_entries_succeeds` | 32nd entry succeeds (boundary inclusive) |
| `test_append_beyond_max_panics` | 33rd entry panics |
| `test_append_duplicate_digest_allowed` | Duplicate digests accepted |
| `test_append_non_admin_panics` | Non-admin append is rejected |
| `test_primary_bind_does_not_affect_append_log` | Primary bind leaves log empty |
| `test_append_does_not_affect_primary_hash` | Append leaves primary hash `None` |
| `test_primary_and_append_coexist` | Both can be set independently |
