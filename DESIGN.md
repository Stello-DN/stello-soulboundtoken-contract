# StelloSbtEngine design

Status: architecture specification for STELLO-29; no implementation or deployment claim.
Baseline date: 17 September 2026. The task owner's explicit approved decisions
are recorded below. Release evidence remains subject to section 17.

## 1. Scope, sources and non-goals

Issue: [STELLO-29](https://stellodn.atlassian.net/browse/STELLO-29),
parent [STELLO-12](https://stellodn.atlassian.net/browse/STELLO-12).
Their descriptions, acceptance criteria, status and comments were read through
Atlassian MCP. Workflow status and comments are maintained in Jira and are not
hard-coded here. The Epic calls for a
non-transferable credential linked to completed bookings for review authorization;
it has no separate acceptance-criteria list. STELLO-29 assigns cross-contract
integration, final deployment and evidence to STELLO-13.

Other inputs: AGENTS.md, docs/booking-contract-interface.md v0.2, and the locally
available Stello_SBT_BA_Specification_v0.1.md (STELLO-BA-SBT-001, Draft). The
older BA specification's eligibility summary and AGENTS.md's older eligibility
summary (which omitted `was_disputed`) are superseded for this design by the
verified Booking Contract interface and the approved STELLO-29 baseline: any
prior dispute permanently excludes a booking. Constructor initialization,
idempotent success, the storage schema and TTL baseline are also explicitly
approved. These decisions supersede those older eligibility summaries without
silently resolving unrelated source conflicts or labeling the entire BA approved.

Scope: one immutable Proof-of-Experience credential for the recorded traveller
of each eligible booking; authenticated issuance, durable uniqueness, reads,
booking-specific review eligibility and issuance events. Settlement must already
have succeeded in a separate transaction.

Non-goals: escrow, payment, refund, settlement, review content, review consumption,
marketplace, transfer, approval, allowance, operator transfer, burn, reissuance,
owner migration, group participants, revocation, pause, mutable metadata,
NFT-wallet compatibility, SBT upgrade entrypoints and automatic migration.
No Booking-side SBT minted flag is required.

Repository baseline: Cargo.toml is an edition-2024 scaffold with no dependencies;
src/lib.rs contains a template function/test. README.md, tests/ and an existing
DESIGN.md were absent. This document adds no contract implementation.

## 2. Booking dependency and pinned provenance

| Item | Baseline |
|---|---|
| Provider | StelloBookingContract |
| Version | 0.1.1 |
| Network | Stellar Testnet, deployment reported by task owner |
| Repository | vudoancs/stello-booking-contract |
| WASM filename | stello_booking_contract.wasm |
| WASM SHA-256 | `90f7e80462bdd6ca9b18f3b2b02c31a7ee467e47b4fb9b996874ccf50ffcd86f` |
| Provider tests | 138 passed, 0 failed, supplied verification baseline; not rerun for this document |
| Provider commit/release tag | Must be recorded with artifact provenance before release |
| Deployed contract ID and transaction/ledger | Must be recorded and verified before SBT deployment |

The existing local WASM hash was independently checked and its embedded interface
inspected. That verifies local bytes and ABI, not their deployment or source-build
provenance. Version 0.1.1 alone is insufficient to identify the updated artifact.

The only Booking entrypoint required by SBT execution is:

```rust
get_booking(booking_id: u64) -> Result<Booking, Error>
```

The source signature additionally takes `env: Env`. Use a client and provider
types generated from this exact WASM specification. Do not manually recreate
Booking, BookingState or provider Error. Pin the artifact in a reproducible build
input and reject a hash mismatch before compiling the consumer.

| Required provider data | Actual type |
|---|---|
| booking_id | u64 |
| traveller | soroban_sdk::Address |
| state | generated BookingState; Completed has discriminant 3 |
| settled | bool |
| was_cancelled | bool |
| was_disputed | bool |

Successful lookup establishes existence. Provider BookingNotFound is error code
5. Other invocation/decode errors cannot be interpreted as an absent booking.
No supplementary Booking method or write operation is needed.

`booking_id` is the canonical lookup key supplied to
`get_booking(booking_id)`. The returned Booking also contains `booking_ref`, a
stable business/reference value. The SBT persists the canonical
`BookingKey { booking_contract, booking_id }` inside each Credential and in the
Issuance uniqueness index; it does not copy provider fields into that key. The
`booking_ref` remains provider-owned and is available through the returned
Booking for application display or evidence, but it is not an alternative SBT
lookup key or a second uniqueness namespace. No new Booking method is implied.

Use no_std and the same resolved soroban-sdk and Rust toolchain as the pinned
provider build. Its manifest declares soroban-sdk "27.0.1", a version requirement,
not an exact build pin. Record the resolved lockfile version, Rust version,
target, CLI version and build flags with the artifact; do not infer them from the
manifest or local machine alone.

## 3. Credential identity and uniqueness

The uniqueness namespace is `(booking_contract, booking_id)`. The Booking address
is fixed in Config. A sequential u64 credential ID is local to this SBT deployment;
application references also include network and SBT contract ID. Transaction
hashes are evidence, not credential identity.

IDs start at 1. NextCredentialId stores the next unused ID. Compute its successor
with checked_add before any issuance write; overflow fails the transaction.
Never reuse IDs, delete issuance history, reset the counter or infer a counter
from backend records. Schema version starts at 1. issued_at is the successful
mint ledger timestamp in Unix seconds.

## 4. Public contract interface

The following SBT signatures omit the implicit Rust `env: Env` parameter:

```rust
__constructor(booking_contract: Address, mint_authority: Address)
get_config() -> Result<Config, SbtError>
mint_for_booking(booking_id: u64) -> Result<u64, SbtError>
get_credential(credential_id: u64) -> Result<Credential, SbtError>
get_credential_by_booking(booking_id: u64) -> Result<Option<Credential>, SbtError>
is_review_eligible(booking_id: u64, traveller: Address) -> Result<bool, SbtError>
```

Constructor failures abort deployment; there is no separately callable initialize
method. Reads require initialized state but no caller authentication. No caller
can pass an alternative source, owner or eligibility facts. Public reads perform
no writes or TTL bumps.

get_credential returns CredentialNotFound for ID 0 or a never-allocated ID. An
allocated ID whose credential is missing is StorageInvariantViolation. Validate
the loaded credential's ID, schema, configured source and reverse index before
returning it. get_credential_by_booking returns None only for authoritative
never-issued absence under sections 8 and 12; a broken pair is an error.

## 5. Data structures and storage keys

These are SBT-owned schema definitions, not copies of provider types:

```rust
struct Config {
    booking_contract: Address,
    mint_authority: Address,
}

struct BookingKey {
    booking_contract: Address,
    booking_id: u64,
}

struct Credential {
    credential_id: u64,
    booking: BookingKey,
    owner: Address,
    issued_at: u64,
    schema_version: u32,
}

enum DataKey {
    Config,
    NextCredentialId,
    Credential(u64),
    Issuance(BookingKey),
}
```

| Key | Value | Storage class |
|---|---|---|
| Config | Config | Instance |
| NextCredentialId | u64 | Instance |
| Credential(id) | Credential | Persistent |
| Issuance(booking) | u64 credential ID | Persistent |

Config and counter share the instance lifetime; each persistent record has its
own lifetime. No temporary storage is used for authoritative state. Config and
credential payloads are immutable after their initial creation.

## 6. Constructor and authorization

Deploy atomically with the two approved constructor arguments. Require the
configured mint_authority's authentication during construction and every mint,
including an idempotent retry. Constructor arguments must be bound into the
authorized deployment. There is no publicly uninitialized deployment window.

Reject self-reference as Booking source and require a contract address for the
source. Deployment tooling verifies its executable against the pinned artifact;
address shape alone does not establish trust. The authority must support Soroban
authentication; it can be an account or an appropriately authenticated contract.
Authentication failure is a host error. Invalid configuration aborts with
InvalidConfiguration. Store Config and NextCredentialId = 1 atomically and
maintain instance/code TTL. Constructor lifecycle rules prevent reinitialization.

All public methods require Config; genuine absence yields NotInitialized.
Once initialized, a missing, zero or malformed counter is an integrity failure,
never a reason to default to 1. Archived instance data follows section 12.

The backend/operator signs and pays for mint submission. It cannot determine
on-chain eligibility, override the recipient or submit eligibility booleans.
There is no configuration/authority rotation method in the baseline.

## 7. Booking eligibility validation

For a new issuance, fetch from Config.booking_contract and require exactly:

```rust
booking.booking_id == requested_booking_id
    && booking.state == BookingState::Completed
    && booking.settled
    && !booking.was_cancelled
    && !booking.was_disputed
```

Always derive owner from booking.traveller. Created, Escrowed, CheckedIn,
Cancelled and Disputed states are ineligible. Completed without settlement is
ineligible. Cancellation and dispute flags are permanent exclusions, even after
the current state returns to Completed and funds settle. Refund ratios and
escrow_locked are not substitute eligibility checks.

For deterministic error reporting, check identity, cancellation history, dispute
history, Completed state, then settlement. A cancelled/disputed history takes
precedence over a current-state error after identity is validated.

## 8. Atomic and idempotent minting

1. Load initialized Config/counter; authenticate mint_authority.
2. Construct BookingKey from Config and the requested ID; read persistent Issuance.
3. If indexed, load Credential and validate both directions, ID range, schema and
   namespace. Return its ID after extending both TTLs and instance/code TTL.
   Do not refetch Booking, change payloads/counter or emit an issuance event.
4. If authoritatively never issued, call the generated Booking client using its
   fallible invocation path. Map known provider missing-booking errors separately
   from other dependency/decode failures; preserve uncatchable host failures.
5. Validate section 7 and derive owner. Compute the next counter with checked
   arithmetic. The target Credential key must be absent; never overwrite it.
6. Store Credential and Issuance and advance the counter in the same invocation.
   Extend both persistent TTLs and instance/code TTL; emit SBTMinted; return ID.

Any failure aborts the invocation and rolls back issuance writes and events.
Booking settlement is a previously confirmed, separate transaction and remains
unchanged. No SBT path invokes payment or settlement.

Validate an indexed credential's credential_id equals the indexed ID, booking
equals the requested BookingKey, schema_version equals 1, and ID is nonzero and
below NextCredentialId. Reverse lookups must resolve to the same ID. Missing or
mismatched linked records are StorageInvariantViolation, never an automatic
repair or a second issuance.

Safety assumes only these atomic paths can create records and no path deletes
them. An orphan credential with a physically deleted index cannot be discovered
by a constant-time booking lookup using this schema alone. Archival is not such
deletion. Arbitrary corruption/migration is outside the supported state model;
detected inconsistencies must halt operational mint submissions pending review,
not be repaired by resubmission. See section 19.

## 9. Duplicate and concurrent requests

All workers contend on the same issuance key and counter. Committed transactions
must observe consistent ledger state: only one creates the pair/event. Another
request returns the existing ID, or encounters a transaction conflict requiring
resimulation and retry; it must never create a second credential.

After an unknown transaction outcome, reconcile transaction status and
authoritative credential state before retrying mint. Do not repeat settlement.
Backend job uniqueness improves operations but is not the safety mechanism.
Different eligible bookings for one traveller receive distinct credentials.

## 10. Non-transferability

Expose no transfer, approve, allowance or operator-transfer entrypoints, and no
ordinary owner update, burn, reissue or SBT upgrade entrypoint. The owner field is
written only during issuance. Authentication by an owner or mint authority does
not grant reassignment power. Non-transferability makes no claim about control
of an external wallet being sold or compromised.

## 11. Persistent storage and TTL policy

Use threshold 100,000 ledgers and extend-to 500,000 ledgers for Credential and
Issuance. Extend both on creation and successful mint retries. Use the same
baseline for instance/code maintenance. Ledger counts are not fixed durations.
Check target-network limits during release validation; do not silently weaken
the approved baseline if unsupported.

Periodic operational transactions extend both persistent keys plus instance/code
before archival; maintain a rebuildable inventory from events and confirmed
queries. Extending instance/code alone does not extend independent persistent
entries. Public read simulation is not committed retention work. Booking's
get_booking is a pure read and does not maintain its own booking TTL on behalf
of SBT; provider retention/restoration is also an operational dependency.

## 12. Archived-entry restoration

RPC/cache absence, simulation failure and archival are never evidence of no
prior issuance. Read authoritative persistent keys inside contract execution.
A never-created key can be absent after successful authoritative access; an
archived key must be restored or execution must fail. Never catch an archival
or dependency failure and substitute None, false or a fresh counter.

For networks supporting automatic restoration, simulate the complete invocation
and include the required restore list/resources and fees in the submitted
transaction. Archived persistent/instance entries are restored before execution
when included; omission must fail closed. Where needed, restore explicitly and
resimulate. Restore both sides of the issuance pair, instance/code, and required
Booking dependencies, then read and validate again. Restore failure creates no
credential and grants no review authorization.

Restoration is an operator/network transaction procedure, not an invented SBT or
Booking restore method. Keep original records; never recreate them from a cache.
Verify this procedure with the pinned SDK and target network before release.
Protocol reference: [Stellar state archival](https://developers.stellar.org/docs/learn/fundamentals/contract-development/storage/state-archival).

## 13. Events and stable typed errors

SBTMinted uses topics `["sbt", "minted", credential_id]`. Its complete logical
schema is:

| Field | Type | Encoding |
|---|---|---|
| credential_id | u64 | Topic |
| booking_contract | Address | Data |
| booking_id | u64 | Data |
| traveller | Address | Data; equals credential.owner |
| issued_at | u64 | Data; equals credential.issued_at |
| schema_version | u32 | Data; initially 1 |

Emit exactly once for new committed issuance; none on retry or failure. Do not
include review content, personal identifiers, dispute evidence or secrets.

Define SbtError with contracterror and repr(u32). Preserve these numeric codes:

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

No custom Unauthorized error: require_auth fails at the host authorization
layer. Uncatchable host/resource/archival errors remain host failures, not
fabricated typed successes. No AlreadyMinted error. Future codes must be added
without reassigning existing values. Constructor errors abort deployment.

## 14. Review-eligibility query semantics

is_review_eligible returns true only if an internally consistent credential
exists for the exact configured-source booking and credential.owner equals the
supplied traveller. Return false for a genuinely never-issued booking or owner
mismatch. Return an error/fail closed for uninitialized, inconsistent, unavailable
or unrestorable state; never grant access based on stale cached ownership.

This checks issued credentials, not whether an unminted booking could qualify.
It does not revalidate Booking or retroactively revoke issued credentials. The
baseline has no revocation and does not consume credentials when used.

The review service independently authenticates wallet control and applies a
database uniqueness constraint for one review per booking/traveller. Passing an
Address does not prove control. A credential for another booking does not qualify.

### Privacy and consent

Minting creates a permanent public link between the traveller wallet, the
configured Booking Contract and a booking identifier. Before the mint request is
submitted, the application must obtain explicit, informed traveller consent for
that public Proof-of-Experience credential and its booking linkage. The SBT
cannot enforce an off-chain consent screen, so the backend must record the
consent decision and avoid submitting a mint without it; that operational record
is not an authority to bypass on-chain eligibility.

On-chain data is minimized to the Credential schema and required event fields.
Do not write names, email addresses, precise location, chat, review text,
payment details, raw commercial terms or other PII to SBT storage or events.
The application must disclose that the credential is public, non-transferable,
booking-specific and may reveal experience history to anyone who can inspect the
ledger. External presentation metadata, if later approved, must preserve the
same minimization and integrity rules.

The MVP has no burn, revocation, owner migration or consent-withdrawal method.
If a traveller withdraws consent after issuance, the application must stop new
uses or presentation of the credential where policy permits, while preserving
the immutable on-chain record; it cannot erase the public linkage. The exact
withdrawal, suppression, support and legal-retention policy remains unresolved
for production and requires Product Owner and privacy approval. No off-chain
flag may be treated as an on-chain revocation or alter `is_review_eligible` in
this baseline.

## 15. Trust boundaries and upgrades

Booking owns lifecycle, traveller and irreversible cancellation/dispute history.
SBT owns issuance, immutable ownership and duplicate protection. The worker only
submits transactions and maintains operational projections. Completion remains
dependent on Booking authorization and its operational process; an SBT does not
independently prove physical attendance or review truthfulness.

Booking exposes authenticated upgrade. Its upgrade authority can change behavior
at the same address, so address pinning and imported ABI types do not freeze
semantics. Deployment tooling must verify the executable hash and operators must
monitor upgrades. On a change, stop worker submissions, inspect/pin the new
artifact, regenerate the client and rerun integration tests before resuming.
There is no on-chain pause or runtime hash-enforcement method in this design;
monitoring cannot eliminate the interval between detection and an upgrade.

A second SBT deployment has separate storage and cannot automatically share
uniqueness. Migration/redeployment requires separately approved reconciliation
and application routing; never reset identity by deploying another registry.

## 16. Test strategy

| Area | Required evidence |
|---|---|
| Construction/auth | Authenticated atomic constructor; unauthorized construction/mint/retry fail; no initialize entrypoint; missing Config and counter behavior; invalid source |
| Eligibility | Successful exact predicate; each other state; unsettled Completed; identity mismatch; each historical flag; resolved dispute remains rejected |
| Ownership | Owner derived from generated provider traveller; no recipient/source override; same traveller with distinct bookings |
| Atomicity | Forced failure leaves no credential/index/counter advance/event; earlier settlement unchanged |
| Idempotency | Repeat returns same ID and payload; one event; competing transactions and lost response converge safely |
| Integrity | Missing indexed credential, reverse-index mismatch, invalid schema/ID/counter, occupied allocation key and overflow fail closed |
| Queries | Both lookup paths agree; unknown ID/booking semantics; correct/wrong wallet and booking; errors never authorize reviews |
| Non-transferability | ABI lacks transfer/approval/allowance/operator-transfer and owner-changing paths |
| TTL/restore | Threshold behavior; pair extension; independent index/credential expiry; instance/code and provider restoration; failure when restore omitted; retry never remints |
| Real integration | Invoke pinned provider WASM; real completion and settlement; cancellation and dispute-resolution rejection; hash/ABI mismatch blocks release |
| Review service | Authenticated reviewer and database uniqueness; SBT remains intact after review |

Use generated provider types even in SBT unit fixtures. Mocks supplement rather
than replace real-WASM integration. Prove counter/index invariants across every
reachable mutation path. Use network tests for competing transactions and actual
archival/resource handling; unit tests alone are insufficient release evidence.

After contract implementation, run and report exact results:

```bash
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
stellar contract build
```

No contract tests/builds are claimed for this documentation task. The supplied
138-test provider result is not an SBT test result.

## 17. Release and deployment evidence

Before SBT release, retain provider repository, commit/tag, pinned WASM/hash,
resolved dependencies/toolchain and generated ABI. Verify deployed Booking
address/network/executable hash against the baseline and record deployment
transaction/ledger. Check historical bookings were created or migrated under a
schema with reliable irreversible dispute history; never assume missing history
means false.

Retain SBT source commit, test reports, optimized WASM/hash and ABI, deployment
address/network and transaction, constructor arguments and authentication
evidence. Include successful mint, unauthorized/ineligible rejection, idempotent
retry, lookup/review eligibility, concurrent-request and archival/restore evidence.
Record observed fees/resource limits and operational retention responsibility.
Source verification on an explorer is distinct from deployment evidence.

STELLO-13 owns integration/deployment evidence as stated in STELLO-29. Do not
claim implementation acceptance or mark Jira Done based on this document alone.
No secrets, signing material, internal commercial terms or private user data
belong in this document or release evidence.

## 18. Jira acceptance-criteria traceability

| STELLO-29 acceptance criterion (verbatim) | Design sections | Verification target |
|---|---|---|
| Credential contains traveler address and a stable booking reference. | 3, 5, 7 | Credential.owner and BookingKey from trusted provider |
| Credential identifiers and uniqueness rules are documented. | 3, 5, 8, 9 | Sequential IDs, atomic pair, checked counter, idempotency |
| Minting authority and completion-validation trust boundary are documented. | 6, 7, 15 | Authentication, provider-derived facts and upgrade trust |
| Storage keys and TTL/restore strategy are defined. | 5, 11, 12 | Storage classes, paired TTLs and restore-or-fail behavior |
| The mint eligibility policy specifies handling of cancelled and disputed bookings. | 7, 16 | Permanent rejection through both history flags |
| Contract interface is documented without inventing unsupported methods. | 2, 4, 13, 14 | Only verified get_booking dependency; separate proposed SBT ABI |

All six architectural criteria are specified. This is a documentation readiness
assessment, not a Jira status transition or assertion that implementation tests
have passed. Next implementation issue:
[STELLO-30](https://stellodn.atlassian.net/browse/STELLO-30), initialization and
minting authorization, implemented through the approved constructor model.

## 19. Remaining risks and approval boundaries

- Provider commit/tag, deployed identity/transaction and reproducible toolchain
  provenance remain release evidence requirements, not missing eligibility data.
- Confirm historical schema migration preserves dispute history before allowing
  pre-update bookings. Unsupported or ambiguous records must fail closed.
- The approved minimal storage model protects reachable state through atomicity,
  non-deletion and restoration. It does not provide constant-time discovery of
  arbitrary orphan records after destructive corruption or a future migration.
  Stronger corruption recovery requires a separately approved schema/recovery
  design; never add silent repair or claim such detection is already guaranteed.
- Approve operational retention ownership, fee funding, monitoring cadence and
  recovery objectives before release; the 100,000/500,000 ledger baseline itself
  is approved. Verify network resource compatibility.
- Immutable mint authority means lost-key recovery/rotation is unavailable.
  Rotation, emergency pause, revocation or SBT upgradeability require separate
  approval and threat-model review; none is implicitly included here.
- Booking upgrades remain trusted. On-chain executable-hash enforcement would
  require a separately approved interface/configuration change.
- Resolve public travel-history disclosure and consent/refusal policy before
  production; there is no recipient opt-out endpoint in this baseline.
- Broader BA approvals and optional NFT/metadata/group-credential features are
  not implied by this task. The approved dispute policy, constructor, uniqueness,
  idempotency, schema and TTL decisions are not reopened by older draft wording.

STELLO-29 is fully specified for the approved baseline. Release is gated by the
evidence and operational items above; implementation remains separate work.
