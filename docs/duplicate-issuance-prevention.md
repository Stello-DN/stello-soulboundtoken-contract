# Duplicate credential issuance prevention

STELLO-33 requires that repeated issuance for the same eligible booking cannot
create a second credential or a second successful mint event. The mechanism is
the approved STELLO-29 baseline (DESIGN.md sections 3, 8, 9, 11, 13), already
implemented by `mint_for_booking` in STELLO-31 and unchanged here.

## Uniqueness key

The uniqueness namespace is `BookingKey { booking_contract, booking_id }`
(DESIGN.md section 3). `Config.booking_contract` is fixed at construction, so
in practice the key is `booking_id`. Owner (traveller) is derived from the
trusted Booking Contract at issuance time and stored on the Credential; it is
not part of the uniqueness key, so a changed provider traveller cannot bypass
or duplicate an existing issuance for the same booking
(`changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner`).

The `Issuance(BookingKey)` persistent record is the authoritative index. A
booking that is already indexed can never reach the new-issuance path in
`mint_for_booking`, regardless of retry, corruption, or authorization state.

## Resolved conflict: "rejected with a typed error"

STELLO-33's acceptance criteria, taken literally, call for repeated issuance
to return a typed error (e.g. an `AlreadyMinted` variant). The approved
DESIGN.md baseline explicitly rejects that shape: section 13 states "No
AlreadyMinted error," and section 8 requires a retry to return the existing
credential ID as success while extending both persistent TTLs.

This is not stylistic. In Soroban, a contract invocation that returns `Err`
rolls back every storage write made during that invocation, including TTL
extensions. An error-returning duplicate-mint path could not also refresh the
`Credential`/`Issuance` TTLs on retry, silently removing the keep-alive
mechanism DESIGN.md section 11 relies on ("periodic operational transactions
extend both persistent keys ... before archival").

Per AGENTS.md ("If Jira, BA documentation, and code conflict, stop and report
the conflict"), this was raised and resolved in favor of the approved design:
duplicate issuance is prevented through idempotent, side-effect-free success
(no new credential, no new event, existing ID returned, TTLs extended) rather
than a rejection error. This satisfies the intent of "repeated issuance is
rejected" — no second credential or mint ever occurs — without discarding the
TTL keep-alive property. No code change was required; STELLO-31's
implementation already conforms to this decision.

## Acceptance criterion traceability

| STELLO-33 acceptance criterion (verbatim) | Status | Evidence |
|---|---|---|
| The documented booking/traveler uniqueness key is enforced. | Met | DESIGN.md section 3; `Issuance(BookingKey)` index; `eligible_booking_mints_and_is_queryable` |
| Repeated issuance is rejected with a typed error. | Superseded by approved design (see above) | Duplicate mint is a no-op success instead of an error, by DESIGN.md sections 8/9/13/11 |
| Retry does not create another credential or another successful mint event. | Met | `duplicate_mint_is_idempotent_and_does_not_emit_again`; `retry_requires_auth_and_extends_both_keys_but_reads_do_not`; `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner`; `counter_overflow_and_occupied_slot_do_not_partially_mint` |
| Different valid bookings can issue separate credentials. | Met | `review_eligibility_is_booking_and_owner_specific_without_provider_refetch` mints distinct credentials for booking IDs 1 and 2 |
| Duplicate and retry tests pass. | Met | `cargo test` — 21 passed, 0 failed |

## Verification

```sh
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
stellar contract build
```

This document adds no contract behavior. It records STELLO-33 acceptance
evidence and the conflict resolution above; it does not claim STELLO-13
integration/deployment evidence, which remains out of scope per DESIGN.md
section 17.
