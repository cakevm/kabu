use alloy::primitives::{Address, U256};
use alloy::sol_types::{SolCall, SolInterface};
use alloy_evm::EvmEnv;
use eyre::{eyre, Result};
use kabu_defi_abi::uniswap2::IUniswapV2Pair;
use kabu_evm_db::KabuDBError;
use kabu_evm_utils::evm_call;
use revm::DatabaseRef;
use tracing::error;

pub struct UniswapV2EVMStateReader {}

impl UniswapV2EVMStateReader {
    pub fn factory<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, pool: Address) -> Result<Address> {
        let input = IUniswapV2Pair::IUniswapV2PairCalls::factory(IUniswapV2Pair::factoryCall {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, EvmEnv::default(), pool, input)?;

        let call_return = IUniswapV2Pair::factoryCall::abi_decode_returns(&call_data_result)?;
        Ok(call_return)
    }

    pub fn token0<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, pool: Address) -> Result<Address> {
        let input = IUniswapV2Pair::IUniswapV2PairCalls::token0(IUniswapV2Pair::token0Call {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, EvmEnv::default(), pool, input)?;

        let call_return = IUniswapV2Pair::token0Call::abi_decode_returns(&call_data_result)?;
        Ok(call_return)
    }

    pub fn token1<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, pool: Address) -> Result<Address> {
        let input = IUniswapV2Pair::IUniswapV2PairCalls::token1(IUniswapV2Pair::token1Call {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, EvmEnv::default(), pool, input)?;

        let call_return = IUniswapV2Pair::token1Call::abi_decode_returns(&call_data_result)?;
        Ok(call_return)
    }

    pub fn get_reserves<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, pool: Address) -> Result<(U256, U256)> {
        let input = IUniswapV2Pair::IUniswapV2PairCalls::getReserves(IUniswapV2Pair::getReservesCall {}).abi_encode();
        let call_data_result = match evm_call(db, EvmEnv::default(), pool, input) {
            Ok((call_data, _, _)) => call_data,
            Err(error) => {
                error!(%error,"get_reserves");
                return Err(eyre!(error));
            }
        };

        let call_return = IUniswapV2Pair::getReservesCall::abi_decode_returns(&call_data_result)?;
        Ok((U256::from(call_return.reserve0), U256::from(call_return.reserve1)))
    }
}
