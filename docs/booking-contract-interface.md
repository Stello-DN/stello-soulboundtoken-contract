# Stello Booking Contract Interface for SBT Integration

| Field | Value |
|---|---|
| Document | SBT dependency interface |
| Version | 0.1 |
| Date | 17 September 2026 |
| Consumer | StelloSbtEngine |
| Provider | StelloBookingContract |
| Status | Reported baseline — verify against current source/WASM before implementation |

## 1. Purpose

Define the minimum Booking Contract interface required by `StelloSbtEngine` to validate a booking before issuing a Proof-of-Experience credential.

This file is not an authoritative ABI dump. The authoritative integration input is the contract specification embedded in the exact Booking Contract WASM artifact used for deployment. Never manually recreate provider types when a generated Soroban client can be used.

## 2. Ownership boundary

| Concern | Authoritative owner |
|---|---|
| Booking state, traveler, host, escrow and settlement | Booking Contract |
| Credential issuance, owner and duplicate-mint index | SBT Contract |
| Mint job, retry and transaction hash | Backend |
| Review content and one-review constraint | Review service |

Booking Contract does not store `sbt_minted`. SBT Contract must check its own issuance index.

## 3. Reported public interface

The following methods were reported from the deployed Testnet contract interface:

```text
initialize
get_config
book
update_booking
get_booking
get_booking_state
lock_escrow
check_in
complete
execute_split
cancel_by_traveller
cancel_by_host
get_cancel_settlement
open_dispute
resolve_dispute
get_total_escrowed
```

The following methods are not part of the reported interface and must not be invented:

```text
get_next_booking_id
claim_payout
get_claimable_balance
mint_sbt
is_sbt_minted
```

## 4. Minimum SBT dependency

SBT should require only one Booking Contract read operation if it exposes all required fields:

```text
get_booking(booking_id: u64) -> Booking
```

Before implementation, confirm the exact argument and return schema from the current WASM. The returned booking must expose equivalent data for:

| Required fact | Reported field | SBT validation |
|---|---|---|
| Traveler recipient | `traveller` | Credential owner must be derived from this address |
| Lifecycle state | `state` | Must equal `BookingState::Completed` |
| Financial finality | `settled` | Must be `true` |
| Cancellation status | `was_cancelled` | Must be `false` |
| Booking identity | Booking ID or returned ID/reference | Must match the requested booking |

Reported supporting fields include `escrow_locked`, but SBT eligibility should rely on `settled`, not merely escrow being locked.

### Proposed eligibility predicate

```rust
booking.state == BookingState::Completed
    && booking.settled
    && !booking.was_cancelled
```

This predicate is a proposed product baseline. Dispute-resolved completed bookings require a Product Owner decision before final approval.

## 5. Reported types relevant to SBT

### BookingState

```text
Created
Escrowed
CheckedIn
Completed
Cancelled
Disputed
```

### Booking

The reported interface confirms a `Booking` custom type containing lifecycle and financial fields including:

```text
state
escrow_locked
settled
was_cancelled
```

It is also expected to contain the traveler address used by Booking Contract operations. Do not treat this abbreviated description as the complete struct definition. Confirm:

- Exact field names and order.
- Whether the traveler field is spelled `traveller` or `traveler`.
- Exact numeric type of booking ID.
- Exact amount and timestamp types.
- Whether a booking ID is stored inside the value or exists only as the storage key.
- Soroban SDK/type compatibility between both contracts.

### Other reported types

These are not required to decide basic SBT eligibility but form part of the reported Booking Contract interface:

```text
Config
SplitAmounts
CancelSettlement
SettlementAmounts
CancelledBy
SettlementType
```

## 6. Integration approach

Use a client generated from the exact Booking Contract WASM specification. Do not duplicate `Booking` and `BookingState` manually in the SBT repository.

Illustrative structure only:

```rust
mod booking_contract {
    soroban_sdk::contractimport!(file = "path/to/verified_booking_contract.wasm");
}

let client = booking_contract::Client::new(&env, &booking_contract_address);
let booking = client.get_booking(&booking_id);
```

The actual generated client name and method signature must come from the WASM specification.

### Artifact rules

- Pin the Booking Contract WASM/spec version consumed by SBT.
- Record Booking Contract repository, commit SHA and WASM SHA-256.
- Do not point generation at an untracked local artifact.
- Rebuild and review generated types whenever the Booking ABI changes.
- Treat an ABI-breaking Booking change as an SBT integration change requiring regression tests.

## 7. Authorization model

Recommended MVP flow:

1. Booking settlement succeeds in its own transaction.
2. Backend mint worker submits `mint_for_booking(booking_id)` to SBT Contract.
3. SBT Contract authenticates the configured mint authority.
4. SBT Contract calls the configured trusted Booking Contract.
5. SBT derives the recipient from the returned booking.
6. SBT checks its own booking-to-credential index.
7. SBT stores the credential/index atomically and emits `SBTMinted`.

Backend must not submit eligibility booleans or choose the credential recipient.

The SBT configuration must contain one trusted Booking Contract address. A mint caller must not be able to provide an arbitrary Booking Contract address.

## 8. Failure handling

| Condition | Required behavior |
|---|---|
| Booking does not exist | Reject mint; no SBT storage mutation |
| Booking Contract invocation fails | Reject/fail closed; backend may reconcile and retry |
| State is not Completed | Reject as ineligible |
| `settled == false` | Reject as ineligible |
| `was_cancelled == true` | Reject as ineligible |
| Booking already has credential | Return existing ID or typed `AlreadyMinted` according to approved API; never issue twice |
| Booking storage is archived | Restore provider data or fail closed; do not interpret as missing/new |
| SBT issuance fails after settlement | Booking settlement remains successful; retry only SBT issuance |
| Transaction result is unknown | Query confirmed SBT state before submitting another transaction |

## 9. Required verification commands

Run against the exact build/deployment artifact:

```bash
stellar contract info interface \
  --wasm <PATH_TO_BOOKING_WASM>
```

For a deployed Testnet contract:

```bash
stellar contract info interface \
  --id "$BOOKING_CONTRACT_ID" \
  --network testnet
```

Save the output as a release artifact or CI evidence. Confirm at minimum:

- `get_booking` exists.
- Its booking ID type.
- Complete `Booking` struct.
- Exact `BookingState` representation.
- Traveler field spelling/type.
- `settled` and `was_cancelled` are returned.
- The artifact hash matches the intended deployed code.

Do not commit addresses, keys or environment-specific secrets into contract source.

## 10. Contract-level integration tests

1. Eligible Completed + settled + not-cancelled booking mints to its recorded traveler.
2. Created, Escrowed, CheckedIn, Cancelled and Disputed bookings do not mint.
3. Completed but unsettled booking does not mint.
4. Cancelled flag prevents mint even if inconsistent state is presented in a mock.
5. Caller cannot override traveler.
6. Caller cannot replace trusted Booking Contract.
7. Unknown booking and provider invocation failure leave no partial SBT state.
8. Repeated/concurrent request produces at most one credential.
9. Different eligible bookings for the same traveler can produce separate credentials.
10. A deployment/spec mismatch is detected before production deployment.

Mock-based tests are useful for SBT unit coverage, but Epic 3 must demonstrate actual cross-contract invocation using the compiled Booking Contract WASM.

## 11. Open items before coding mint

- Verify the complete current Booking struct from source/WASM.
- Confirm Booking Contract artifact path, version, commit and hash.
- Confirm the exact `get_booking` behavior for a missing/archived booking.
- Confirm the Soroban SDK/toolchain versions used by both repositories.
- Decide whether dispute-resolved Completed bookings qualify for SBT.
- Decide idempotent result: return existing credential ID or `AlreadyMinted`.
- Approve persistent TTL and restoration policy for both Credential and issuance index.

If any required eligibility field is absent, stop. Propose the smallest read-only Booking Contract interface change, document compatibility impact and implement it under a separate approved Jira ticket.

## 12. Source baseline

- Reported StelloBookingContract Testnet interface from project deployment discussions.
- `Stello_SBT_BA_Specification_v0.1.md`.
- `Stello_Booking_Contract_BA_Specification_v0.1.md`.
- Actual Booking Contract source, generated specification and deployed WASM must supersede this reported summary when inspected.
