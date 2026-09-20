#![no_std]
//! # StelloSbtEngine
//!
//! Soulbound credential minting for completed Stello bookings on Soroban.
//!
//! ## Deployment
//! Configuration is set atomically via [`StelloSbtEngine::__constructor`] at
//! deploy time. There is no separate `initialize` entrypoint and no post-deploy
//! uninitialized window.
//!
//! ## Roles
//! - **Mint authority:** `mint_for_booking`.
//! - **Upgrade authority:** `upgrade`, `set_upgrade_authority`.
//! - **Credential owner (traveller):** `mark_reviewed`.
//!
//! ## Upgradeability
//! `upgrade(new_wasm_hash)` replaces this contract's executable while keeping the
//! same Contract ID and storage. It does **not** migrate schema; future WASMs must
//! remain storage-compatible or ship an explicit migration.

mod errors;
mod events;
mod provider;
mod storage;
mod types;

#[cfg(test)]
mod test;

pub use errors::Error;
pub use events::{
    ContractUpgraded, ReviewMarked, SbtInitialized, SbtMinted, UpgradeAuthorityChanged,
};
pub use provider::{Booking, BookingState, CancelledBy, ProviderError};
pub use storage::{
    INSTANCE_TTL_EXTEND_TO, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND_TO,
    PERSISTENT_TTL_THRESHOLD,
};
pub use types::*;

use provider::{fetch_booking, validate_booking};
use storage::{
    DataKey, bump_instance_ttl, extend_credential_entries, extend_persistent, load_config,
    load_credential, load_next_id, load_review_status, lookup_booking, store_review_status,
};

use soroban_sdk::{Address, BytesN, Env, String, contract, contractimpl, panic_with_error};

#[contract]
pub struct StelloSbtEngine;

#[contractimpl]
impl StelloSbtEngine {
    pub fn __constructor(
        env: Env,
        booking_contract: Address,
        mint_authority: Address,
        upgrade_authority: Address,
    ) {
        if env.storage().instance().has(&DataKey::Config) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        if !valid_configuration(&env, &booking_contract, &mint_authority, &upgrade_authority) {
            panic_with_error!(&env, Error::InvalidConfiguration);
        }
        mint_authority.require_auth();
        upgrade_authority.require_auth();
        env.storage().instance().set(
            &DataKey::Config,
            &Config {
                booking_contract: booking_contract.clone(),
                mint_authority: mint_authority.clone(),
                upgrade_authority: upgrade_authority.clone(),
            },
        );
        env.storage()
            .instance()
            .set(&DataKey::NextCredentialId, &1_u64);
        bump_instance_ttl(&env);
        SbtInitialized {
            booking_contract,
            mint_authority,
            upgrade_authority,
        }
        .publish(&env);
    }

    pub fn get_config(env: Env) -> Result<Config, Error> {
        load_config(&env)
    }

    /// Replace this contract's WASM executable.
    /// **Auth:** `upgrade_authority` only (not `mint_authority`).
    ///
    /// `new_wasm_hash` must already be uploaded on-ledger (`stellar contract upload`).
    /// Contract ID and persistent credential storage are preserved; this is **not**
    /// an automatic schema migration.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.upgrade_authority.require_auth();
        if new_wasm_hash == BytesN::from_array(&env, &[0u8; 32]) {
            return Err(Error::InvalidWasmHash);
        }
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        bump_instance_ttl(&env);
        ContractUpgraded {
            new_wasm_hash,
            upgraded_at: env.ledger().timestamp(),
        }
        .publish(&env);
        Ok(())
    }

    /// Rotate WASM upgrade authority.
    ///
    /// **Auth:** current `upgrade_authority` only.
    /// Rejects `new_upgrade_authority == mint_authority` so operational mint keys
    /// cannot become upgrade keys through rotation.
    ///
    /// Testnet may use a single admin wallet. For Mainnet, prefer configuring a
    /// multisig or timelock contract as `upgrade_authority`. This SBT contract
    /// does not implement key custody, multisig, or timelock logic itself.
    pub fn set_upgrade_authority(env: Env, new_upgrade_authority: Address) -> Result<(), Error> {
        let mut config = load_config(&env)?;
        config.upgrade_authority.require_auth();
        if new_upgrade_authority == config.mint_authority {
            return Err(Error::InvalidConfiguration);
        }
        if new_upgrade_authority == env.current_contract_address() {
            return Err(Error::InvalidConfiguration);
        }
        if new_upgrade_authority == config.upgrade_authority {
            return Ok(());
        }
        let previous_authority = config.upgrade_authority.clone();
        config.upgrade_authority = new_upgrade_authority.clone();
        env.storage().instance().set(&DataKey::Config, &config);
        bump_instance_ttl(&env);
        UpgradeAuthorityChanged {
            previous_authority,
            new_authority: new_upgrade_authority,
            changed_at: env.ledger().timestamp(),
        }
        .publish(&env);
        Ok(())
    }

    /// Compile-time package version string (e.g. `"0.3.0"`).
    /// **Read-only** — no auth, no storage, no TTL mutation.
    pub fn contract_version(env: Env) -> String {
        String::from_str(&env, env!("CARGO_PKG_VERSION"))
    }

    /// Extend contract instance (and WASM/code) TTL using instance constants.
    ///
    /// **Permissionless** — TTL extension cannot change ownership, authority,
    /// review state, or credential state; the transaction submitter pays rent.
    ///
    /// On soroban-sdk 27.x, `storage().instance().extend_ttl` already extends
    /// both the contract instance entry and the linked contract code entry.
    pub fn extend_instance_ttl(env: Env) -> Result<(), Error> {
        bump_instance_ttl(&env);
        Ok(())
    }

    /// Renew TTL for all credential-related persistent entries of a booking.
    ///
    /// Extends `Issuance`, `Credential`, and `ReviewStatus` (when present),
    /// then bumps instance/code TTL in the same transaction.
    ///
    /// **Permissionless** — does not mutate stored business values. An archived
    /// Persistent entry is not treated as absence; host/RPC restoration applies.
    pub fn extend_credential_ttl(env: Env, booking_id: u64) -> Result<(), Error> {
        let config = load_config(&env)?;
        let booking_key = BookingKey {
            booking_contract: config.booking_contract.clone(),
            booking_id,
        };
        let next_id = load_next_id(&env)?;
        let Some(credential) = lookup_booking(&env, &config, &booking_key, next_id)? else {
            return Err(Error::CredentialNotFound);
        };
        extend_credential_entries(&env, &booking_key, credential.credential_id);
        bump_instance_ttl(&env);
        Ok(())
    }

    pub fn mint_for_booking(env: Env, booking_id: u64) -> Result<u64, Error> {
        let config = load_config(&env)?;
        config.mint_authority.require_auth();
        let booking_key = BookingKey {
            booking_contract: config.booking_contract.clone(),
            booking_id,
        };

        let credential_id = load_next_id(&env)?;
        if let Some(credential) = lookup_booking(&env, &config, &booking_key, credential_id)? {
            extend_credential_entries(&env, &booking_key, credential.credential_id);
            bump_instance_ttl(&env);
            return Ok(credential.credential_id);
        }

        let booking = fetch_booking(&env, &config.booking_contract, booking_id)?;
        validate_booking(&booking, booking_id)?;
        let next_id = credential_id
            .checked_add(1)
            .ok_or(Error::CredentialIdOverflow)?;
        let credential = Credential {
            credential_id,
            booking: booking_key.clone(),
            owner: booking.traveller.clone(),
            issued_at: env.ledger().timestamp(),
            schema_version: 1,
        };
        let issuance_key = DataKey::Issuance(booking_key.clone());
        let credential_key = DataKey::Credential(credential_id);
        if env.storage().persistent().has(&credential_key) {
            return Err(Error::StorageInvariantViolation);
        }
        env.storage().persistent().set(&credential_key, &credential);
        env.storage()
            .persistent()
            .set(&issuance_key, &credential_id);
        env.storage()
            .instance()
            .set(&DataKey::NextCredentialId, &next_id);
        extend_persistent(&env, &credential_key);
        extend_persistent(&env, &issuance_key);
        bump_instance_ttl(&env);
        SbtMinted {
            credential_id,
            booking_contract: booking_key.booking_contract,
            booking_id,
            traveller: booking.traveller,
            issued_at: credential.issued_at,
            schema_version: credential.schema_version,
        }
        .publish(&env);
        Ok(credential_id)
    }

    pub fn get_credential(env: Env, credential_id: u64) -> Result<Credential, Error> {
        let config = load_config(&env)?;
        let next_id = load_next_id(&env)?;
        if credential_id == 0 || credential_id >= next_id {
            return Err(Error::CredentialNotFound);
        }
        load_credential(&env, &config, credential_id, next_id)
    }

    pub fn get_credential_by_booking(
        env: Env,
        booking_id: u64,
    ) -> Result<Option<Credential>, Error> {
        let config = load_config(&env)?;
        let next_id = load_next_id(&env)?;
        let key = BookingKey {
            booking_contract: config.booking_contract.clone(),
            booking_id,
        };
        lookup_booking(&env, &config, &key, next_id)
    }

    pub fn is_review_eligible(
        env: Env,
        booking_id: u64,
        traveller: Address,
    ) -> Result<bool, Error> {
        // Read-only: no auth, no TTL bump (same policy as get_credential*).
        // No credential for booking_id → false (not an error).
        let Some(credential) = Self::get_credential_by_booking(env.clone(), booking_id)? else {
            return Ok(false);
        };
        if credential.owner != traveller {
            return Ok(false);
        }
        let key = BookingKey {
            booking_contract: credential.booking.booking_contract,
            booking_id,
        };
        Ok(load_review_status(&env, &key)?.is_none())
    }

    /// Persist immutable on-chain proof that the credential owner submitted a review.
    /// **Auth:** credential owner (traveller) only — not mint/upgrade authority.
    ///
    /// Review body lives off-chain (e.g. MongoDB). `review_hash` must be the
    /// SHA-256 of a deterministic canonical JSON payload (schemaVersion, fixed
    /// field order, UTF-8, normalized newlines; sort mediaHashes if unordered).
    pub fn mark_reviewed(
        env: Env,
        booking_id: u64,
        review_hash: BytesN<32>,
    ) -> Result<ReviewStatus, Error> {
        let config = load_config(&env)?;
        let booking_key = BookingKey {
            booking_contract: config.booking_contract.clone(),
            booking_id,
        };
        let next_id = load_next_id(&env)?;
        let Some(credential) = lookup_booking(&env, &config, &booking_key, next_id)? else {
            return Err(Error::CredentialNotFound);
        };
        credential.owner.require_auth();
        if review_hash == BytesN::from_array(&env, &[0u8; 32]) {
            return Err(Error::InvalidReviewHash);
        }
        if let Some(existing) = load_review_status(&env, &booking_key)? {
            if existing.review_hash == review_hash {
                // Idempotent retry: keep reviewed_at, no duplicate event.
                // Renew all related persistent entries so they stay in sync.
                extend_credential_entries(&env, &booking_key, credential.credential_id);
                bump_instance_ttl(&env);
                return Ok(existing);
            }
            return Err(Error::ReviewAlreadySubmitted);
        }
        let status = ReviewStatus {
            booking: booking_key.clone(),
            credential_id: credential.credential_id,
            reviewer: credential.owner.clone(),
            review_hash: review_hash.clone(),
            reviewed_at: env.ledger().timestamp(),
        };
        store_review_status(&env, &status);
        // Keep Credential / Issuance / ReviewStatus TTL aligned after first review.
        extend_credential_entries(&env, &booking_key, credential.credential_id);
        bump_instance_ttl(&env);
        ReviewMarked {
            credential_id: status.credential_id,
            booking_contract: booking_key.booking_contract,
            booking_id,
            traveller: status.reviewer.clone(),
            review_hash,
            reviewed_at: status.reviewed_at,
        }
        .publish(&env);
        Ok(status)
    }

    /// Read review-submission status for a booking. **Read-only** — no auth / TTL bump.
    pub fn get_review_status(env: Env, booking_id: u64) -> Result<Option<ReviewStatus>, Error> {
        let config = load_config(&env)?;
        let key = BookingKey {
            booking_contract: config.booking_contract,
            booking_id,
        };
        load_review_status(&env, &key)
    }
}

pub(crate) fn valid_configuration(
    env: &Env,
    booking_contract: &Address,
    mint_authority: &Address,
    upgrade_authority: &Address,
) -> bool {
    let self_addr = env.current_contract_address();
    booking_contract != &self_addr
        && mint_authority != &self_addr
        && upgrade_authority != &self_addr
        && mint_authority != upgrade_authority
}
