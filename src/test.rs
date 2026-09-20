#![cfg(test)]

use soroban_sdk::{
    Address, BytesN, Env, Event as _, IntoVal, String, Symbol, contract, contractimpl,
    contracttype, testutils::Address as _, testutils::Events as _,
    testutils::storage::Persistent as _, vec,
};

use crate::storage::{DataKey, load_config, load_next_id};
use crate::{
    Booking, BookingKey, BookingState, CancelledBy, Config, Credential, Error,
    INSTANCE_TTL_EXTEND_TO, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND_TO,
    PERSISTENT_TTL_THRESHOLD, ProviderError, ReviewStatus, SbtInitialized, StelloSbtEngine,
    StelloSbtEngineClient, valid_configuration,
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

fn setup(env: &Env, record: Booking) -> (StelloSbtEngineClient<'_>, Address, Address, Address) {
    let provider = env.register(MockBookingContract, (Some(record),));
    let mint_authority = Address::generate(env);
    let upgrade_authority = Address::generate(env);
    let sbt = env.register(
        StelloSbtEngine,
        (&provider, &mint_authority, &upgrade_authority),
    );
    (
        StelloSbtEngineClient::new(env, &sbt),
        provider,
        mint_authority,
        upgrade_authority,
    )
}

fn register_sbt(env: &Env, booking_contract: &Address, mint_authority: &Address) -> Address {
    let upgrade_authority = Address::generate(env);
    env.register(
        StelloSbtEngine,
        (booking_contract, mint_authority, &upgrade_authority),
    )
}

#[test]
fn constructor_persists_config_and_emits_event() {
    let env = test_env();
    let booking_contract = Address::generate(&env);
    let mint_authority = Address::generate(&env);
    let upgrade_authority = Address::generate(&env);
    let sbt = env.register(
        StelloSbtEngine,
        (&booking_contract, &mint_authority, &upgrade_authority),
    );
    let client = StelloSbtEngineClient::new(&env, &sbt);
    assert_eq!(
        client.get_config(),
        Config {
            booking_contract: booking_contract.clone(),
            mint_authority: mint_authority.clone(),
            upgrade_authority: upgrade_authority.clone(),
        }
    );
    let expected = SbtInitialized {
        booking_contract,
        mint_authority,
        upgrade_authority,
    };
    env.as_contract(&sbt, || expected.publish(&env));
    assert_eq!(env.events().all().events(), [expected.to_xdr(&env, &sbt)]);
}

#[test]
fn same_address_configuration_is_allowed() {
    let env = test_env();
    let address = Address::generate(&env);
    let upgrade_authority = Address::generate(&env);
    let sbt = env.register(StelloSbtEngine, (&address, &address, &upgrade_authority));
    assert_eq!(
        StelloSbtEngineClient::new(&env, &sbt)
            .get_config()
            .booking_contract,
        address
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn mint_equals_upgrade_authority_is_rejected() {
    let env = test_env();
    let booking_contract = Address::generate(&env);
    let authority = Address::generate(&env);
    env.register(StelloSbtEngine, (&booking_contract, &authority, &authority));
}

#[test]
fn self_referential_configuration_is_rejected() {
    let env = test_env();
    let booking_contract = Address::generate(&env);
    let mint_authority = Address::generate(&env);
    let upgrade_authority = Address::generate(&env);
    let sbt = env.register(
        StelloSbtEngine,
        (&booking_contract, &mint_authority, &upgrade_authority),
    );
    env.as_contract(&sbt, || {
        assert!(!valid_configuration(
            &env,
            &sbt,
            &mint_authority,
            &upgrade_authority
        ));
        assert!(!valid_configuration(
            &env,
            &booking_contract,
            &sbt,
            &upgrade_authority
        ));
        assert!(!valid_configuration(
            &env,
            &booking_contract,
            &mint_authority,
            &sbt
        ));
        assert!(!valid_configuration(
            &env,
            &booking_contract,
            &mint_authority,
            &mint_authority
        ));
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn self_referential_configuration_panics_during_construction() {
    let env = test_env();
    let contract_id = Address::generate(&env);
    let mint_authority = Address::generate(&env);
    let upgrade_authority = Address::generate(&env);
    env.register_at(
        &contract_id,
        StelloSbtEngine,
        (&contract_id, &mint_authority, &upgrade_authority),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn reinitializing_configured_instance_panics_with_already_initialized() {
    let env = test_env();
    let booking_contract = Address::generate(&env);
    let mint_authority = Address::generate(&env);
    let upgrade_authority = Address::generate(&env);
    let sbt = env.register(
        StelloSbtEngine,
        (&booking_contract, &mint_authority, &upgrade_authority),
    );
    env.as_contract(&sbt, || {
        StelloSbtEngine::__constructor(
            env.clone(),
            booking_contract,
            mint_authority,
            upgrade_authority,
        );
    });
}

#[test]
fn mint_authority_auth_invocation_is_recorded() {
    let env = test_env();
    let authority = Address::generate(&env);
    let sbt = register_sbt(&env, &Address::generate(&env), &authority);
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
    let (client, provider, authority, _) = setup(&env, record);
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
    let (client, _, _, _) = setup(&env, record);
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
        let (client, _, _, _) = setup(&env, record);
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
    let sbt = register_sbt(&env, &provider, &authority);
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
    let (client, _, _, _) = setup(&env, record);
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
    let (client, provider, _, _) = setup(&env, eligible(&env, 1));
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
    let sbt = register_sbt(&env, &provider, &Address::generate(&env));
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
        let (client, _, _, _) = setup(&env, record);
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
    let (client, provider, _, _) = setup(&env, record.clone());
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
        let (client, _, _, _) = setup(&env, record);
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
        let (client, provider, _, _) = setup(&env, eligible(&env, 1));
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
    let (client, _, _, _) = setup(&env, eligible(&env, 1));
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
    assert_eq!(
        client.try_get_review_status(&1),
        Err(Ok(Error::NotInitialized))
    );
}

#[test]
fn mint_and_config_fail_closed_without_initialization() {
    let env = test_env();
    let (client, _, _, _) = setup(&env, eligible(&env, 1));
    env.as_contract(&client.address, || {
        env.storage().instance().remove(&DataKey::Config)
    });
    assert_eq!(client.try_get_config(), Err(Ok(Error::NotInitialized)));
    assert_eq!(
        client.mock_all_auths().try_mint_for_booking(&1),
        Err(Ok(Error::NotInitialized))
    );
    assert!(env.events().all().events().is_empty());
    env.as_contract(&client.address, || {
        assert!(!env.storage().persistent().has(&DataKey::Credential(1)));
    });
}

#[test]
fn counter_overflow_and_occupied_slot_do_not_partially_mint() {
    for occupied in [false, true] {
        let env = test_env();
        let (client, provider, _, _) = setup(&env, eligible(&env, 1));
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
    let (client, provider, _, _) = setup(&env, record);
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
#[test]
fn transfer_and_approval_calls_cannot_reassign_owner_even_with_auth() {
    let env = test_env();
    let record = eligible(&env, 1);
    let owner = record.traveller.clone();
    let recipient = Address::generate(&env);
    let (client, _, _, _) = setup(&env, record);
    let id = client.mock_all_auths().mint_for_booking(&1);
    let original = client.get_credential(&id);
    // Grant every requested authorization: absence of an ownership-changing
    // entrypoint must protect the credential even from its owner/authority.
    env.mock_all_auths();
    for (name, args) in [
        (
            "transfer",
            vec![
                &env,
                owner.clone().into_val(&env),
                recipient.clone().into_val(&env),
                id.into_val(&env),
            ],
        ),
        (
            "transfer_from",
            vec![
                &env,
                recipient.clone().into_val(&env),
                owner.clone().into_val(&env),
                recipient.clone().into_val(&env),
                id.into_val(&env),
            ],
        ),
        (
            "approve",
            vec![
                &env,
                owner.clone().into_val(&env),
                recipient.clone().into_val(&env),
                id.into_val(&env),
                100_u32.into_val(&env),
            ],
        ),
        (
            "set_owner",
            vec![&env, id.into_val(&env), recipient.clone().into_val(&env)],
        ),
        (
            "burn",
            vec![&env, owner.clone().into_val(&env), id.into_val(&env)],
        ),
    ] {
        let result = env.try_invoke_contract::<soroban_sdk::Val, Error>(
            &client.address,
            &Symbol::new(&env, name),
            args,
        );
        assert!(result.is_err(), "unexpected callable method: {name}");
        assert!(env.events().all().events().is_empty());
        assert_eq!(client.get_credential(&id), original);
        assert_eq!(client.get_credential_by_booking(&1), Some(original.clone()));
        assert!(client.is_review_eligible(&1, &owner));
        assert!(!client.is_review_eligible(&1, &recipient));
    }
}

#[test]
fn changed_provider_traveller_and_unauthorized_retry_cannot_replace_owner() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let mut record = eligible(&env, 1);
    let owner = record.traveller.clone();
    let replacement = Address::generate(&env);
    let (client, provider, _, _) = setup(&env, record.clone());
    let id = client.mock_all_auths().mint_for_booking(&1);
    let original = client.get_credential(&id);
    record.traveller = replacement.clone();
    env.as_contract(&provider, || {
        env.storage().instance().set(&MockKey::Booking, &record)
    });
    // Neither the original recipient nor an unrelated wallet is the mint authority.
    for signer in [&owner, &replacement] {
        assert!(
            client
                .mock_auths(&[MockAuth {
                    address: signer,
                    invoke: &MockAuthInvoke {
                        contract: &client.address,
                        fn_name: "mint_for_booking",
                        args: (1_u64,).into_val(&env),
                        sub_invokes: &[],
                    },
                }])
                .try_mint_for_booking(&1)
                .is_err()
        );
        assert!(env.events().all().events().is_empty());
        assert_eq!(client.get_credential(&id), original);
    }
    assert_eq!(client.mock_all_auths().mint_for_booking(&1), id);
    assert!(env.events().all().events().is_empty());
    assert_eq!(client.get_credential(&id), original);
    assert_eq!(client.get_credential_by_booking(&1), Some(original));
    assert!(client.is_review_eligible(&1, &owner));
    assert!(!client.is_review_eligible(&1, &replacement));
    env.as_contract(&client.address, || assert_eq!(load_next_id(&env), Ok(2)));
}

#[contract]
struct StorageAttacker;

#[contractimpl]
impl StorageAttacker {
    pub fn forge(env: Env, credential: Credential) {
        env.storage()
            .persistent()
            .set(&DataKey::Credential(credential.credential_id), &credential);
        env.storage().persistent().set(
            &DataKey::Issuance(credential.booking),
            &credential.credential_id,
        );
    }
}

#[test]
fn another_contract_cannot_overwrite_sbt_storage_with_identical_keys() {
    let env = test_env();
    let (client, _, _, _) = setup(&env, eligible(&env, 1));
    let id = client.mock_all_auths().mint_for_booking(&1);
    let original = client.get_credential(&id);
    let mut forged = original.clone();
    forged.owner = Address::generate(&env);
    let attacker = env.register(StorageAttacker, ());
    StorageAttackerClient::new(&env, &attacker).forge(&forged);
    env.as_contract(&attacker, || {
        assert_eq!(
            env.storage()
                .persistent()
                .get::<_, Credential>(&DataKey::Credential(id)),
            Some(forged.clone())
        );
    });
    assert_eq!(client.get_credential(&id), original);
    assert_eq!(client.get_credential_by_booking(&1), Some(original.clone()));
    assert!(client.is_review_eligible(&1, &original.owner));
    assert!(!client.is_review_eligible(&1, &forged.owner));
}

fn dummy_wasm_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xABu8; 32])
}

#[test]
fn contract_version_is_read_only_package_version() {
    let env = test_env();
    let (client, _, _, _) = setup(&env, eligible(&env, 1));
    env.set_auths(&[]);
    let v = client.contract_version();
    assert_eq!(v, String::from_str(&env, env!("CARGO_PKG_VERSION")));
    assert_eq!(v, String::from_str(&env, "0.3.0"));
}

#[test]
fn upgrade_rejects_without_any_auth() {
    let env = test_env();
    let (client, _, _, _) = setup(&env, eligible(&env, 1));
    let hash = dummy_wasm_hash(&env);
    env.set_auths(&[]);
    assert!(
        client.try_upgrade(&hash).is_err(),
        "upgrade must fail without upgrade_authority authorization"
    );
}

#[test]
fn upgrade_rejects_mint_authority_auth_only() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let (client, _, mint_authority, _) = setup(&env, eligible(&env, 1));
    let hash = dummy_wasm_hash(&env);
    env.set_auths(&[]);
    env.mock_auths(&[MockAuth {
        address: &mint_authority,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "upgrade",
            args: (hash.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_upgrade(&hash).is_err());
}

#[test]
fn upgrade_rejects_traveller_and_stranger_auth_only() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let record = eligible(&env, 1);
    let traveller = record.traveller.clone();
    let (client, _, _, _) = setup(&env, record);
    let hash = dummy_wasm_hash(&env);
    let stranger = Address::generate(&env);
    for signer in [&traveller, &stranger] {
        env.set_auths(&[]);
        env.mock_auths(&[MockAuth {
            address: signer,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "upgrade",
                args: (hash.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }]);
        assert!(client.try_upgrade(&hash).is_err());
    }
}

#[test]
fn upgrade_rejects_all_zero_wasm_hash() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let (client, _, _, upgrade_authority) = setup(&env, eligible(&env, 1));
    let zero = BytesN::from_array(&env, &[0u8; 32]);
    env.set_auths(&[]);
    env.mock_auths(&[MockAuth {
        address: &upgrade_authority,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "upgrade",
            args: (zero.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert_eq!(
        client.try_upgrade(&zero).unwrap_err(),
        Ok(Error::InvalidWasmHash)
    );
}

#[test]
fn upgrade_with_upgrade_authority_reaches_wasm_update() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    // Native unit tests do not ship a replacement Wasm blob. With only
    // upgrade_authority mocked, the call must pass require_auth and reach
    // update_current_contract_wasm (which then fails because the hash is
    // not uploaded). Credential state must remain intact.
    let env = test_env();
    let (client, _, _, upgrade_authority) = setup(&env, eligible(&env, 1));
    let id = client.mock_all_auths().mint_for_booking(&1);
    let before = client.get_credential(&id);
    let config_before = client.get_config();
    let hash = dummy_wasm_hash(&env);

    env.set_auths(&[]);
    env.mock_auths(&[MockAuth {
        address: &upgrade_authority,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "upgrade",
            args: (hash.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let err = client.try_upgrade(&hash);
    assert!(
        err.is_err(),
        "missing uploaded Wasm should not silently succeed"
    );
    assert_eq!(client.get_credential(&id), before);
    assert_eq!(client.get_config(), config_before);
}

#[test]
fn set_upgrade_authority_rejects_unauthorized_and_mint_authority() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let (client, _, mint_authority, upgrade_authority) = setup(&env, eligible(&env, 1));
    let next = Address::generate(&env);

    env.set_auths(&[]);
    assert!(client.try_set_upgrade_authority(&next).is_err());

    env.set_auths(&[]);
    env.mock_auths(&[MockAuth {
        address: &mint_authority,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_upgrade_authority",
            args: (next.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_set_upgrade_authority(&next).is_err());

    env.set_auths(&[]);
    env.mock_auths(&[MockAuth {
        address: &upgrade_authority,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_upgrade_authority",
            args: (mint_authority.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert_eq!(
        client
            .try_set_upgrade_authority(&mint_authority)
            .unwrap_err(),
        Ok(Error::InvalidConfiguration)
    );
    assert_eq!(client.get_config().upgrade_authority, upgrade_authority);
}

#[test]
fn set_upgrade_authority_idempotent_same_authority_emits_no_event() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let (client, _, _, upgrade_authority) = setup(&env, eligible(&env, 1));
    let _ = env.events().all();
    env.set_auths(&[]);
    env.mock_auths(&[MockAuth {
        address: &upgrade_authority,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_upgrade_authority",
            args: (upgrade_authority.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_upgrade_authority(&upgrade_authority);
    assert!(env.events().all().events().is_empty());
    assert_eq!(client.get_config().upgrade_authority, upgrade_authority);
}

#[test]
fn set_upgrade_authority_rotates_and_emits_event() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let (client, provider, mint_authority, upgrade_authority) = setup(&env, eligible(&env, 1));
    let _ = env.events().all();
    let next = Address::generate(&env);
    env.set_auths(&[]);
    env.mock_auths(&[MockAuth {
        address: &upgrade_authority,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_upgrade_authority",
            args: (next.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_upgrade_authority(&next);
    let changed_at = env.ledger().timestamp();
    let all_events = env.events().all();
    let events = all_events.events();
    assert_eq!(events.len(), 1);
    let soroban_sdk::xdr::ContractEventBody::V0(body) = &events[0].body;
    let topics: soroban_sdk::Vec<soroban_sdk::Val> = vec![
        &env,
        Symbol::new(&env, "authority").into_val(&env),
        Symbol::new(&env, "upgrade_changed").into_val(&env),
    ];
    assert_eq!(body.topics, topics.into());
    let data: soroban_sdk::Map<Symbol, soroban_sdk::Val> = soroban_sdk::map![
        &env,
        (
            Symbol::new(&env, "previous_authority"),
            upgrade_authority.into_val(&env)
        ),
        (
            Symbol::new(&env, "new_authority"),
            next.clone().into_val(&env)
        ),
        (Symbol::new(&env, "changed_at"), changed_at.into_val(&env))
    ];
    assert_eq!(body.data, data.into());
    assert_eq!(
        client.get_config(),
        Config {
            booking_contract: provider,
            mint_authority,
            upgrade_authority: next,
        }
    );
}

fn review_hash(env: &Env, fill: u8) -> BytesN<32> {
    BytesN::from_array(env, &[fill; 32])
}

#[test]
fn storage_compat_config_credential_and_upgrade_authority_unchanged_with_review_api() {
    let env = test_env();
    let (client, provider, mint_authority, upgrade_authority) = setup(&env, eligible(&env, 1));
    let id = client.mock_all_auths().mint_for_booking(&1);
    let credential = client.get_credential(&id);
    let config = client.get_config();
    assert_eq!(
        config,
        Config {
            booking_contract: provider.clone(),
            mint_authority,
            upgrade_authority,
        }
    );
    assert_eq!(credential.booking.booking_contract, provider);
    assert_eq!(
        client.get_credential_by_booking(&1),
        Some(credential.clone())
    );
    // New review keys start empty; existing shapes remain readable.
    assert_eq!(client.get_review_status(&1), None);
    assert!(client.is_review_eligible(&1, &credential.owner));
    assert_eq!(client.mock_all_auths().mint_for_booking(&1), id);
    assert_eq!(client.get_credential(&id), credential);
}

#[test]
fn mark_reviewed_happy_path_emits_once_and_gates_eligibility() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let record = eligible(&env, 4);
    let owner = record.traveller.clone();
    let (client, provider, _, _) = setup(&env, record);
    let id = client.mock_all_auths().mint_for_booking(&4);
    assert_eq!(client.get_review_status(&4), None);
    assert!(client.is_review_eligible(&4, &owner));
    assert!(!client.is_review_eligible(&4, &Address::generate(&env)));

    let hash = review_hash(&env, 0x11);
    let _ = env.events().all();
    env.set_auths(&[]);
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (4_u64, hash.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let status = client.mark_reviewed(&4, &hash);
    let reviewed_at = env.ledger().timestamp();
    assert_eq!(
        status,
        ReviewStatus {
            booking: BookingKey {
                booking_contract: provider.clone(),
                booking_id: 4,
            },
            credential_id: id,
            reviewer: owner.clone(),
            review_hash: hash.clone(),
            reviewed_at,
        }
    );
    let all_events = env.events().all();
    let events = all_events.events();
    assert_eq!(events.len(), 1);
    let soroban_sdk::xdr::ContractEventBody::V0(body) = &events[0].body;
    let topics: soroban_sdk::Vec<soroban_sdk::Val> = vec![
        &env,
        Symbol::new(&env, "review").into_val(&env),
        Symbol::new(&env, "marked").into_val(&env),
        id.into_val(&env),
    ];
    assert_eq!(body.topics, topics.into());
    assert_eq!(client.get_review_status(&4), Some(status));
    assert!(!client.is_review_eligible(&4, &owner));
    assert_eq!(client.get_credential(&id).owner, owner);
    assert_eq!(
        client.get_credential_by_booking(&4).unwrap().booking,
        BookingKey {
            booking_contract: provider,
            booking_id: 4
        }
    );
}

#[test]
fn mark_reviewed_rejects_non_owner_mint_and_upgrade_authority() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let record = eligible(&env, 1);
    let owner = record.traveller.clone();
    let (client, _, mint_authority, upgrade_authority) = setup(&env, record);
    client.mock_all_auths().mint_for_booking(&1);
    let hash = review_hash(&env, 0x22);
    let stranger = Address::generate(&env);
    for signer in [&stranger, &mint_authority, &upgrade_authority] {
        env.set_auths(&[]);
        env.mock_auths(&[MockAuth {
            address: signer,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "mark_reviewed",
                args: (1_u64, hash.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }]);
        assert!(client.try_mark_reviewed(&1, &hash).is_err());
    }
    assert_eq!(client.get_review_status(&1), None);
    assert!(client.is_review_eligible(&1, &owner));
}

#[test]
fn mark_reviewed_before_credential_and_zero_hash_fail() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let record = eligible(&env, 1);
    let owner = record.traveller.clone();
    let (client, _, _, _) = setup(&env, record);
    let hash = review_hash(&env, 0x33);
    env.set_auths(&[]);
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (1_u64, hash.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert_eq!(
        client.try_mark_reviewed(&1, &hash).unwrap_err(),
        Ok(Error::CredentialNotFound)
    );

    let id = client.mock_all_auths().mint_for_booking(&1);
    let zero = BytesN::from_array(&env, &[0u8; 32]);
    env.set_auths(&[]);
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (1_u64, zero.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert_eq!(
        client.try_mark_reviewed(&1, &zero).unwrap_err(),
        Ok(Error::InvalidReviewHash)
    );
    assert_eq!(client.get_review_status(&1), None);
    assert!(client.is_review_eligible(&1, &owner));
    assert_eq!(client.get_credential(&id).owner, owner);
}

#[test]
fn mark_reviewed_same_hash_idempotent_different_hash_rejected() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let record = eligible(&env, 1);
    let owner = record.traveller.clone();
    let (client, _, _, _) = setup(&env, record);
    let id = client.mock_all_auths().mint_for_booking(&1);
    let hash_a = review_hash(&env, 0x44);
    let hash_b = review_hash(&env, 0x55);

    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (1_u64, hash_a.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let first = client.mark_reviewed(&1, &hash_a);
    let _ = env.events().all();

    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (1_u64, hash_a.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let retry = client.mark_reviewed(&1, &hash_a);
    assert_eq!(retry, first);
    assert_eq!(retry.reviewed_at, first.reviewed_at);
    assert!(env.events().all().events().is_empty());
    assert_eq!(client.get_review_status(&1), Some(first.clone()));

    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (1_u64, hash_b.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert_eq!(
        client.try_mark_reviewed(&1, &hash_b).unwrap_err(),
        Ok(Error::ReviewAlreadySubmitted)
    );
    assert_eq!(client.get_review_status(&1), Some(first));
    assert!(!client.is_review_eligible(&1, &owner));
    assert_eq!(client.get_credential(&id).credential_id, id);
}

#[test]
fn mark_reviewed_extends_review_status_ttl() {
    use soroban_sdk::testutils::{Ledger, MockAuth, MockAuthInvoke};
    let env = test_env();
    let record = eligible(&env, 1);
    let owner = record.traveller.clone();
    let (client, provider, _, _) = setup(&env, record);
    let id = client.mock_all_auths().mint_for_booking(&1);
    let hash = review_hash(&env, 0x66);
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (1_u64, hash.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.mark_reviewed(&1, &hash);
    let booking_key = BookingKey {
        booking_contract: provider,
        booking_id: 1,
    };
    let keys = [
        DataKey::Credential(id),
        DataKey::Issuance(booking_key.clone()),
        DataKey::ReviewStatus(booking_key),
    ];
    env.as_contract(&client.address, || {
        for key in &keys {
            assert_eq!(
                env.storage().persistent().get_ttl(key),
                PERSISTENT_TTL_EXTEND_TO
            );
        }
    });

    env.ledger()
        .with_mut(|info| info.sequence_number += 450_001);
    let before = env.as_contract(&client.address, || {
        keys.clone()
            .map(|key| env.storage().persistent().get_ttl(&key))
    });
    assert!(before.iter().all(|ttl| *ttl < PERSISTENT_TTL_THRESHOLD));
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (1_u64, hash.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.mark_reviewed(&1, &hash);
    env.as_contract(&client.address, || {
        for key in &keys {
            assert_eq!(
                env.storage().persistent().get_ttl(key),
                PERSISTENT_TTL_EXTEND_TO
            );
        }
    });
    // Reads still do not bump TTL.
    env.ledger()
        .with_mut(|info| info.sequence_number += 450_001);
    let mid = env.as_contract(&client.address, || {
        keys.clone()
            .map(|key| env.storage().persistent().get_ttl(&key))
    });
    let _ = client.get_review_status(&1);
    let _ = client.is_review_eligible(&1, &owner);
    let _ = client.get_credential(&id);
    let _ = client.get_credential_by_booking(&1);
    let _ = client.get_config();
    let _ = client.contract_version();
    env.as_contract(&client.address, || {
        assert_eq!(
            keys.clone()
                .map(|key| env.storage().persistent().get_ttl(&key)),
            mid
        );
    });
}

#[test]
fn review_does_not_enable_transfer_burn_or_credential_mutation() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let record = eligible(&env, 1);
    let owner = record.traveller.clone();
    let (client, _, _, _) = setup(&env, record);
    let id = client.mock_all_auths().mint_for_booking(&1);
    let hash = review_hash(&env, 0x77);
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (1_u64, hash.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.mark_reviewed(&1, &hash);
    let original = client.get_credential(&id);
    env.mock_all_auths();
    for name in ["transfer", "burn", "set_owner", "revoke"] {
        let result = env.try_invoke_contract::<soroban_sdk::Val, Error>(
            &client.address,
            &Symbol::new(&env, name),
            vec![&env, owner.clone().into_val(&env), id.into_val(&env)],
        );
        assert!(result.is_err(), "unexpected callable: {name}");
    }
    assert_eq!(client.get_credential(&id), original);
    assert_eq!(client.get_review_status(&1).unwrap().review_hash, hash);
}

#[test]
fn extend_instance_ttl_renews_instance_and_code_without_mutating_config() {
    use soroban_sdk::testutils::{Deployer as _, Ledger, storage::Instance as _};
    let env = test_env();
    let (client, _, _, _) = setup(&env, eligible(&env, 1));
    let config_before = client.get_config();
    let next_before = env.as_contract(&client.address, || load_next_id(&env).unwrap());

    env.ledger()
        .with_mut(|info| info.sequence_number += 450_001);
    let (instance_before, code_before) = (
        env.as_contract(&client.address, || env.storage().instance().get_ttl()),
        env.deployer().get_contract_code_ttl(&client.address),
    );
    assert!(instance_before < INSTANCE_TTL_THRESHOLD);
    assert!(code_before < INSTANCE_TTL_THRESHOLD);

    assert_eq!(client.extend_instance_ttl(), ());
    assert_eq!(client.get_config(), config_before);
    env.as_contract(&client.address, || {
        assert_eq!(load_next_id(&env).unwrap(), next_before);
        assert_eq!(env.storage().instance().get_ttl(), INSTANCE_TTL_EXTEND_TO);
    });
    // SDK 27 Instance::extend_ttl also renews WASM/code when below threshold.
    assert_eq!(
        env.deployer().get_contract_code_ttl(&client.address),
        INSTANCE_TTL_EXTEND_TO
    );
    assert!(env.events().all().events().is_empty());

    // Above threshold: no unnecessary extension (TTL continues to decay).
    env.ledger().with_mut(|info| info.sequence_number += 1);
    let mid_instance = env.as_contract(&client.address, || env.storage().instance().get_ttl());
    let mid_code = env.deployer().get_contract_code_ttl(&client.address);
    assert!(mid_instance > INSTANCE_TTL_THRESHOLD);
    assert_eq!(client.extend_instance_ttl(), ());
    env.as_contract(&client.address, || {
        assert_eq!(env.storage().instance().get_ttl(), mid_instance);
    });
    assert_eq!(
        env.deployer().get_contract_code_ttl(&client.address),
        mid_code
    );
    assert_eq!(client.get_config(), config_before);
}

#[test]
fn extend_credential_ttl_renews_related_entries_and_is_idempotent() {
    use soroban_sdk::testutils::{
        Ledger, MockAuth, MockAuthInvoke,
        storage::{Instance as _, Persistent as _},
    };
    let env = test_env();
    let record = eligible(&env, 9);
    let owner = record.traveller.clone();
    let (client, provider, _, _) = setup(&env, record);
    let id = client.mock_all_auths().mint_for_booking(&9);
    let credential_before = client.get_credential(&id);
    let hash = review_hash(&env, 0x11);
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (9_u64, hash.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let review_before = client.mark_reviewed(&9, &hash);

    let booking_key = BookingKey {
        booking_contract: provider.clone(),
        booking_id: 9,
    };
    let keys = [
        DataKey::Credential(id),
        DataKey::Issuance(booking_key.clone()),
        DataKey::ReviewStatus(booking_key),
    ];

    env.ledger()
        .with_mut(|info| info.sequence_number += 450_001);
    let before = env.as_contract(&client.address, || {
        keys.clone()
            .map(|key| env.storage().persistent().get_ttl(&key))
    });
    assert!(before.iter().all(|ttl| *ttl < PERSISTENT_TTL_THRESHOLD));

    assert_eq!(client.extend_credential_ttl(&9), ());
    env.as_contract(&client.address, || {
        for key in &keys {
            assert_eq!(
                env.storage().persistent().get_ttl(key),
                PERSISTENT_TTL_EXTEND_TO
            );
        }
        assert_eq!(env.storage().instance().get_ttl(), INSTANCE_TTL_EXTEND_TO);
    });
    assert_eq!(client.get_credential(&id), credential_before);
    assert_eq!(client.get_review_status(&9), Some(review_before.clone()));
    assert!(env.events().all().events().is_empty());

    // Repeated call is idempotent and does not mutate values.
    env.ledger()
        .with_mut(|info| info.sequence_number += 450_001);
    assert_eq!(client.extend_credential_ttl(&9), ());
    assert_eq!(client.extend_credential_ttl(&9), ());
    assert_eq!(client.get_credential(&id), credential_before);
    assert_eq!(client.get_review_status(&9), Some(review_before.clone()));
    assert_eq!(client.get_credential(&id).owner, credential_before.owner);
    assert_eq!(client.get_credential(&id).booking.booking_id, 9);
    assert_eq!(
        client.get_credential(&id).issued_at,
        credential_before.issued_at
    );
    assert_eq!(client.get_credential(&id).schema_version, 1);
    assert_eq!(client.get_review_status(&9).unwrap().review_hash, hash);
    assert_eq!(
        client.get_review_status(&9).unwrap().reviewed_at,
        review_before.reviewed_at
    );

    assert_eq!(
        client.try_extend_credential_ttl(&99).unwrap_err(),
        Ok(Error::CredentialNotFound)
    );
}

#[test]
fn extend_credential_ttl_without_review_extends_issuance_and_credential_only() {
    use soroban_sdk::testutils::{Ledger, storage::Persistent as _};
    let env = test_env();
    let (client, provider, _, _) = setup(&env, eligible(&env, 3));
    let id = client.mock_all_auths().mint_for_booking(&3);
    let booking_key = BookingKey {
        booking_contract: provider,
        booking_id: 3,
    };
    env.ledger()
        .with_mut(|info| info.sequence_number += 450_001);
    assert_eq!(client.extend_credential_ttl(&3), ());
    env.as_contract(&client.address, || {
        assert_eq!(
            env.storage().persistent().get_ttl(&DataKey::Credential(id)),
            PERSISTENT_TTL_EXTEND_TO
        );
        assert_eq!(
            env.storage()
                .persistent()
                .get_ttl(&DataKey::Issuance(booking_key.clone())),
            PERSISTENT_TTL_EXTEND_TO
        );
        assert!(
            !env.storage()
                .persistent()
                .has(&DataKey::ReviewStatus(booking_key))
        );
    });
}

#[test]
fn mint_retry_renews_review_status_when_present_without_duplicate_event() {
    use soroban_sdk::testutils::{Ledger, MockAuth, MockAuthInvoke, storage::Persistent as _};
    let env = test_env();
    let record = eligible(&env, 1);
    let owner = record.traveller.clone();
    let (client, provider, _, _) = setup(&env, record);
    let id = client.mock_all_auths().mint_for_booking(&1);
    let hash = review_hash(&env, 0x22);
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (1_u64, hash.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let review = client.mark_reviewed(&1, &hash);
    let booking_key = BookingKey {
        booking_contract: provider,
        booking_id: 1,
    };
    let keys = [
        DataKey::Credential(id),
        DataKey::Issuance(booking_key.clone()),
        DataKey::ReviewStatus(booking_key),
    ];
    env.ledger()
        .with_mut(|info| info.sequence_number += 450_001);
    assert_eq!(client.mock_all_auths().mint_for_booking(&1), id);
    assert!(env.events().all().events().is_empty());
    assert_eq!(client.get_review_status(&1), Some(review));
    env.as_contract(&client.address, || {
        for key in keys {
            assert_eq!(
                env.storage().persistent().get_ttl(&key),
                PERSISTENT_TTL_EXTEND_TO
            );
        }
    });
}

#[test]
fn mark_reviewed_new_path_renews_all_three_persistent_entries() {
    use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
    let env = test_env();
    let record = eligible(&env, 2);
    let owner = record.traveller.clone();
    let (client, provider, _, _) = setup(&env, record);
    let id = client.mock_all_auths().mint_for_booking(&2);
    let hash = review_hash(&env, 0x33);
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (2_u64, hash.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let status = client.mark_reviewed(&2, &hash);
    let booking_key = BookingKey {
        booking_contract: provider,
        booking_id: 2,
    };
    env.as_contract(&client.address, || {
        for key in [
            DataKey::Credential(id),
            DataKey::Issuance(booking_key.clone()),
            DataKey::ReviewStatus(booking_key),
        ] {
            assert_eq!(
                env.storage().persistent().get_ttl(&key),
                PERSISTENT_TTL_EXTEND_TO
            );
        }
    });
    assert_eq!(status.review_hash, hash);
    assert_eq!(client.get_review_status(&2), Some(status));
}

#[test]
fn mark_reviewed_different_hash_rejects_without_ttl_or_value_mutation() {
    use soroban_sdk::testutils::{Ledger, MockAuth, MockAuthInvoke, storage::Persistent as _};
    let env = test_env();
    let record = eligible(&env, 1);
    let owner = record.traveller.clone();
    let (client, provider, _, _) = setup(&env, record);
    let id = client.mock_all_auths().mint_for_booking(&1);
    let hash_a = review_hash(&env, 0x44);
    let hash_b = review_hash(&env, 0x55);
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (1_u64, hash_a.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let first = client.mark_reviewed(&1, &hash_a);
    let booking_key = BookingKey {
        booking_contract: provider,
        booking_id: 1,
    };
    let keys = [
        DataKey::Credential(id),
        DataKey::Issuance(booking_key.clone()),
        DataKey::ReviewStatus(booking_key),
    ];
    env.ledger()
        .with_mut(|info| info.sequence_number += 450_001);
    let before = env.as_contract(&client.address, || {
        keys.clone()
            .map(|key| env.storage().persistent().get_ttl(&key))
    });
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "mark_reviewed",
            args: (1_u64, hash_b.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert_eq!(
        client.try_mark_reviewed(&1, &hash_b).unwrap_err(),
        Ok(Error::ReviewAlreadySubmitted)
    );
    assert_eq!(client.get_review_status(&1), Some(first));
    env.as_contract(&client.address, || {
        assert_eq!(
            keys.clone()
                .map(|key| env.storage().persistent().get_ttl(&key)),
            before
        );
    });
}

#[test]
fn getters_remain_read_only_and_do_not_bump_ttl() {
    use soroban_sdk::testutils::{
        Ledger,
        storage::{Instance as _, Persistent as _},
    };
    let env = test_env();
    let record = eligible(&env, 5);
    let owner = record.traveller.clone();
    let (client, provider, _, _) = setup(&env, record);
    let id = client.mock_all_auths().mint_for_booking(&5);
    let booking_key = BookingKey {
        booking_contract: provider,
        booking_id: 5,
    };
    let keys = [DataKey::Credential(id), DataKey::Issuance(booking_key)];
    env.ledger()
        .with_mut(|info| info.sequence_number += 450_001);
    let persistent_before = env.as_contract(&client.address, || {
        keys.clone()
            .map(|key| env.storage().persistent().get_ttl(&key))
    });
    let instance_before = env.as_contract(&client.address, || env.storage().instance().get_ttl());

    let _ = client.get_config();
    let _ = client.contract_version();
    let _ = client.get_credential(&id);
    let _ = client.get_credential_by_booking(&5);
    let _ = client.get_review_status(&5);
    let _ = client.is_review_eligible(&5, &owner);

    env.as_contract(&client.address, || {
        assert_eq!(
            keys.clone()
                .map(|key| env.storage().persistent().get_ttl(&key)),
            persistent_before
        );
        assert_eq!(env.storage().instance().get_ttl(), instance_before);
    });
}

#[test]
fn maintenance_after_near_expiry_extends_live_entries_with_auto_restore_emulation() {
    use soroban_sdk::testutils::{Ledger, storage::Persistent as _};
    // SDK 27 / protocol 23 unit tests emulate automatic restoration when an
    // archived Persistent entry is accessed. Full RPC restoration-footprint
    // behavior is outside this unit-test environment; this is the closest
    // deterministic coverage: advance past expiry, then maintain.
    let env = test_env();
    let (client, provider, _, _) = setup(&env, eligible(&env, 8));
    let id = client.mock_all_auths().mint_for_booking(&8);
    let booking_key = BookingKey {
        booking_contract: provider,
        booking_id: 8,
    };
    let current = env.ledger().sequence();
    // Past persistent extend_to so entries would be archived on-network.
    env.ledger()
        .set_sequence_number(current + PERSISTENT_TTL_EXTEND_TO + 1);

    // Access + maintenance still succeeds under auto-restore emulation and
    // renews entries — never treats archived state as CredentialNotFound.
    assert_eq!(client.extend_credential_ttl(&8), ());
    assert_eq!(client.get_credential(&id).credential_id, id);
    env.as_contract(&client.address, || {
        assert_eq!(
            env.storage().persistent().get_ttl(&DataKey::Credential(id)),
            PERSISTENT_TTL_EXTEND_TO
        );
        assert_eq!(
            env.storage()
                .persistent()
                .get_ttl(&DataKey::Issuance(booking_key)),
            PERSISTENT_TTL_EXTEND_TO
        );
    });
}
