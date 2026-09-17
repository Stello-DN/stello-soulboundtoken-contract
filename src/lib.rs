#![no_std]

use soroban_sdk::{
    Address, BytesN, Env, IntoVal, InvokeError, Symbol, contract, contracterror, contractevent,
    contractimpl, contracttype, vec,
};

pub const INSTANCE_TTL_THRESHOLD: u32 = 100_000;
pub const INSTANCE_TTL_EXTEND_TO: u32 = 500_000;
pub const PERSISTENT_TTL_THRESHOLD: u32 = 100_000;
pub const PERSISTENT_TTL_EXTEND_TO: u32 = 500_000;

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
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub booking_contract: Address,
    pub mint_authority: Address,
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

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Config,
    NextCredentialId,
    Credential(u64),
    Issuance(BookingKey),
}

#[contractevent(topics = ["sbt", "minted"])]
pub struct SbtMinted {
    pub credential_id: u64,
    pub booking_contract: Address,
    pub booking_id: u64,
    pub traveller: Address,
    pub issued_at: u64,
    pub schema_version: u32,
}

#[contractevent(topics = ["sbt", "initialized"])]
pub struct SbtInitialized {
    pub booking_contract: Address,
    pub mint_authority: Address,
}

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

#[contract]
pub struct StelloSbtEngine;

#[contractimpl]
impl StelloSbtEngine {
    pub fn __constructor(env: Env, booking_contract: Address, mint_authority: Address) {
        if env.storage().instance().has(&DataKey::Config) {
            soroban_sdk::panic_with_error!(&env, Error::AlreadyInitialized);
        }
        if !valid_configuration(&env, &booking_contract, &mint_authority) {
            soroban_sdk::panic_with_error!(&env, Error::InvalidConfiguration);
        }
        mint_authority.require_auth();
        env.storage().instance().set(
            &DataKey::Config,
            &Config {
                booking_contract: booking_contract.clone(),
                mint_authority: mint_authority.clone(),
            },
        );
        env.storage()
            .instance()
            .set(&DataKey::NextCredentialId, &1_u64);
        bump_instance_ttl(&env);
        SbtInitialized {
            booking_contract,
            mint_authority,
        }
        .publish(&env);
    }

    pub fn get_config(env: Env) -> Result<Config, Error> {
        load_config(&env)
    }

    pub fn mint_for_booking(env: Env, booking_id: u64) -> Result<u64, Error> {
        let config = load_config(&env)?;
        config.mint_authority.require_auth();
        let booking_key = BookingKey {
            booking_contract: config.booking_contract.clone(),
            booking_id,
        };

        if let Some(existing_id) = env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::Issuance(booking_key.clone()))
        {
            let credential = env
                .storage()
                .persistent()
                .get::<_, Credential>(&DataKey::Credential(existing_id))
                .ok_or(Error::StorageInvariantViolation)?;
            if credential.credential_id != existing_id
                || credential.booking != booking_key
                || credential.schema_version != 1
            {
                return Err(Error::StorageInvariantViolation);
            }
            extend_persistent(&env, &DataKey::Issuance(booking_key));
            extend_persistent(&env, &DataKey::Credential(existing_id));
            bump_instance_ttl(&env);
            return Ok(existing_id);
        }

        let booking = fetch_booking(&env, &config.booking_contract, booking_id)?;
        validate_booking(&booking, booking_id)?;
        let credential_id = env
            .storage()
            .instance()
            .get::<_, u64>(&DataKey::NextCredentialId)
            .ok_or(Error::StorageInvariantViolation)?;
        if credential_id == 0 {
            return Err(Error::StorageInvariantViolation);
        }
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
        load_config(&env)?;
        if credential_id == 0 {
            return Err(Error::CredentialNotFound);
        }
        env.storage()
            .persistent()
            .get(&DataKey::Credential(credential_id))
            .ok_or(Error::CredentialNotFound)
    }

    pub fn get_credential_by_booking(
        env: Env,
        booking_id: u64,
    ) -> Result<Option<Credential>, Error> {
        let config = load_config(&env)?;
        let key = BookingKey {
            booking_contract: config.booking_contract,
            booking_id,
        };
        let Some(id) = env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::Issuance(key.clone()))
        else {
            return Ok(None);
        };
        let credential = env
            .storage()
            .persistent()
            .get::<_, Credential>(&DataKey::Credential(id))
            .ok_or(Error::StorageInvariantViolation)?;
        if credential.credential_id != id || credential.booking != key {
            return Err(Error::StorageInvariantViolation);
        }
        Ok(Some(credential))
    }
}

fn load_config(env: &Env) -> Result<Config, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Config)
        .ok_or(Error::NotInitialized)
}

fn valid_configuration(env: &Env, booking_contract: &Address, mint_authority: &Address) -> bool {
    booking_contract != &env.current_contract_address()
        && mint_authority != &env.current_contract_address()
}

fn fetch_booking(env: &Env, contract: &Address, booking_id: u64) -> Result<Booking, Error> {
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

fn validate_booking(booking: &Booking, requested_id: u64) -> Result<(), Error> {
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

fn extend_persistent(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND_TO);
}

fn bump_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        Event as _, testutils::Address as _, testutils::Events as _,
        testutils::storage::Persistent as _,
    };

    #[contract]
    pub struct MockBookingContract;

    #[contracttype]
    enum MockKey {
        Booking,
        Failure,
    }

    #[contractimpl]
    impl MockBookingContract {
        pub fn __constructor(env: Env, booking: Option<Booking>) {
            if let Some(booking) = booking {
                env.storage().instance().set(&MockKey::Booking, &booking);
            }
        }

        pub fn get_booking(env: Env, _booking_id: u64) -> Result<Booking, ProviderError> {
            env.storage()
                .instance()
                .get(&MockKey::Booking)
                .ok_or(ProviderError::BookingNotFound)
        }
    }

    fn booking(
        env: &Env,
        id: u64,
        state: BookingState,
        settled: bool,
        cancelled: bool,
        disputed: bool,
        traveller: &Address,
    ) -> Booking {
        Booking {
            amount: 100,
            booking_id: id,
            booking_ref: BytesN::from_array(env, &[1; 32]),
            cancelled_by: CancelledBy::None,
            checked_in: true,
            created_at: 1,
            escrow_amount: 100,
            escrow_locked: true,
            host: Address::generate(env),
            service_ref: BytesN::from_array(env, &[2; 32]),
            settled,
            start_time: 1,
            state,
            token: Address::generate(env),
            traveller: traveller.clone(),
            was_cancelled: cancelled,
            was_disputed: disputed,
        }
    }

    fn setup(env: &Env, record: Booking) -> (StelloSbtEngineClient<'_>, Address, Address) {
        let provider = env.register(MockBookingContract, (Some(record),));
        let authority = Address::generate(env);
        let sbt = env.register(StelloSbtEngine, (&provider, &authority));
        (StelloSbtEngineClient::new(env, &sbt), provider, authority)
    }

    #[test]
    fn constructor_persists_config_and_emits_event() {
        let env = Env::default();
        let booking_contract = Address::generate(&env);
        let mint_authority = Address::generate(&env);
        let sbt = env.register(StelloSbtEngine, (&booking_contract, &mint_authority));
        let client = StelloSbtEngineClient::new(&env, &sbt);
        assert_eq!(
            client.get_config(),
            Config {
                booking_contract: booking_contract.clone(),
                mint_authority: mint_authority.clone()
            }
        );
        let expected = SbtInitialized {
            booking_contract,
            mint_authority,
        };
        env.as_contract(&sbt, || expected.publish(&env));
        assert_eq!(env.events().all().events(), [expected.to_xdr(&env, &sbt)]);
    }

    #[test]
    fn same_address_configuration_is_allowed() {
        let env = Env::default();
        let address = Address::generate(&env);
        let sbt = env.register(StelloSbtEngine, (&address, &address));
        assert_eq!(
            StelloSbtEngineClient::new(&env, &sbt)
                .get_config()
                .booking_contract,
            address
        );
    }

    #[test]
    fn self_referential_configuration_is_rejected() {
        let env = Env::default();
        let booking_contract = Address::generate(&env);
        let mint_authority = Address::generate(&env);
        let sbt = env.register(StelloSbtEngine, (&booking_contract, &mint_authority));
        env.as_contract(&sbt, || {
            assert!(!valid_configuration(&env, &sbt, &mint_authority));
            assert!(!valid_configuration(&env, &booking_contract, &sbt));
        });
    }

    #[test]
    fn mint_authority_auth_invocation_is_recorded() {
        let env = Env::default();
        let authority = Address::generate(&env);
        let sbt = env.register(StelloSbtEngine, (&Address::generate(&env), &authority));
        env.mock_all_auths();
        env.as_contract(&sbt, || authority.require_auth());
        assert_eq!(env.auths()[0].0, authority);
    }

    #[test]
    fn eligible_booking_mints_and_is_queryable() {
        let env = Env::default();
        let traveller = Address::generate(&env);
        let record = booking(
            &env,
            7,
            BookingState::Completed,
            true,
            false,
            false,
            &traveller,
        );
        let (client, provider, authority) = setup(&env, record);
        let id = client.mock_all_auths().mint_for_booking(&7);
        let issued_at = env.ledger().timestamp();
        let expected_event = SbtMinted {
            credential_id: id,
            booking_contract: provider.clone(),
            booking_id: 7,
            traveller: traveller.clone(),
            issued_at,
            schema_version: 1,
        };
        assert_eq!(
            env.events().all().events(),
            [expected_event.to_xdr(&env, &client.address)]
        );
        let credential = client.get_credential(&id);
        assert_eq!(credential.owner, traveller);
        assert_eq!(
            credential.booking,
            BookingKey {
                booking_contract: provider,
                booking_id: 7
            }
        );
        assert_eq!(client.get_credential_by_booking(&7), Some(credential));
        let ttl = env.as_contract(&client.address, || {
            env.storage().persistent().get_ttl(&DataKey::Credential(id))
        });
        assert_eq!(ttl, PERSISTENT_TTL_EXTEND_TO);
        let _ = authority;
    }

    #[test]
    fn duplicate_mint_is_idempotent_and_does_not_emit_again() {
        let env = Env::default();
        let traveller = Address::generate(&env);
        let record = booking(
            &env,
            1,
            BookingState::Completed,
            true,
            false,
            false,
            &traveller,
        );
        let (client, _, _) = setup(&env, record);
        let first = client.mock_all_auths().mint_for_booking(&1);
        let second = client.mock_all_auths().mint_for_booking(&1);
        assert_eq!(first, second);
        assert_eq!(env.events().all().events().len(), 0);
    }

    #[test]
    fn ineligible_bookings_return_typed_errors() {
        for (state, settled, cancelled, disputed, expected) in [
            (
                BookingState::CheckedIn,
                true,
                false,
                false,
                Error::BookingNotCompleted,
            ),
            (
                BookingState::Completed,
                false,
                false,
                false,
                Error::BookingNotSettled,
            ),
            (
                BookingState::Completed,
                true,
                true,
                false,
                Error::BookingCancelled,
            ),
            (
                BookingState::Completed,
                true,
                false,
                true,
                Error::BookingDisputed,
            ),
        ] {
            let env = Env::default();
            let traveller = Address::generate(&env);
            let record = booking(&env, 1, state, settled, cancelled, disputed, &traveller);
            let (client, _, _) = setup(&env, record);
            assert_eq!(
                client
                    .mock_all_auths()
                    .try_mint_for_booking(&1)
                    .unwrap_err(),
                Ok(expected)
            );
        }
    }

    #[test]
    fn missing_booking_maps_to_booking_not_found() {
        let env = Env::default();
        let authority = Address::generate(&env);
        let provider = env.register(MockBookingContract, (None::<Booking>,));
        let sbt = env.register(StelloSbtEngine, (&provider, &authority));
        let client = StelloSbtEngineClient::new(&env, &sbt);
        assert_eq!(
            client
                .mock_all_auths()
                .try_mint_for_booking(&9)
                .unwrap_err(),
            Ok(Error::BookingNotFound)
        );
    }

    #[test]
    fn unauthorized_mint_is_rejected() {
        let env = Env::default();
        let traveller = Address::generate(&env);
        let record = booking(
            &env,
            1,
            BookingState::Completed,
            true,
            false,
            false,
            &traveller,
        );
        let (client, _, _) = setup(&env, record);
        env.set_auths(&[]);
        assert!(client.try_mint_for_booking(&1).is_err());
    }
}
