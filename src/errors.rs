//! Contract errors for StelloSbtEngine.
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    InvalidConfiguration = 2,
    BookingNotFound = 3,
    BookingNotCompleted = 4,
    BookingNotSettled = 5,
    BookingCancelled = 6,
    BookingDisputed = 7,
    BookingIdentityMismatch = 8,
    BookingDependencyFailed = 9,
    CredentialNotFound = 10,
    StorageInvariantViolation = 11,
    CredentialIdOverflow = 12,
    AlreadyInitialized = 13,
    InvalidWasmHash = 14,
    ReviewAlreadySubmitted = 15,
    InvalidReviewHash = 16,
}
