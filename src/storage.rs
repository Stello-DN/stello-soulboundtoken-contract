//! Storage keys, TTL policy, and helpers.
use soroban_sdk::{Env, contracttype};

use crate::errors::Error;
use crate::types::{BookingKey, Config, Credential, ReviewStatus};

/// If remaining TTL is below this, bump instance/code toward [`INSTANCE_TTL_EXTEND_TO`].
pub const INSTANCE_TTL_THRESHOLD: u32 = 100_000;
pub const INSTANCE_TTL_EXTEND_TO: u32 = 500_000;
pub const PERSISTENT_TTL_THRESHOLD: u32 = 100_000;
pub const PERSISTENT_TTL_EXTEND_TO: u32 = 500_000;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Config,
    NextCredentialId,
    Credential(u64),
    Issuance(BookingKey),
    /// Separate from Credential so existing persisted credentials stay readable
    /// after WASM upgrade without migration.
    ReviewStatus(BookingKey),
}

pub(crate) fn load_next_id(env: &Env) -> Result<u64, Error> {
    env.storage()
        .instance()
        .get::<_, u64>(&DataKey::NextCredentialId)
        .filter(|id| *id != 0)
        .ok_or(Error::StorageInvariantViolation)
}

pub(crate) fn load_credential(
    env: &Env,
    config: &Config,
    id: u64,
    next_id: u64,
) -> Result<Credential, Error> {
    if id == 0 || id >= next_id {
        return Err(Error::StorageInvariantViolation);
    }
    let credential = env
        .storage()
        .persistent()
        .get::<_, Credential>(&DataKey::Credential(id))
        .ok_or(Error::StorageInvariantViolation)?;
    if credential.credential_id != id
        || credential.schema_version != 1
        || credential.booking.booking_contract != config.booking_contract
        || env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::Issuance(credential.booking.clone()))
            != Some(id)
    {
        return Err(Error::StorageInvariantViolation);
    }
    Ok(credential)
}

pub(crate) fn lookup_booking(
    env: &Env,
    config: &Config,
    key: &BookingKey,
    next_id: u64,
) -> Result<Option<Credential>, Error> {
    // Authoritative persistent access: archival host errors are not absence.
    let Some(id) = env
        .storage()
        .persistent()
        .get::<_, u64>(&DataKey::Issuance(key.clone()))
    else {
        return Ok(None);
    };
    let credential = load_credential(env, config, id, next_id)?;
    if credential.booking != *key {
        return Err(Error::StorageInvariantViolation);
    }
    Ok(Some(credential))
}

pub(crate) fn load_config(env: &Env) -> Result<Config, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Config)
        .ok_or(Error::NotInitialized)
}

pub(crate) fn load_review_status(
    env: &Env,
    booking_key: &BookingKey,
) -> Result<Option<ReviewStatus>, Error> {
    let Some(status) = env
        .storage()
        .persistent()
        .get::<_, ReviewStatus>(&DataKey::ReviewStatus(booking_key.clone()))
    else {
        return Ok(None);
    };
    if status.booking != *booking_key {
        return Err(Error::StorageInvariantViolation);
    }
    Ok(Some(status))
}

pub(crate) fn store_review_status(env: &Env, status: &ReviewStatus) {
    let key = DataKey::ReviewStatus(status.booking.clone());
    env.storage().persistent().set(&key, status);
    extend_persistent(env, &key);
}

pub(crate) fn extend_persistent(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND_TO);
}

/// Extend TTL for all persistent entries tied to one credential issuance.
/// Extends `ReviewStatus` only when that entry already exists.
/// Does not write or modify application data.
pub(crate) fn extend_credential_entries(env: &Env, booking_key: &BookingKey, credential_id: u64) {
    extend_persistent(env, &DataKey::Issuance(booking_key.clone()));
    extend_persistent(env, &DataKey::Credential(credential_id));
    let review_key = DataKey::ReviewStatus(booking_key.clone());
    if env.storage().persistent().has(&review_key) {
        extend_persistent(env, &review_key);
    }
}

/// Extend current contract instance and WASM/code TTL when below threshold.
///
/// soroban-sdk 27.x `Instance::extend_ttl` maps to
/// `extend_current_contract_instance_and_code_ttl`, so both instance storage
/// (Config, NextCredentialId) and the contract code entry are renewed.
pub(crate) fn bump_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
}
