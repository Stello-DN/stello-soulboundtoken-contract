# Stello SBT Engine

## Purpose

Implement a non-transferable Proof-of-Experience credential contract on Stellar.

## Authoritative requirements

- Jira Epic: [SOW1] Proof-of-Experience SBT Contract
- BA document: Stello_SBT_BA_Specification_v0.1.md
- Booking Contract interface is the source of booking eligibility data.

If Jira, BA documentation, and code conflict, stop and report the conflict.
Do not silently choose one.

## Architecture boundaries

- Booking Contract owns booking state, traveler, cancellation and settlement.
- SBT Contract owns credential issuance, ownership and duplicate protection.
- Backend triggers minting but cannot choose the recipient or bypass eligibility.
- Booking Contract does not store an SBT minted flag.
- SBT issuance is separate from financial settlement.
- No transfer, approval, marketplace, payment or review-content functionality.
- No revocation, pause, NFT-wallet compatibility or metadata mutability unless
  separately approved.

## Eligibility

A booking is eligible only when:

- state == Completed
- settled == true
- was_cancelled == false
- traveler is obtained from the trusted Booking Contract
- no credential has previously been issued for the booking

## Engineering rules

- Use no_std.
- Use the same Rust and soroban-sdk versions as StelloBookingContract.
- Use checked arithmetic where applicable.
- Require explicit authorization for privileged methods.
- Use typed errors.
- Store the credential and issuance index atomically.
- Never treat missing or archived storage as safe to mint again.
- Do not log secrets or private user data.
- Do not invent unsupported Booking Contract methods.

## Required verification

Run after every implementation:

cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
stellar contract build

Report exact results. Never claim a command passed without running it.

## Git and Jira

- Branch: feature/<JIRA-KEY>-short-description
- Commit: <JIRA-KEY> concise change
- Do not mark a Jira issue Done until acceptance criteria and tests pass.
- Add a Jira comment with summary, files, tests, commit and remaining risks.