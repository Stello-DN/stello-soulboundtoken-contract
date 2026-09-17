# Regression and security test suite

STELLO-36 requires a consolidated regression and security test suite for the
SBT contract: shared fixtures, additional edge cases and regression coverage
beyond the basic tests already delivered with STELLO-30 through STELLO-35.
This ticket adds one previously-unexercised regression test
(`mint_and_config_fail_closed_without_initialization`) and records the full
suite's coverage against the DESIGN.md section 16 test strategy and the
Jira acceptance criteria below. No contract behavior changes.

## Shared fixtures

All tests share one fixture set in `src/lib.rs` (`#[cfg(test)] mod tests`),
avoiding duplicated setup across functional tickets:

| Fixture | Purpose |
|---|---|
| `test_env()` | `Env::default()` with snapshot capture disabled |
| `MockBookingContract` | Generated-shape provider stub; returns a configured `Booking` or a configured `ProviderError`, or `BookingNotFound` when neither is set |
| `booking(..)` | Builds a fully-specified `Booking` record with explicit state/settled/cancelled/disputed flags |
| `eligible(env, id)` | Shorthand for a `Completed`, settled, non-cancelled, non-disputed booking |
| `setup(env, record)` | Registers the mock provider and a configured `StelloSbtEngine` instance, returns `(client, provider, authority)` |
| `assert_unissued(env, client, booking_id)` | Asserts no event, no credential, no issuance index and an unchanged counter after a rejected mint |
| `StorageAttacker` | Separate contract used to prove SBT's persistent keys are not writable by another contract despite identical key shapes |

New tests reuse these fixtures rather than re-deriving booking records or
provider setup, per the ticket's "does not duplicate basic tests" scope note.

## Coverage against DESIGN.md section 16 ("Test strategy")

| Area | Required evidence | Test(s) |
|---|---|---|
| Construction/auth | Authenticated atomic constructor | `constructor_persists_config_and_emits_event`, `mint_authority_auth_invocation_is_recorded` |
| | Unauthorized construction/mint/retry fail | `self_referential_configuration_panics_during_construction`, `unauthorized_mint_is_rejected`, `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner` |
| | No initialize entrypoint; repeated construction rejected | `reinitializing_configured_instance_panics_with_already_initialized` |
| | Missing Config and counter behavior | `uninitialized_queries_fail_closed`, **`mint_and_config_fail_closed_without_initialization` (new)**, `corrupt_credentials_fail_closed_in_queries_reviews_and_retries` (counter corruption cases) |
| | Invalid source | `self_referential_configuration_is_rejected`, `self_referential_configuration_panics_during_construction` |
| Eligibility | Successful exact predicate | `eligible_booking_mints_and_is_queryable` |
| | Each other state | `ineligible_bookings_return_typed_errors` |
| | Unsettled Completed | `ineligible_bookings_return_typed_errors` |
| | Identity mismatch | `identity_and_history_precedence_reject_without_writes` |
| | Each historical flag; resolved dispute remains rejected | `ineligible_bookings_return_typed_errors`, `identity_and_history_precedence_reject_without_writes` (cancellation/dispute are permanent historical flags independent of current state) |
| Ownership | Owner derived from generated provider traveller | `eligible_booking_mints_and_is_queryable` |
| | No recipient/source override | `transfer_and_approval_calls_cannot_reassign_owner_even_with_auth`, `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner` |
| | Same traveller with distinct bookings | `review_eligibility_is_booking_and_owner_specific_without_provider_refetch` |
| Atomicity | Forced failure leaves no credential/index/counter advance/event | `provider_failure_leaves_no_issuance`, `provider_decode_failure_leaves_no_issuance`, `identity_and_history_precedence_reject_without_writes`, `counter_overflow_and_occupied_slot_do_not_partially_mint` |
| | Earlier settlement unchanged | `retry_requires_auth_and_extends_both_keys_but_reads_do_not` |
| Idempotency | Repeat returns same ID and payload; one event | `duplicate_mint_is_idempotent_and_does_not_emit_again` |
| | Competing/unauthorized retries converge safely | `retry_requires_auth_and_extends_both_keys_but_reads_do_not`, `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner` |
| Integrity | Missing indexed credential, reverse-index mismatch, invalid schema/ID/counter, occupied allocation key, overflow | `corrupt_credentials_fail_closed_in_queries_reviews_and_retries`, `reverse_index_and_allocated_range_are_validated`, `counter_overflow_and_occupied_slot_do_not_partially_mint` |
| Queries | Both lookup paths agree; unknown ID/booking semantics | `review_eligibility_is_booking_and_owner_specific_without_provider_refetch` |
| | Correct/wrong wallet and booking; errors never authorize reviews | `review_eligibility_is_booking_and_owner_specific_without_provider_refetch`, `corrupt_credentials_fail_closed_in_queries_reviews_and_retries` |
| Non-transferability | ABI lacks transfer/approval/allowance/operator-transfer and owner-changing paths | `transfer_and_approval_calls_cannot_reassign_owner_even_with_auth` |
| TTL/restore | Threshold behavior; pair extension | `eligible_booking_mints_and_is_queryable`, `retry_requires_auth_and_extends_both_keys_but_reads_do_not` |
| | Independent index/credential expiry; retry never remints | `retry_requires_auth_and_extends_both_keys_but_reads_do_not` |
| Real integration | Pinned provider WASM, real completion/settlement, cancellation/dispute rejection, hash/ABI mismatch | Out of scope for STELLO-36 (AGENTS.md: "This issue implements the SBT contract itself. Cross-contract integration, final deployment and evidence belong to Epic 3 (STELLO-13)"); tracked as a remaining risk below |
| Review service | Authenticated reviewer/database uniqueness; SBT remains intact after review | Out of scope — external review service, not part of this repository |

## Security-focused tests

| Threat | Test(s) |
|---|---|
| Forged storage from another contract using identical key shapes | `another_contract_cannot_overwrite_sbt_storage_with_identical_keys` |
| Owner reassignment via transfer/approve/owner-setter/burn-shaped calls, even with full auth | `transfer_and_approval_calls_cannot_reassign_owner_even_with_auth` |
| Unauthorized mint by owner or an unrelated wallet | `unauthorized_mint_is_rejected`, `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner` |
| Provider traveller swapped between mint and retry | `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner` |
| Corrupted/partial persistent state (7 corruption modes) fails closed instead of returning unverified data | `corrupt_credentials_fail_closed_in_queries_reviews_and_retries` |
| Forged or out-of-range reverse index | `reverse_index_and_allocated_range_are_validated` |
| Counter overflow / occupied allocation slot | `counter_overflow_and_occupied_slot_do_not_partially_mint` |
| Operating on an uninitialized instance (queries, mint, config) | `uninitialized_queries_fail_closed`, `mint_and_config_fail_closed_without_initialization` |

## Acceptance criterion traceability

| STELLO-36 acceptance criterion (verbatim) | Status | Evidence |
|---|---|---|
| Initialization and repeated initialization are tested. | Met | `constructor_persists_config_and_emits_event`, `reinitializing_configured_instance_panics_with_already_initialized` |
| Authorized and unauthorized minting are tested. | Met | `mint_authority_auth_invocation_is_recorded`, `unauthorized_mint_is_rejected`, `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner` |
| Invalid traveler and booking references are tested. | Met | `identity_and_history_precedence_reject_without_writes` (booking ID mismatch), `missing_booking_maps_to_booking_not_found` (unknown booking), `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner` (traveller swap) |
| Duplicate issuance and retries are tested. | Met | `duplicate_mint_is_idempotent_and_does_not_emit_again`, `retry_requires_auth_and_extends_both_keys_but_reads_do_not` |
| Non-transferability is tested. | Met | `transfer_and_approval_calls_cannot_reassign_owner_even_with_auth` |
| Ownership and review eligibility queries are tested. | Met | `review_eligibility_is_booking_and_owner_specific_without_provider_refetch` |
| Storage behavior is tested where supported by the simulator. | Met | `corrupt_credentials_fail_closed_in_queries_reviews_and_retries`, `reverse_index_and_allocated_range_are_validated`, `counter_overflow_and_occupied_slot_do_not_partially_mint`, persistent TTL assertions in `eligible_booking_mints_and_is_queryable` and `retry_requires_auth_and_extends_both_keys_but_reads_do_not` |
| The full suite passes using documented commands. | Met | See Verification below — `cargo test` — 24 passed, 0 failed |
| Covers shared fixtures, additional edge cases and regression; does not duplicate basic tests. | Met | "Shared fixtures" above; one new regression test closes the only remaining gap (missing-Config path for `mint_for_booking`/`get_config`) |

## Verification

Run from the repository root:

```sh
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
stellar contract build
```

Results recorded for this change: `cargo test` — 24 passed, 0 failed (23
pre-existing plus `mint_and_config_fail_closed_without_initialization`).

This document adds one regression test and records STELLO-36 acceptance
evidence for the existing suite; it does not claim STELLO-13
integration/deployment evidence (real pinned-provider-WASM integration tests),
which remains out of scope per DESIGN.md section 17 and AGENTS.md.
