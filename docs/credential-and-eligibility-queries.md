# Credential and review-eligibility query evidence

STELLO-34 requires read-only queries that let the backend verify Proof-of-
Experience review eligibility: retrieve a credential by its identifier, check
eligibility for a traveller/booking pair, and reject use of another wallet's
credential, without mutating ownership or minting.

This is the approved DESIGN.md baseline (sections 3, 4, 14), already
implemented by `get_credential`, `get_credential_by_booking` and
`is_review_eligible` in STELLO-31 and unchanged here. No contract behavior
change is made by this ticket; it records STELLO-34 acceptance evidence
against the existing implementation and test suite. No conflict between
Jira, DESIGN.md and code was found (AGENTS.md conflict-reporting rule).

## Query entrypoints

```rust
get_credential(credential_id: u64) -> Result<Credential, SbtError>
get_credential_by_booking(booking_id: u64) -> Result<Option<Credential>, SbtError>
is_review_eligible(booking_id: u64, traveller: Address) -> Result<bool, SbtError>
```

All three require initialized state but no caller authentication, and perform
no writes or TTL bumps (DESIGN.md section 4). `get_credential` validates the
loaded credential's ID, schema, configured source and reverse index before
returning it, and fails closed (`StorageInvariantViolation`) on any
inconsistency rather than returning unverified data. `is_review_eligible`
delegates to `get_credential_by_booking` and compares the stored owner to the
supplied address; it never re-derives eligibility from the Booking Contract
directly, so it is unaffected by provider unavailability once a credential is
issued.

## Acceptance criterion traceability

| STELLO-34 acceptance criterion (verbatim) | Status | Evidence |
|---|---|---|
| Credential can be retrieved using its documented identifier. | Met | `get_credential(credential_id)`, DESIGN.md sections 3-4; `eligible_booking_mints_and_is_queryable` |
| Eligibility can be checked for a traveler and booking reference. | Met | `is_review_eligible(booking_id, traveller)`, DESIGN.md section 14; `review_eligibility_is_booking_and_owner_specific_without_provider_refetch` |
| Another wallet cannot use the credential as its own proof. | Met | Owner comparison in `is_review_eligible`; `review_eligibility_is_booking_and_owner_specific_without_provider_refetch`, `transfer_and_approval_calls_cannot_reassign_owner_even_with_auth`, `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner`, `another_contract_cannot_overwrite_sbt_storage_with_identical_keys` all assert a non-owner address is ineligible |
| Missing credentials return a documented result or typed error. | Met | `get_credential` returns `CredentialNotFound` for ID 0 or a never-allocated ID; `get_credential_by_booking` returns `Ok(None)` for an authoritative never-issued booking (DESIGN.md section 4); `review_eligibility_is_booking_and_owner_specific_without_provider_refetch`, `uninitialized_queries_fail_closed` |
| Queries do not modify ownership or mint credentials. | Met | No transfer/approval/owner-setter/burn entrypoint exists (`docs/sbt-contract-interface.md`); reads perform no writes or TTL bumps (DESIGN.md section 4); `retry_requires_auth_and_extends_both_keys_but_reads_do_not` captures persistent-key TTLs before and after `get_credential`/`get_credential_by_booking`/`is_review_eligible` calls and asserts they are unchanged |
| Contract eligibility is clearly distinguished from off-chain review submission. | Met | DESIGN.md section 14: on-chain eligibility checks issued credentials only; the review service independently authenticates wallet control and enforces a database uniqueness constraint per booking/traveller; passing an address does not prove control |
| Query and ownership tests pass. | Met | `cargo test` — 21 passed, 0 failed |

## Verification

Run from the repository root:

```sh
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
stellar contract build
python3 scripts/check_sbt_interface.py
```

This document adds no contract behavior. It records STELLO-34 acceptance
evidence for the query interface; it does not claim STELLO-13
integration/deployment evidence, which remains out of scope per DESIGN.md
section 17.
