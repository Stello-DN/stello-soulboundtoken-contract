//! Booking-contract ABI mirrors and cross-contract helpers.
use soroban_sdk::{
    Address, BytesN, Env, IntoVal, InvokeError, Symbol, contracterror, contracttype, vec,
};

use crate::errors::Error;

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookingState {
    Created = 0,
    Escrowed = 1,
    CheckedIn = 2,
    Completed = 3,
    Cancelled = 4,
    Disputed = 5,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancelledBy {
    None = 0,
    Traveller = 1,
    Host = 2,
}

/// Exact provider ABI record required to decode `get_booking`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Booking {
    pub amount: i128,
    pub booking_id: u64,
    pub booking_ref: BytesN<32>,
    pub cancelled_by: CancelledBy,
    pub checked_in: bool,
    pub created_at: u64,
    pub escrow_amount: i128,
    pub escrow_locked: bool,
    pub host: Address,
    pub service_ref: BytesN<32>,
    pub settled: bool,
    pub start_time: u64,
    pub state: BookingState,
    pub token: Address,
    pub traveller: Address,
    pub was_cancelled: bool,
    pub was_disputed: bool,
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ProviderError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidStateTransition = 4,
    BookingNotFound = 5,
    InvalidAmount = 6,
    InvalidAddress = 7,
    InvalidToken = 8,
    EscrowAlreadyLocked = 9,
    EscrowNotLocked = 10,
    AlreadySettled = 11,
    AlreadyCancelled = 12,
    InvalidDispute = 13,
    InvalidBpsAllocation = 14,
    MathError = 15,
    InvalidUpdate = 16,
    EscrowExceedsAmount = 17,
    Insolvent = 18,
    InvalidStartTime = 19,
    SettlementNotFound = 20,
    DuplicateBookingRef = 21,
}

pub fn fetch_booking(env: &Env, contract: &Address, booking_id: u64) -> Result<Booking, Error> {
    let result = env.try_invoke_contract::<Booking, ProviderError>(
        contract,
        &Symbol::new(env, "get_booking"),
        vec![env, booking_id.into_val(env)],
    );
    match result {
        Ok(Ok(booking)) => Ok(booking),
        Ok(Err(_)) => Err(Error::BookingDependencyFailed),
        Err(Ok(ProviderError::BookingNotFound)) => Err(Error::BookingNotFound),
        Err(Ok(_)) | Err(Err(InvokeError::Abort | InvokeError::Contract(_))) => {
            Err(Error::BookingDependencyFailed)
        }
    }
}

pub fn validate_booking(booking: &Booking, requested_id: u64) -> Result<(), Error> {
    if booking.booking_id != requested_id {
        return Err(Error::BookingIdentityMismatch);
    }
    if booking.was_cancelled {
        return Err(Error::BookingCancelled);
    }
    if booking.was_disputed {
        return Err(Error::BookingDisputed);
    }
    if booking.state != BookingState::Completed {
        return Err(Error::BookingNotCompleted);
    }
    if !booking.settled {
        return Err(Error::BookingNotSettled);
    }
    Ok(())
}
