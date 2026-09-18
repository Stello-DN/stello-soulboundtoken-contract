# SBT non-transferability interface

STELLO-32 implements the regression evidence for DESIGN.md section 10 and
BA BR-08 / FR-SBT-06. The existing contract enforces immutable ownership by
omitting every ownership-changing entrypoint. WASM upgrades are authorized
separately via `upgrade_authority` and do not reassign credential owners.

| Entrypoint | Ownership behavior |
|---|---|
| `__constructor(booking_contract, mint_authority, upgrade_authority)` | Configures addresses during deployment; requires mint and upgrade authority auth; rejects `mint_authority == upgrade_authority`; cannot reinitialize an existing credential registry. |
| `get_config()` | Reads configuration. |
| `mint_for_booking(booking_id)` | Requires mint-authority authorization. New issuance derives the owner from the trusted Booking Contract's `traveller`. Retry returns the existing credential and never replaces its owner. |
| `get_credential(credential_id)` | Reads a validated credential. |
| `get_credential_by_booking(booking_id)` | Reads a validated credential using the configured Booking Contract and booking ID. |
| `is_review_eligible(booking_id, traveller)` | Compares the supplied address with the stored owner; does not modify ownership or prove wallet control. |
| `upgrade(new_wasm_hash)` | Requires upgrade-authority authorization. Replaces contract WASM; does not mutate credential ownership. Rejects an all-zero hash. |
| `set_upgrade_authority(new_upgrade_authority)` | Requires current upgrade-authority authorization. Rotates upgrade authority; rejects equality with mint authority. |
| `contract_version()` | Read-only compile-time package version; no auth or storage writes. |

The contract has no transfer, transfer-from, approval, operator, owner setter,
burn, or revocation entrypoint. Neither the traveler nor the mint authority can
reassign a credential. Mint authority cannot upgrade unless separately configured
as upgrade authority (constructor rejects that configuration). Changing the
provider's traveler after issuance cannot change the already stored owner through
a mint retry. No transferable-token or NFT-wallet compatibility is promised.

Soroban storage is scoped to the executing contract. A different contract can
write an identical key in its own storage, but cannot overwrite this SBT's
credential. Test-only `env.as_contract` access is fixture machinery, not an
on-chain entrypoint available to an attacker.

The canonical reference is `BookingKey { booking_contract, booking_id }`.
Provider `booking_ref` is not persisted or indexed by SBT. Ownership remains
bound to the originally issued wallet; this does not prevent external loss,
sale or compromise of that wallet's signing credentials.

## Verification

Run from the repository root:

```sh
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
stellar contract build
python3 scripts/check_sbt_interface.py
```

The interface check reads the just-built WASM specification through Stellar CLI
and requires exactly the approved entrypoints and their input signatures.
An added ownership-changing method or recipient override fails the check and
requires review. It complements behavior tests; matching function names alone
does not prove safe implementations. Run it after rebuilding, never against a
stale artifact.

| Acceptance criterion | Executable evidence |
|---|---|
| No public owner reassignment or transfer/approval API | Built-WASM interface check; `transfer_and_approval_calls_cannot_reassign_owner_even_with_auth` exercises rejected calls with all requested authorization granted. |
| Unauthorized mutation cannot change owner | `another_contract_cannot_overwrite_sbt_storage_with_identical_keys` exercises contract storage isolation; `changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner` rejects retries authorized only by the owner or another wallet. |
| Ownership remains bound to recipient | Both tests compare original credential and BookingKey lookup; changed-provider test also verifies authorized retry and review eligibility for original/replacement wallets. |
| Non-transferability documented | This interface document and DESIGN.md section 10. |

Native tests and local WASM inspection do not establish deployed network,
archival/restoration or real-provider integration evidence. Those remain in
STELLO-13 scope. This ticket does not deploy a contract.
