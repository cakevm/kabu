#[cfg(feature = "broadcast")]
pub mod broadcast {
    #[cfg(feature = "broadcast-accounts")]
    pub use kabu_broadcast_accounts as accounts;
    #[cfg(feature = "broadcast-broadcaster")]
    pub use kabu_broadcast_broadcaster as broadcaster;
    #[cfg(feature = "broadcast-flashbots")]
    pub use kabu_broadcast_flashbots as flashbots;
}

#[cfg(feature = "core")]
pub mod core {
    #[cfg(feature = "core-actors")]
    pub use kabu_core_actors as actors;
    #[cfg(feature = "core-actors-macros")]
    pub use kabu_core_actors_macros as macros;
    #[cfg(feature = "core-block-history")]
    pub use kabu_core_block_history as block_history;
    #[cfg(feature = "core-blockchain")]
    pub use kabu_core_blockchain as blockchain;
    #[cfg(feature = "core-blockchain-actors")]
    pub use kabu_core_blockchain_actors as blockchain_actors;
    #[cfg(feature = "core-mempool")]
    pub use kabu_core_mempool as mempool;
    #[cfg(feature = "core-router")]
    pub use kabu_core_router as router;
    #[cfg(feature = "core-topology")]
    pub use kabu_core_topology as topology;
}

#[cfg(feature = "defi")]
pub mod defi {
    #[cfg(feature = "defi-abi")]
    pub use kabu_defi_abi as abi;
    #[cfg(feature = "defi-address-book")]
    pub use kabu_defi_address_book as address_book;
    #[cfg(feature = "defi-health-monitor")]
    pub use kabu_defi_health_monitor as health_monitor;
    #[cfg(feature = "defi-market")]
    pub use kabu_defi_market as market;
    #[cfg(feature = "defi-pools")]
    pub use kabu_defi_pools as pools;
    #[cfg(feature = "defi-preloader")]
    pub use kabu_defi_preloader as preloader;
    #[cfg(feature = "defi-price")]
    pub use kabu_defi_price as price;
    #[cfg(feature = "defi-uniswap-v3-math")]
    pub use kabu_defi_uniswap_v3_math as uniswap_v3_math;
}

#[cfg(feature = "evm")]
pub mod evm {
    #[cfg(feature = "evm-db")]
    pub use kabu_evm_db as db;
    #[cfg(feature = "evm-utils")]
    pub use kabu_evm_utils as utils;
}

#[cfg(feature = "execution")]
pub mod execution {
    #[cfg(feature = "execution-estimator")]
    pub use kabu_execution_estimator as estimator;
    #[cfg(feature = "execution-multicaller")]
    pub use kabu_execution_multicaller as multicaller;
}

#[cfg(feature = "metrics")]
pub use kabu_metrics as metrics;

#[cfg(feature = "node")]
pub mod node {
    #[cfg(feature = "node-actor-config")]
    pub use kabu_node_actor_config as actor_config;
    #[cfg(feature = "node-debug-provider")]
    pub use kabu_node_debug_provider as debug_provider;
    #[cfg(feature = "node-exex")]
    pub use kabu_node_exex as exex;
    #[cfg(feature = "node-json-rpc")]
    pub use kabu_node_json_rpc as json_rpc;
    #[cfg(feature = "node-player")]
    pub use kabu_node_player as player;
}

#[cfg(feature = "rpc")]
pub mod rpc {
    #[cfg(feature = "rpc-handler")]
    pub use kabu_rpc_handler as handler;
    #[cfg(feature = "rpc-state")]
    pub use kabu_rpc_state as state;
}

#[cfg(feature = "storage")]
pub mod storage {
    #[cfg(feature = "storage-db")]
    pub use kabu_storage_db as db;
}

#[cfg(feature = "strategy")]
pub mod strategy {
    #[cfg(feature = "strategy-backrun")]
    pub use kabu_strategy_backrun as backrun;
    #[cfg(feature = "strategy-merger")]
    pub use kabu_strategy_merger as merger;
}

#[cfg(feature = "types")]
pub mod types {
    #[cfg(feature = "types-blockchain")]
    pub use kabu_types_blockchain as blockchain;
    #[cfg(feature = "types-entities")]
    pub use kabu_types_entities as entities;
    #[cfg(feature = "types-events")]
    pub use kabu_types_events as events;
}
