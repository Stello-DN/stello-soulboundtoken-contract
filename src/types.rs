//! Domain types for StelloSbtEngine.
use soroban_sdk::{Address, BytesN, contracttype};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub booking_contract: Address,
    pub mint_authority: Address,
    pub upgrade_authority: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookingKey {
    pub booking_contract: Address,
    pub booking_id: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Credential {
    pub credential_id: u64,
    pub booking: BookingKey,
    pub owner: Address,
    pub issued_at: u64,
    pub schema_version: u32,
}

/// Immutable on-chain proof that a review was submitted for a booking.
/// Absence of a record means the booking has not been reviewed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewStatus {
    pub booking: BookingKey,
    pub credential_id: u64,
    pub reviewer: Address,
    pub review_hash: BytesN<32>,
    pub reviewed_at: u64,
}
