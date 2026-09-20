//! Contract events for off-chain indexers.
use soroban_sdk::{Address, BytesN, contractevent};

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
    pub upgrade_authority: Address,
}

#[contractevent(topics = ["contract", "upgraded"])]
pub struct ContractUpgraded {
    pub new_wasm_hash: BytesN<32>,
    pub upgraded_at: u64,
}

#[contractevent(topics = ["authority", "upgrade_changed"])]
pub struct UpgradeAuthorityChanged {
    pub previous_authority: Address,
    pub new_authority: Address,
    pub changed_at: u64,
}

#[contractevent(topics = ["review", "marked"])]
pub struct ReviewMarked {
    #[topic]
    pub credential_id: u64,
    pub booking_contract: Address,
    pub booking_id: u64,
    pub traveller: Address,
    pub review_hash: BytesN<32>,
    pub reviewed_at: u64,
}
