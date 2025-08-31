use alloy::primitives::{Address, U256};
use alloy::sol_types::{SolCall, SolInterface};
use alloy_evm::EvmEnv;
use eyre::Result;
use kabu_defi_abi::uniswap2::IUniswapV2Pair;
use kabu_evm_db::KabuDBError;
use kabu_evm_utils::evm_call;
use kabu_types_market::PoolError;
use revm::DatabaseRef;
use tracing::error;

pub struct UniswapV2EVMStateReader {}

impl UniswapV2EVMStateReader {
    pub fn factory<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, evm_env: &EvmEnv, pool: Address) -> Result<Address, PoolError> {
        let input = IUniswapV2Pair::IUniswapV2PairCalls::factory(IUniswapV2Pair::factoryCall {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, evm_env.clone(), pool, input)?;

        let call_return = IUniswapV2Pair::factoryCall::abi_decode_returns(&call_data_result)
            .map_err(|e| PoolError::AbiDecodingError { method: "factory", source: e })?;
        Ok(call_return)
    }

    pub fn token0<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, evm_env: &EvmEnv, pool: Address) -> Result<Address, PoolError> {
        let input = IUniswapV2Pair::IUniswapV2PairCalls::token0(IUniswapV2Pair::token0Call {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, evm_env.clone(), pool, input)?;

        let call_return = IUniswapV2Pair::token0Call::abi_decode_returns(&call_data_result)
            .map_err(|e| PoolError::AbiDecodingError { method: "token0", source: e })?;
        Ok(call_return)
    }

    pub fn token1<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, evm_env: &EvmEnv, pool: Address) -> Result<Address, PoolError> {
        let input = IUniswapV2Pair::IUniswapV2PairCalls::token1(IUniswapV2Pair::token1Call {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, evm_env.clone(), pool, input)?;

        let call_return = IUniswapV2Pair::token1Call::abi_decode_returns(&call_data_result)
            .map_err(|e| PoolError::AbiDecodingError { method: "token1", source: e })?;
        Ok(call_return)
    }

    pub fn get_reserves<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(
        db: &DB,
        evm_env: EvmEnv,
        pool: Address,
    ) -> Result<(U256, U256), PoolError> {
        let input = IUniswapV2Pair::IUniswapV2PairCalls::getReserves(IUniswapV2Pair::getReservesCall {}).abi_encode();
        let call_data_result = match evm_call(db, evm_env, pool, input) {
            Ok((call_data, _, _)) => call_data,
            Err(error) => {
                error!(%error,"get_reserves");
                return Err(error.into());
            }
        };

        let call_return = IUniswapV2Pair::getReservesCall::abi_decode_returns(&call_data_result)
            .map_err(|e| PoolError::AbiDecodingError { method: "getReserves", source: e })?;
        Ok((U256::from(call_return.reserve0), U256::from(call_return.reserve1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UniswapV2Pool;
    use alloy::primitives::address;
    use alloy_evm::EvmEnv;
    use kabu_evm_db::{DatabaseHelpers, KabuDBType};
    use kabu_node_debug_provider::AnvilDebugProviderFactory;
    use kabu_types_market::Pool;
    use kabu_types_market::RequiredStateReader;
    use std::env;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_uniswap_v2_state_reader_get_reserves() -> Result<()> {
        dotenvy::from_filename(".env.test").ok();
        let node_url = env::var("MAINNET_WS")?;

        let client = AnvilDebugProviderFactory::from_node_on_block(node_url, 22952705).await?;

        let pool_address = address!("10ef7f8833ea986109abc6ef2ac379bf4baf2801");
        let pool = UniswapV2Pool::fetch_pool_data(client.clone(), pool_address).await?;
        let state_required = pool.get_state_required()?;
        let state_update = RequiredStateReader::fetch_calls_and_slots(client.clone(), state_required, None).await?;

        let mut state_db = KabuDBType::default();
        //state_db.apply_geth_update(state_update);
        DatabaseHelpers::apply_geth_state_update(&mut state_db, state_update);

        let evm_env = EvmEnv::default();
        let (reserve0, reserve1) = UniswapV2EVMStateReader::get_reserves(&state_db, evm_env, pool_address)?;

        assert_eq!(reserve0, U256::from(356862508671847911238069u128));
        assert_eq!(reserve1, U256::from(11007058744278921557u128));

        Ok(())
    }
}
