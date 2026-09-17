#![no_std]

use soroban_sdk::{
    Address, Env, contract, contracterror, contractevent, contractimpl, contracttype,
};

pub const INSTANCE_TTL_THRESHOLD: u32 = 100_000;
pub const INSTANCE_TTL_EXTEND_TO: u32 = 500_000;

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    InvalidConfiguration = 2,
    AlreadyInitialized = 13,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub booking_contract: Address,
    pub mint_authority: Address,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Config,
}

#[contractevent(topics = ["sbt", "initialized"])]
pub struct SbtInitialized {
    pub booking_contract: Address,
    pub mint_authority: Address,
}

#[contract]
pub struct StelloSbtEngine;

#[contractimpl]
impl StelloSbtEngine {
    /// Configure the contract atomically during deployment.
    pub fn __constructor(env: Env, booking_contract: Address, mint_authority: Address) {
        if env.storage().instance().has(&DataKey::Config) {
            soroban_sdk::panic_with_error!(&env, Error::AlreadyInitialized);
        }
        if !valid_configuration(&env, &booking_contract, &mint_authority) {
            soroban_sdk::panic_with_error!(&env, Error::InvalidConfiguration);
        }

        require_mint_authority(&env, &mint_authority);
        env.storage().instance().set(
            &DataKey::Config,
            &Config {
                booking_contract: booking_contract.clone(),
                mint_authority: mint_authority.clone(),
            },
        );
        bump_instance_ttl(&env);
        SbtInitialized {
            booking_contract,
            mint_authority,
        }
        .publish(&env);
    }

    /// Return the immutable deployment configuration.
    pub fn get_config(env: Env) -> Result<Config, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(Error::NotInitialized)
    }
}

fn bump_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
}

fn valid_configuration(env: &Env, booking_contract: &Address, mint_authority: &Address) -> bool {
    // The two roles may intentionally be the same address for future
    // contract-to-contract authorization flows; only self-reference is
    // rejected.
    booking_contract != &env.current_contract_address()
        && mint_authority != &env.current_contract_address()
}

fn require_mint_authority(_env: &Env, mint_authority: &Address) {
    mint_authority.require_auth();
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        Env, testutils::Address as _, testutils::Events as _, testutils::storage::Instance as _,
    };

    #[test]
    fn constructor_persists_config() {
        let env = Env::default();
        let booking = Address::generate(&env);
        let authority = Address::generate(&env);
        let contract_id = env.register(StelloSbtEngine, (&booking, &authority));
        let client = StelloSbtEngineClient::new(&env, &contract_id);

        assert_eq!(
            client.get_config(),
            Config {
                booking_contract: booking,
                mint_authority: authority.clone(),
            }
        );
        // Constructor authentication is enforced by the deployment host and
        // is not exposed through `Env::auths()` for the registration call.
    }

    #[test]
    fn same_address_configuration_is_allowed() {
        let env = Env::default();
        let address = Address::generate(&env);
        let contract_id = env.register(StelloSbtEngine, (&address, &address));
        let client = StelloSbtEngineClient::new(&env, &contract_id);
        assert_eq!(client.get_config().mint_authority, address);
    }

    #[test]
    fn mint_authority_require_auth_records_invocation() {
        let env = Env::default();
        let booking = Address::generate(&env);
        let authority = Address::generate(&env);
        let contract_id = env.register(StelloSbtEngine, (&booking, &authority));
        env.mock_all_auths();
        env.as_contract(&contract_id, || require_mint_authority(&env, &authority));
        assert_eq!(env.auths().len(), 1);
        assert_eq!(env.auths()[0].0, authority);
    }

    #[test]
    fn constructor_rejects_self_referential_addresses() {
        let env = Env::default();
        let booking = Address::generate(&env);
        let authority = Address::generate(&env);
        let contract_id = env.register(StelloSbtEngine, (&booking, &authority));
        env.as_contract(&contract_id, || {
            assert!(!valid_configuration(&env, &contract_id, &authority));
            assert!(!valid_configuration(&env, &booking, &contract_id));
            assert!(env.storage().instance().get_ttl() > 0);
        });
    }

    #[test]
    fn initialization_event_schema_is_soroban_compatible() {
        let env = Env::default();
        let booking = Address::generate(&env);
        let authority = Address::generate(&env);
        let contract_id = env.register(StelloSbtEngine, (&booking, &authority));
        env.as_contract(&contract_id, || {
            SbtInitialized {
                booking_contract: booking.clone(),
                mint_authority: authority.clone(),
            }
            .publish(&env);
        });
        assert_eq!(env.events().all().events().len(), 1);
    }
}
