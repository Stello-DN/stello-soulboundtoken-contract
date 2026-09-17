# Stello Booking Contract Interface for SBT Integration

| Field | Value |
|---|---|
| Document | Booking Contract dependency interface |
| Version | 0.2 |
| Date | 17 September 2026 |
| Consumer | `StelloSbtEngine` |
| Provider | `StelloBookingContract` |
| Provider version | `0.1.1` |
| Network | Stellar Testnet |
| Verification status | Verified from compiled WASM interface |
| WASM SHA-256 | `90f7e80462bdd6ca9b18f3b2b02c31a7ee467e47b4fb9b996874ccf50ffcd86f` |
| Booking Contract ID | Record before SBT deployment |
| Provider commit SHA | Record before SBT deployment |

## 1. Purpose

This document defines the Booking Contract interface and trust boundary consumed by `StelloSbtEngine` when issuing a Proof-of-Experience credential.

The authoritative integration input is the specification embedded in the exact Booking Contract WASM artifact. Generate the SBT provider client and types from the pinned WASM; do not manually reproduce them.

## 2. Ownership boundary

| Concern | Authoritative owner |
|---|---|
| Booking identity, state, traveler, cancellation, dispute and settlement | Booking Contract |
| Credential, owner and booking-to-credential issuance index | SBT Contract |
| Mint job, retry policy and transaction tracking | Backend |
| Review content and review-submission policy | Review service |

Booking Contract does not store an `sbt_minted` flag. Duplicate prevention belongs to the SBT Contract's authoritative issuance index.

## 3. Verified public interface

The v0.1.1 WASM exports:

```text
__constructor
book
cancel_by_host
cancel_by_traveller
check_in
complete
contract_version
execute_split
get_booking
get_booking_by_ref
get_booking_id_by_ref
get_booking_state
get_cancel_settlement
get_config
get_total_escrowed
lock_escrow
open_dispute
resolve_dispute
update_booking
upgrade
```

These methods are not part of the interface and must not be invented:

```text
initialize
get_next_booking_id
claim_payout
get_claimable_balance
mint_sbt
is_sbt_minted
```

## 4. Constructor

```rust
fn __constructor(
    env: Env,
    stello_wallet: Address,
    token: Address,
    ops_pool: Address,
    review_pool: Address,
    qa_pool: Address,
    o2o_pool: Address,
    host_cancel_fee: i128,
);
```

The provider uses `__constructor`; it does not expose a separately callable `initialize` method.

## 5. SBT dependency method

```rust
fn get_booking(
    env: Env,
    booking_id: u64,
) -> Result<Booking, Error>;
```

An unknown booking returns `Error::BookingNotFound` (code `5`). Dependency failures, archived entries and decode failures must fail closed.

## 6. Verified Booking type

```rust
#[contracttype]
pub struct Booking {
    pub amount: i128,
    pub booking_id: u64,
    pub booking_ref: BytesN<32>,
    pub cancelled_by: CancelledBy,
    pub checked_in: bool,
    pub created_at: u64,
    pub escrow_amount: i128,
    pub escrow_locked: bool,
    pub host: Address,
    pub service_ref: BytesN<32>,
    pub settled: bool,
    pub start_time: u64,
    pub state: BookingState,
    pub token: Address,
    pub traveller: Address,
    pub was_cancelled: bool,
    pub was_disputed: bool,
}
```

Integration rules:

- The field is spelled `traveller`.
- `was_cancelled` is an irreversible cancellation-history flag.
- `was_disputed` is an irreversible dispute-history flag.
- `resolve_dispute()` may return the current state to `Completed`, but preserves `was_disputed == true`.
- Derive the SBT recipient from `booking.traveller`; the caller cannot override it.

## 7. Verified enums

```rust
#[contracttype]
pub enum BookingState {
    Created = 0,
    Escrowed = 1,
    CheckedIn = 2,
    Completed = 3,
    Cancelled = 4,
    Disputed = 5,
}

#[contracttype]
pub enum CancelledBy {
    None = 0,
    Traveller = 1,
    Host = 2,
}

#[contracttype]
pub enum SettlementType {
    Completed = 0,
    TravellerCancel = 1,
    HostCancel = 2,
    Dispute = 3,
}
```

## 8. Approved SBT eligibility policy

```rust
booking.booking_id == requested_booking_id
    && booking.state == BookingState::Completed
    && booking.settled
    && !booking.was_cancelled
    && !booking.was_disputed
```

| Validation | Required value | Failure behavior |
|---|---:|---|
| Booking lookup | Successful | Reject missing/dependency failure |
| `booking_id` | Requested ID | Reject mismatch |
| `state` | `Completed` | Reject other states |
| `settled` | `true` | Reject unsettled booking |
| `was_cancelled` | `false` | Reject any booking ever cancelled |
| `was_disputed` | `false` | Reject any booking ever disputed |

The policy rejects a dispute-resolved booking even when its current state is `Completed` and `settled == true`.

## 9. Integration approach

```rust
mod booking_contract {
    soroban_sdk::contractimport!(
        file = "path/to/pinned/stello_booking_contract.wasm"
    );
}

let client = booking_contract::Client::new(
    &env,
    &trusted_booking_contract,
);

let booking = client.get_booking(&booking_id);
```

Confirm the generated error behavior through compilation and cross-contract tests. Do not duplicate `Booking`, `BookingState` or provider errors manually in production integration code.

### Required artifact record

```text
Provider repository
Provider release/tag
Provider commit SHA
Booking Contract ID and network
WASM filename
WASM SHA-256
Soroban SDK/toolchain version
```

Any Booking ABI change requires client regeneration, integration review and regression tests.

## 10. Authorization and trust boundary

1. Booking settlement succeeds independently.
2. Backend submits `mint_for_booking(booking_id)` using the configured mint authority.
3. SBT authenticates that authority.
4. SBT calls the configured trusted Booking Contract.
5. SBT validates the complete eligibility predicate.
6. SBT derives the owner from `booking.traveller`.
7. SBT checks its own issuance index.
8. SBT atomically stores the credential and index, then emits `SBTMinted`.

The caller must not supply an alternative Booking Contract address, recipient, eligibility flag or Booking state.

Booking Contract exposes an authenticated `upgrade` operation. Pinning its address therefore does not freeze provider behavior; Booking's configured `stello_wallet` remains part of the SBT trust boundary.

## 11. Failure handling

| Condition | Required behavior |
|---|---|
| Booking not found | Reject; do not mutate SBT storage |
| Provider invocation/decode failure | Fail closed |
| Booking ID mismatch | Reject |
| State is not `Completed` | Reject |
| `settled == false` | Reject |
| `was_cancelled == true` | Reject |
| `was_disputed == true` | Reject |
| Credential already exists | Return existing ID; do not emit another mint event |
| Provider data is archived | Restore or fail; never treat archive as absence |
| Mint fails after settlement | Preserve Booking settlement; retry SBT issuance only |
| Submission result is unknown | Query authoritative SBT state before resubmitting |

## 12. Required integration tests

1. Completed, settled, never-cancelled and never-disputed booking mints.
2. Created, Escrowed, CheckedIn, Cancelled and Disputed bookings do not mint.
3. Completed but unsettled booking does not mint.
4. `was_cancelled == true` prevents mint regardless of current state.
5. `was_disputed == true` prevents mint after dispute resolution.
6. Caller cannot override `booking.traveller`.
7. Caller cannot replace the trusted Booking Contract.
8. Unknown booking/provider failure leaves no partial SBT state.
9. Repeated/concurrent requests create at most one credential.
10. Different eligible bookings for one traveler can create distinct credentials.
11. Real cross-contract tests use the pinned Booking v0.1.1 WASM.
12. CI detects a provider hash or ABI mismatch.

## 13. Verification baseline

```bash
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
stellar contract build
stellar contract info interface \
  --wasm target/wasm32v1-none/release/stello_booking_contract.wasm
```

Recorded result:

```text
Provider version: 0.1.1
Tests: 138 passed; 0 failed
Optimized WASM size: 31,217 bytes
Exported functions: 20
WASM SHA-256: 90f7e80462bdd6ca9b18f3b2b02c31a7ee467e47b4fb9b996874ccf50ffcd86f
```

Before deploying SBT, add the deployed Booking Contract ID, deployment transaction/ledger and provider commit SHA.

## 14. Change control

If the provider ABI changes:

1. Stop the SBT release.
2. Pin and inspect the new Booking WASM.
3. Regenerate the imported client.
4. Review `Booking`, `BookingState`, `get_booking` and error changes.
5. Re-run cross-contract eligibility and idempotency tests.
6. Update this document and release evidence.

Do not infer compatibility from a matching method name alone.
