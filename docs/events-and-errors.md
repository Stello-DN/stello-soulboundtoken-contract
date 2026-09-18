# SBT events and contract errors

STELLO-35 requires stable issuance events and typed errors for SBT operations,
with backend-indexable payloads and documented, tested schemas.

This is the approved DESIGN.md baseline (section 13), already implemented by
`SbtMinted`, `SbtInitialized` and `Error` in STELLO-31 and unchanged here. This
ticket adds two constructor-panic regression tests that were previously
unexercised and records STELLO-35 acceptance evidence against the existing
implementation and test suite. No conflict between Jira, DESIGN.md and code
was found (AGENTS.md conflict-reporting rule); see the note on "booking
reference" below.

## Events

### `SbtMinted` — topics `["sbt", "minted", credential_id]`

| Field | Type | Encoding |
|---|---|---|
| credential_id | u64 | Topic |
| booking_contract | Address | Data |
| booking_id | u64 | Data |
| traveller | Address | Data; equals `credential.owner` |
| issued_at | u64 | Data; equals `credential.issued_at` |
| schema_version | u32 | Data; initially 1 |

Emitted exactly once for a new committed issuance in `mint_for_booking`; never
on a duplicate/idempotent retry and never on a failed invocation, so backend
indexers can treat each event as a one-time issuance record keyed by
`credential_id`. `booking_id` is the canonical on-chain booking reference used
throughout the contract (`BookingKey { booking_contract, booking_id }`); the
provider's opaque `booking_ref` field is available through the fetched
`Booking` record for application display but is intentionally not persisted
or emitted by SBT (DESIGN.md section 3, `docs/sbt-contract-interface.md`) —
`booking_id` is the identifier this event exposes for indexing. No review
content, personal identifiers, dispute evidence or secrets are included.

### `SbtInitialized` — topics `["sbt", "initialized"]`

| Field | Type | Encoding |
|---|---|---|
| booking_contract | Address | Data |
| mint_authority | Address | Data |
| upgrade_authority | Address | Data |

Emitted exactly once, from the constructor, when a new SBT instance is
configured. `mint_authority` and `upgrade_authority` must be distinct.

### `ContractUpgraded` — topics `["contract", "upgraded"]`

| Field | Type | Encoding |
|---|---|---|
| new_wasm_hash | BytesN of 32 | Data |
| upgraded_at | u64 | Data |

Emitted after a successful `upgrade` call authorized by `upgrade_authority`.

### `UpgradeAuthorityChanged` — topics `["authority", "upgrade_changed"]`

| Field | Type | Encoding |
|---|---|---|
| previous_authority | Address | Data |
| new_authority | Address | Data |
| changed_at | u64 | Data |

Emitted when `set_upgrade_authority` successfully rotates to a different address.
Idempotent same-authority calls succeed without emitting this event.

## Errors

`Error` is a `#[contracterror]` `repr(u32)` enum with stable, non-reassignable
numeric codes:

| Code | Variant | Meaning |
|---:|---|---|
| 1 | NotInitialized | Configuration absent |
| 2 | InvalidConfiguration | Invalid constructor configuration |
| 3 | BookingNotFound | Provider reports unknown booking |
| 4 | BookingNotCompleted | Current state is not Completed |
| 5 | BookingNotSettled | Settlement has not completed |
| 6 | BookingCancelled | Cancellation history flag set |
| 7 | BookingDisputed | Dispute history flag set |
| 8 | BookingIdentityMismatch | Returned booking ID differs |
| 9 | BookingDependencyFailed | Catchable provider invocation/decode failure other than missing booking |
| 10 | CredentialNotFound | Requested credential ID was never allocated |
| 11 | StorageInvariantViolation | Broken linked records, malformed state or missing initialized counter |
| 12 | CredentialIdOverflow | Checked counter increment fails |
| 13 | AlreadyInitialized | Defensive rejection of repeated initialization |
| 14 | InvalidWasmHash | Upgrade rejected because the WASM hash is all zeros |

Coverage by category (STELLO-35 acceptance criterion: "Errors cover
initialization, authorization, invalid input, duplicate issuance and missing
credentials"):

- Initialization: `NotInitialized`, `AlreadyInitialized`, `InvalidConfiguration`.
- Authorization: no custom `Unauthorized` variant. `require_auth()` fails at
  the host authorization layer before contract logic runs, so an
  authorization failure is an uncatchable host error, not a fabricated typed
  success (DESIGN.md section 13). `unauthorized_mint_is_rejected` and
  `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner`
  exercise this.
- Invalid input: `InvalidConfiguration`, `BookingIdentityMismatch`.
- Duplicate issuance: `mint_for_booking` is idempotent by design — a repeat
  call for an already-issued booking returns the existing `credential_id`
  without a new event, storage write or error (`docs/duplicate-issuance-prevention.md`).
  There is no `AlreadyMinted` error because a duplicate call is not a fault
  condition. `StorageInvariantViolation` still fails closed if the persisted
  pair is inconsistent.
- Missing credentials: `CredentialNotFound`, `BookingNotFound`.

## Test coverage

| Error / event | Test(s) |
|---|---|
| `SbtInitialized` emitted once on construction | `constructor_persists_config_and_emits_event` |
| `SbtMinted` schema (topics + all data fields) | `eligible_booking_mints_and_is_queryable` |
| `SbtMinted` suppressed on retry | `duplicate_mint_is_idempotent_and_does_not_emit_again` |
| No event on any rejected mint | every `try_mint_for_booking` failure test asserts `env.events().all().events().is_empty()` |
| `InvalidConfiguration` (guard logic) | `self_referential_configuration_is_rejected` |
| `InvalidConfiguration` (actual construction panic) | `self_referential_configuration_panics_during_construction` (new) |
| `AlreadyInitialized` (actual re-construction panic) | `reinitializing_configured_instance_panics_with_already_initialized` (new) |
| `NotInitialized` | `uninitialized_queries_fail_closed` |
| `BookingNotFound` | `missing_booking_maps_to_booking_not_found` |
| `BookingNotCompleted` / `BookingNotSettled` / `BookingCancelled` / `BookingDisputed` | `ineligible_bookings_return_typed_errors`, `identity_and_history_precedence_reject_without_writes` |
| `BookingIdentityMismatch` | `identity_and_history_precedence_reject_without_writes` |
| `BookingDependencyFailed` | `provider_failure_leaves_no_issuance`, `provider_decode_failure_leaves_no_issuance` |
| `CredentialNotFound` | `review_eligibility_is_booking_and_owner_specific_without_provider_refetch` |
| `StorageInvariantViolation` | `corrupt_credentials_fail_closed_in_queries_reviews_and_retries`, `reverse_index_and_allocated_range_are_validated` |
| `CredentialIdOverflow` | `counter_overflow_and_occupied_slot_do_not_partially_mint` |
| Unauthorized (host-level, no typed error) | `unauthorized_mint_is_rejected`, `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner` |

The two new tests close the only previously-untested paths: both constructor
panics (`InvalidConfiguration`, `AlreadyInitialized`) were only exercised
indirectly, one through the `valid_configuration` helper and the other not at
all. `self_referential_configuration_panics_during_construction` uses
`env.register_at` with a predetermined, self-referential contract address so
the panic fires during actual construction rather than through the helper.
`reinitializing_configured_instance_panics_with_already_initialized` calls the
already-configured instance's `__constructor` a second time directly and
asserts it panics with `Error(Contract, #13)`, and that configuration is
unaffected by an unrelated assertion covering both.

## Acceptance criterion traceability

| STELLO-35 acceptance criterion (verbatim) | Status | Evidence |
|---|---|---|
| Issuance event includes traveler, booking reference and credential identifier. | Met | `SbtMinted` carries `credential_id` (topic), `traveller` and `booking_id` (the on-chain booking reference; see note above on provider `booking_ref`) |
| Errors cover initialization, authorization, invalid input, duplicate issuance and missing credentials. | Met | See "Coverage by category" above |
| Event payloads support backend indexing. | Met | `credential_id` is a topic (indexed), and `booking_contract`/`booking_id`/`traveller` let an indexer join events to a specific booking and wallet without a contract call |
| Schemas are documented. | Met | This document; DESIGN.md section 13 |
| Event and error behavior is tested. | Met | See "Test coverage" table; `cargo test` — 23 passed, 0 failed |

## Verification

Run from the repository root:

```sh
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
stellar contract build
python3 scripts/check_sbt_interface.py
```

This document adds no new entrypoint and no event/error schema change; it
adds two regression tests and records STELLO-35 acceptance evidence for the
existing event/error interface. It does not claim STELLO-13
integration/deployment evidence, which remains out of scope per DESIGN.md
section 15.
