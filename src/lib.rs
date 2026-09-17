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
    #[topic]
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

        let credential_id = load_next_id(&env)?;
        if let Some(credential) = lookup_booking(&env, &config, &booking_key, credential_id)? {
            extend_persistent(&env, &DataKey::Issuance(booking_key));
            extend_persistent(&env, &DataKey::Credential(credential.credential_id));
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
        Ok(Self::get_credential_by_booking(env, booking_id)?
            .is_some_and(|credential| credential.owner == traveller))
    }
}

fn load_next_id(env: &Env) -> Result<u64, Error> {
    env.storage()
        .instance()
        .get::<_, u64>(&DataKey::NextCredentialId)
        .filter(|id| *id != 0)
        .ok_or(Error::StorageInvariantViolation)
}

fn load_credential(env: &Env, config: &Config, id: u64, next_id: u64) -> Result<Credential, Error> {
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

fn lookup_booking(
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

    fn test_env() -> Env {
        let mut env = Env::default();
        env.set_config(soroban_sdk::testutils::EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        env
    }

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
            if let Some(error) = env.storage().instance().get(&MockKey::Failure) {
                return Err(error);
            }
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
        let env = test_env();
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
        let env = test_env();
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
        let env = test_env();
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
        let env = test_env();
        let authority = Address::generate(&env);
        let sbt = env.register(StelloSbtEngine, (&Address::generate(&env), &authority));
        env.mock_all_auths();
        env.as_contract(&sbt, || authority.require_auth());
        assert_eq!(env.auths()[0].0, authority);
    }

    #[test]
    fn eligible_booking_mints_and_is_queryable() {
        let env = test_env();
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
        let all_events = env.events().all();
        let events = all_events.events();
        assert_eq!(events.len(), 1);
        let soroban_sdk::xdr::ContractEventBody::V0(body) = &events[0].body;
        let topics: soroban_sdk::Vec<soroban_sdk::Val> = vec![
            &env,
            Symbol::new(&env, "sbt").into_val(&env),
            Symbol::new(&env, "minted").into_val(&env),
            id.into_val(&env),
        ];
        assert_eq!(body.topics, topics.into());
        let data: soroban_sdk::Map<Symbol, soroban_sdk::Val> = soroban_sdk::map![
            &env,
            (
                Symbol::new(&env, "booking_contract"),
                provider.clone().into_val(&env)
            ),
            (Symbol::new(&env, "booking_id"), 7_u64.into_val(&env)),
            (
                Symbol::new(&env, "traveller"),
                traveller.clone().into_val(&env)
            ),
            (Symbol::new(&env, "issued_at"), issued_at.into_val(&env)),
            (Symbol::new(&env, "schema_version"), 1_u32.into_val(&env))
        ];
        assert_eq!(body.data, data.into());
        assert_eq!(env.auths()[0].0, authority);
        let credential = client.get_credential(&id);
        assert_eq!(credential.owner, traveller);
        assert_eq!(
            credential.booking,
            BookingKey {
                booking_contract: provider.clone(),
                booking_id: 7
            }
        );
        assert_eq!(client.get_credential_by_booking(&7), Some(credential));
        let ttl = env.as_contract(&client.address, || {
            env.storage().persistent().get_ttl(&DataKey::Credential(id))
        });
        assert_eq!(ttl, PERSISTENT_TTL_EXTEND_TO);
        env.as_contract(&client.address, || {
            assert_eq!(
                env.storage()
                    .persistent()
                    .get_ttl(&DataKey::Issuance(BookingKey {
                        booking_contract: provider,
                        booking_id: 7
                    })),
                PERSISTENT_TTL_EXTEND_TO
            );
        });
    }

    #[test]
    fn duplicate_mint_is_idempotent_and_does_not_emit_again() {
        let env = test_env();
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
                BookingState::Created,
                true,
                false,
                false,
                Error::BookingNotCompleted,
            ),
            (
                BookingState::Escrowed,
                true,
                false,
                false,
                Error::BookingNotCompleted,
            ),
            (
                BookingState::Cancelled,
                true,
                false,
                false,
                Error::BookingNotCompleted,
            ),
            (
                BookingState::Disputed,
                true,
                false,
                false,
                Error::BookingNotCompleted,
            ),
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
            let env = test_env();
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
        let env = test_env();
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
        let env = test_env();
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
        assert_unissued(&env, &client, 1);
    }
    fn eligible(env: &Env, id: u64) -> Booking {
        booking(
            env,
            id,
            BookingState::Completed,
            true,
            false,
            false,
            &Address::generate(env),
        )
    }

    fn assert_unissued(env: &Env, client: &StelloSbtEngineClient, booking_id: u64) {
        assert!(env.events().all().events().is_empty());
        env.as_contract(&client.address, || {
            assert_eq!(load_next_id(env), Ok(1));
            assert!(!env.storage().persistent().has(&DataKey::Credential(1)));
            let config = load_config(env).unwrap();
            assert!(
                !env.storage()
                    .persistent()
                    .has(&DataKey::Issuance(BookingKey {
                        booking_contract: config.booking_contract,
                        booking_id
                    }))
            );
        });
    }

    #[test]
    fn provider_failure_leaves_no_issuance() {
        let env = test_env();
        let (client, provider, _) = setup(&env, eligible(&env, 1));
        env.as_contract(&provider, || {
            env.storage()
                .instance()
                .set(&MockKey::Failure, &ProviderError::NotInitialized);
        });
        assert_eq!(
            client.mock_all_auths().try_mint_for_booking(&1),
            Err(Ok(Error::BookingDependencyFailed))
        );
        assert_unissued(&env, &client, 1);
    }

    #[contract]
    struct MalformedProvider;
    #[contractimpl]
    impl MalformedProvider {
        pub fn get_booking(_env: Env, _booking_id: u64) -> u64 {
            0
        }
    }

    #[test]
    fn provider_decode_failure_leaves_no_issuance() {
        let env = test_env();
        let provider = env.register(MalformedProvider, ());
        let sbt = env.register(StelloSbtEngine, (&provider, &Address::generate(&env)));
        let client = StelloSbtEngineClient::new(&env, &sbt);
        assert_eq!(
            client.mock_all_auths().try_mint_for_booking(&1),
            Err(Ok(Error::BookingDependencyFailed))
        );
        assert_unissued(&env, &client, 1);
    }

    #[test]
    fn identity_and_history_precedence_reject_without_writes() {
        for (requested, cancelled, disputed, expected) in [
            (2, true, true, Error::BookingIdentityMismatch),
            (1, true, true, Error::BookingCancelled),
            (1, false, true, Error::BookingDisputed),
        ] {
            let env = test_env();
            let mut record = eligible(&env, 1);
            record.state = BookingState::Created;
            record.settled = false;
            record.was_cancelled = cancelled;
            record.was_disputed = disputed;
            let (client, _, _) = setup(&env, record);
            assert_eq!(
                client.mock_all_auths().try_mint_for_booking(&requested),
                Err(Ok(expected))
            );
            assert_unissued(&env, &client, requested);
        }
    }

    #[test]
    fn review_eligibility_is_booking_and_owner_specific_without_provider_refetch() {
        let env = test_env();
        let record = eligible(&env, 1);
        let owner = record.traveller.clone();
        let (client, provider, _) = setup(&env, record.clone());
        assert!(!client.is_review_eligible(&1, &owner));
        assert_eq!(
            client.try_get_credential(&0),
            Err(Ok(Error::CredentialNotFound))
        );
        assert_eq!(
            client.try_get_credential(&1),
            Err(Ok(Error::CredentialNotFound))
        );
        let first = client.mock_all_auths().mint_for_booking(&1);
        let mut second_record = record;
        second_record.booking_id = 2;
        env.as_contract(&provider, || {
            env.storage()
                .instance()
                .set(&MockKey::Booking, &second_record)
        });
        let second = client.mock_all_auths().mint_for_booking(&2);
        assert_ne!(first, second);
        assert_eq!(client.get_credential(&second).owner, owner);
        env.as_contract(&provider, || {
            env.storage()
                .instance()
                .set(&MockKey::Failure, &ProviderError::NotInitialized)
        });
        env.set_auths(&[]);
        assert!(client.is_review_eligible(&1, &owner));
        assert!(client.is_review_eligible(&2, &owner));
        assert!(!client.is_review_eligible(&3, &owner));
        assert!(!client.is_review_eligible(&1, &Address::generate(&env)));
        assert_eq!(client.get_credential_by_booking(&3), None);
        assert_eq!(
            client.try_get_credential(&3),
            Err(Ok(Error::CredentialNotFound))
        );
    }

    #[test]
    fn corrupt_credentials_fail_closed_in_queries_reviews_and_retries() {
        for corruption in 0..7 {
            let env = test_env();
            let record = eligible(&env, 1);
            let owner = record.traveller.clone();
            let (client, _, _) = setup(&env, record);
            let id = client.mock_all_auths().mint_for_booking(&1);
            let mut credential = client.get_credential(&id);
            env.as_contract(&client.address, || {
                match corruption {
                    0 => env.storage().persistent().remove(&DataKey::Credential(id)),
                    1 => {
                        credential.credential_id = id + 1;
                    }
                    2 => {
                        credential.schema_version = 2;
                    }
                    3 => {
                        credential.booking.booking_contract = Address::generate(&env);
                    }
                    4 => {
                        credential.booking.booking_id = 2;
                    }
                    5 => env.storage().instance().remove(&DataKey::NextCredentialId),
                    6 => env
                        .storage()
                        .instance()
                        .set(&DataKey::NextCredentialId, &0_u64),
                    _ => unreachable!(),
                }
                if (1..=4).contains(&corruption) {
                    env.storage()
                        .persistent()
                        .set(&DataKey::Credential(id), &credential);
                }
            });
            assert_eq!(
                client.try_get_credential(&id),
                Err(Ok(Error::StorageInvariantViolation))
            );
            assert_eq!(
                client.try_get_credential_by_booking(&1),
                Err(Ok(Error::StorageInvariantViolation))
            );
            assert_eq!(
                client.try_is_review_eligible(&1, &owner),
                Err(Ok(Error::StorageInvariantViolation))
            );
            assert_eq!(
                client.mock_all_auths().try_mint_for_booking(&1),
                Err(Ok(Error::StorageInvariantViolation))
            );
            assert!(env.events().all().events().is_empty());
        }
    }

    #[test]
    fn reverse_index_and_allocated_range_are_validated() {
        for index in [None, Some(0_u64), Some(2_u64)] {
            let env = test_env();
            let (client, provider, _) = setup(&env, eligible(&env, 1));
            let id = client.mock_all_auths().mint_for_booking(&1);
            env.as_contract(&client.address, || {
                let key = DataKey::Issuance(BookingKey {
                    booking_contract: provider,
                    booking_id: 1,
                });
                if let Some(index) = index {
                    env.storage().persistent().set(&key, &index);
                } else {
                    env.storage().persistent().remove(&key);
                }
            });
            assert_eq!(
                client.try_get_credential(&id),
                Err(Ok(Error::StorageInvariantViolation))
            );
            if index.is_some() {
                assert_eq!(
                    client.try_get_credential_by_booking(&1),
                    Err(Ok(Error::StorageInvariantViolation))
                );
                assert_eq!(
                    client.mock_all_auths().try_mint_for_booking(&1),
                    Err(Ok(Error::StorageInvariantViolation))
                );
            }
        }
    }

    #[test]
    fn uninitialized_queries_fail_closed() {
        let env = test_env();
        let (client, _, _) = setup(&env, eligible(&env, 1));
        env.as_contract(&client.address, || {
            env.storage().instance().remove(&DataKey::Config)
        });
        assert_eq!(
            client.try_get_credential(&1),
            Err(Ok(Error::NotInitialized))
        );
        assert_eq!(
            client.try_get_credential_by_booking(&1),
            Err(Ok(Error::NotInitialized))
        );
        assert_eq!(
            client.try_is_review_eligible(&1, &Address::generate(&env)),
            Err(Ok(Error::NotInitialized))
        );
    }

    #[test]
    fn counter_overflow_and_occupied_slot_do_not_partially_mint() {
        for occupied in [false, true] {
            let env = test_env();
            let (client, provider, _) = setup(&env, eligible(&env, 1));
            env.as_contract(&client.address, || {
                if occupied {
                    env.storage()
                        .persistent()
                        .set(&DataKey::Credential(1), &99_u64);
                } else {
                    env.storage()
                        .instance()
                        .set(&DataKey::NextCredentialId, &u64::MAX);
                }
            });
            let expected = if occupied {
                Error::StorageInvariantViolation
            } else {
                Error::CredentialIdOverflow
            };
            assert_eq!(
                client.mock_all_auths().try_mint_for_booking(&1),
                Err(Ok(expected))
            );
            assert!(env.events().all().events().is_empty());
            env.as_contract(&client.address, || {
                assert!(
                    !env.storage()
                        .persistent()
                        .has(&DataKey::Issuance(BookingKey {
                            booking_contract: provider,
                            booking_id: 1
                        }))
                );
                assert_eq!(load_next_id(&env), Ok(if occupied { 1 } else { u64::MAX }));
                if occupied {
                    assert_eq!(
                        env.storage()
                            .persistent()
                            .get::<_, u64>(&DataKey::Credential(1)),
                        Some(99)
                    );
                }
            });
        }
    }

    #[test]
    fn retry_requires_auth_and_extends_both_keys_but_reads_do_not() {
        use soroban_sdk::testutils::Ledger;
        let env = test_env();
        let record = eligible(&env, 1);
        let owner = record.traveller.clone();
        let (client, provider, _) = setup(&env, record);
        let id = client.mock_all_auths().mint_for_booking(&1);
        let original = client.get_credential(&id);
        let keys = [
            DataKey::Credential(id),
            DataKey::Issuance(original.booking.clone()),
        ];
        env.as_contract(&provider, || {
            env.storage()
                .instance()
                .set(&MockKey::Failure, &ProviderError::NotInitialized)
        });
        env.ledger()
            .with_mut(|info| info.sequence_number += 450_001);
        let before = env.as_contract(&client.address, || {
            keys.clone()
                .map(|key| env.storage().persistent().get_ttl(&key))
        });
        assert!(before.iter().all(|ttl| *ttl < PERSISTENT_TTL_THRESHOLD));
        env.set_auths(&[]);
        assert!(client.try_mint_for_booking(&1).is_err());
        assert_eq!(client.get_credential(&id), original);
        assert_eq!(client.get_credential_by_booking(&1), Some(original.clone()));
        assert!(client.is_review_eligible(&1, &owner));
        env.as_contract(&client.address, || {
            assert_eq!(
                keys.clone()
                    .map(|key| env.storage().persistent().get_ttl(&key)),
                before
            );
        });
        assert_eq!(client.mock_all_auths().mint_for_booking(&1), id);
        assert!(env.events().all().events().is_empty());
        assert_eq!(client.get_credential(&id), original);
        env.as_contract(&client.address, || {
            for key in keys {
                assert_eq!(
                    env.storage().persistent().get_ttl(&key),
                    PERSISTENT_TTL_EXTEND_TO
                );
            }
            assert_eq!(load_next_id(&env), Ok(2));
        });
    }
}
