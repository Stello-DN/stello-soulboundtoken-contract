# Stello SBT Contract

Soroban **soulbound** (non-transferable) Proof-of-Experience credential for Stello.
Issues one immutable credential per eligible booking, bound to the traveller recorded
by a trusted Booking Contract.

Configuration is set **atomically at deploy** via `__constructor`. There is no
post-deploy `initialize`. WASM upgrades are authorized only by a distinct
`upgrade_authority` (never automatically by `mint_authority`).

## Identifiers

| Field | Meaning |
|-------|---------|
| `credential_id` | Sequential ID local to **this** SBT deployment |
| `booking_id` | Internal Booking Contract ID (provider authority) |
| `BookingKey` | Canonical uniqueness key: `{ booking_contract, booking_id }` |

Provider `booking_ref` is **not** stored or indexed by SBT.

## Public interface

Approved entrypoints (enforced by `scripts/check_sbt_interface.py`):

| Function | Purpose |
|----------|---------|
| `__constructor(booking_contract, mint_authority, upgrade_authority)` | Atomic config; requires mint + upgrade authority auth; rejects mint == upgrade |
| `get_config` | Read config (RPC simulation; no auth / TTL bump) |
| `mint_for_booking(booking_id)` | Issue or return existing credential; mint-authority auth |
| `get_credential(credential_id)` | Read credential by id |
| `get_credential_by_booking(booking_id)` | Read credential by booking |
| `is_review_eligible(booking_id, traveller)` | Owner match and not-yet-reviewed check for review gating |
| `mark_reviewed(booking_id, review_hash)` | Persist review proof; credential-owner auth |
| `get_review_status(booking_id)` | Read optional ReviewStatus |
| `extend_instance_ttl` | Permissionless instance (+ code) TTL maintenance |
| `extend_credential_ttl(booking_id)` | Permissionless credential entry TTL maintenance |
| `upgrade(new_wasm_hash)` | Replace WASM; upgrade-authority auth; rejects all-zero hash |
| `set_upgrade_authority(new_upgrade_authority)` | Rotate upgrade authority; current upgrade-authority auth |
| `contract_version` | Compile-time package version string (read-only) |

There is **no** transfer, transfer-from, approval, burn, revoke, or owner-setter API.

## Roles

| Actor | Actions |
|-------|---------|
| Mint authority | Authorize `__constructor` and every `mint_for_booking` |
| Upgrade authority | Authorize `__constructor`, `upgrade`, and `set_upgrade_authority` |
| Traveller (credential owner) | Authorize `mark_reviewed`; credential remains permanent |
| Anyone | `get_config`, `get_credential*`, `is_review_eligible`, `get_review_status`, `contract_version`, `extend_instance_ttl`, `extend_credential_ttl` |

## Requirements

- Rust with `wasm32v1-none` (`rustup target add wasm32v1-none`)
- [Stellar CLI](https://developers.stellar.org/docs/tools/cli) (`stellar`) — CI pins **28.0.0**
- Soroban SDK 27.x

## Source layout

```
src/
├── lib.rs       # StelloSbtEngine entrypoints
├── types.rs     # Config, Credential, ReviewStatus, ...
├── errors.rs
├── events.rs
├── storage.rs   # DataKey, TTL, load/store helpers
├── provider.rs  # Booking ABI mirror + fetch/validate
└── test.rs
```

## Commands

```bash
make test       # cargo test
make build      # stellar contract build
make fmt
make clippy
make interface  # build + scripts/check_sbt_interface.py
make check      # fmt + clippy + test + interface
```

## CI/CD & Testnet Deployment

GitHub Actions automates checks and Testnet **first deploy**. **Mainnet is not automated.**
After first deploy, lock the registry with `STELLO_TESTNET_CONTRACT_ID`. WASM updates
use on-chain `upgrade()` under `upgrade_authority` (manual operational path; not this workflow).

### What runs on push / PR

On `pull_request` and pushes to `dev` / `main` (and on Testnet tags), the **CI** workflow runs:

1. `cargo fmt --all -- --check`
2. `cargo test`
3. `cargo clippy --all-targets -- -D warnings`
4. Optimized WASM build with provenance meta (`source_repo`, `commit_sha`)
5. Interface check via `scripts/check_sbt_interface.py` (approved entrypoints only)
6. Structured meta check + `sha256sum` == `stellar contract info hash --wasm`
7. WASM artifact upload

A normal push to `dev` **never deploys**.

### How to create a Testnet release

From the commit you want deployed:

```bash
git tag -a v0.2.0-testnet.X -m "SBT Testnet Release X"
git push origin v0.2.0-testnet.X
```

Tag pattern required: `v*-testnet.*` (example: `v0.2.0-testnet.1`).

### What the Deploy Testnet workflow does

On `v*-testnet.*` tags, Environment **`dev`** drives **first deploy only**:

| `STELLO_TESTNET_CONTRACT_ID` | Behavior |
|------------------------------|----------|
| unset / empty | `DEPLOY` — new Contract ID with constructor args |
| whitespace-only | **FAIL** |
| set to a live ID | **FAIL** — refuses automatic redeploy of a second registry (use `upgrade()` manually) |

Gates:

1. Exact tagged commit checkout
2. `fmt` / `test` / `clippy`
3. One optimized WASM build + provenance meta
4. Local interface + meta checks
5. `sha256sum` == `stellar contract info hash --wasm`
6. GitHub build provenance attestation (**soft** — continues if org plan lacks Artifact Attestations)
7. Deploy with constructor args; envelope signed by **deployer**, constructor `mint_authority.require_auth()` and `upgrade_authority.require_auth()` signed via `--auth-mode non-root` + `--auto-sign`
8. On-chain verification (interface, `get_config`, three-way wasm hash)
9. Artifacts + GitHub prerelease + Step Summary

Concurrency: group `stello-sbt-testnet-contract` with `cancel-in-progress: false`.

Limitation: the workflow **cannot** write `STELLO_TESTNET_CONTRACT_ID` back into the GitHub Environment — set it manually from the run Summary after first deploy to lock the registry.

### Build provenance (GitHub Attestation)

When available, the release creates a GitHub **build provenance attestation** for the exact WASM (SEP-55 `source_repo=github:<owner>/<repo>`).

For private repos on plans without Artifact Attestations, attestation is **skipped** and deploy continues. Chain hash checks remain required. Making the repo public or upgrading the org plan enables full Build Verified.

### Where to find the Contract ID

- GitHub Actions run **Summary**
- GitHub Release notes for the tag
- Artifact / release file `deployment/testnet.json` (`contract_id`) — generated only; **not committed** (`/deployment/` is gitignored)

### Environment `dev` configuration

**Variables**

| Name | Required | Meaning |
|------|----------|---------|
| `BOOKING_CONTRACT` | yes | Trusted Booking Contract ID (`C…`) on Testnet |
| `MINT_AUTHORITY` | yes | Address authorized to mint (`G…`) |
| `UPGRADE_AUTHORITY` | yes | Address authorized to upgrade (`G…`); must differ from `MINT_AUTHORITY` |
| `STELLO_TESTNET_CONTRACT_ID` | after first deploy | SBT Contract ID — set to **lock** registry and block auto-redeploy |

**Secrets**

| Name | Required | Meaning |
|------|----------|---------|
| `STELLAR_TESTNET_SECRET_KEY` | yes | Deployer / fee payer (`S…`) for first deploy |
| `MINT_AUTHORITY_SECRET_KEY` | yes | Secret for `MINT_AUTHORITY` — must sign `__constructor` (`mint_authority.require_auth()`). Never printed. |
| `UPGRADE_AUTHORITY_SECRET_KEY` | yes | Secret for `UPGRADE_AUTHORITY` — must sign `__constructor` (`upgrade_authority.require_auth()`). Never printed. |

Notes:

- Deployer may differ from mint authority. Do **not** use `--sign-with-key mint-authority` as the only envelope signer when they differ (causes `TxBadAuth`).
- Secrets and variables must live on Environment **`dev`**, not only at repository level.
- `pull_request` / push to `dev` / `main` never deploy. Only `refs/tags/v*-testnet.*`.
- Deploy job permissions: `contents: write`, `id-token: write`, `attestations: write`.
- Testnet releases do **not** deploy Mainnet.

## Manual Deploy (testnet)

Build first (`make build`). Constructor requires **mint authority** and
**upgrade authority** auth (non-root under create-contract):

```bash
# Identities (examples)
stellar keys generate deployer --network testnet --fund
stellar keys generate mint-authority --network testnet --fund
stellar keys generate upgrade-authority --network testnet --fund

make build

stellar contract deploy \
  --wasm target/wasm32v1-none/release/stello_sbt_contract.wasm \
  --source-account deployer \
  --auth-mode non-root \
  --auto-sign \
  --network testnet \
  -- \
  --booking_contract <BOOKING_CONTRACT_ID> \
  --mint_authority <MINT_AUTHORITY_G_ADDRESS> \
  --upgrade_authority <UPGRADE_AUTHORITY_G_ADDRESS>
```

Ensure the CLI can sign as `mint-authority` and `upgrade-authority` (identities
present / `--auto-sign`). Do **not** call a separate `initialize` — that entrypoint
does not exist. For Mainnet, prefer a multisig or timelock contract as
`upgrade_authority`.

Do not commit real production addresses or secret keys into docs or scripts.

## Design & evidence docs

| Doc | Topic |
|-----|-------|
| `docs/booking-contract-interface.md` | Booking provider ABI / trust boundary |
| `docs/sbt-contract-interface.md` | Non-transferability interface evidence |
| `docs/duplicate-issuance-prevention.md` | Duplicate mint prevention |
| `docs/credential-and-eligibility-queries.md` | Query / eligibility reads |
| `docs/events-and-errors.md` | Events and errors |
| `docs/regression-security-test-suite.md` | Consolidated regression suite |

`DESIGN.md` and `AGENTS.md` are kept local (gitignored) and are not part of the published repo.
