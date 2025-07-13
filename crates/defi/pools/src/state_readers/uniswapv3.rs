use alloy::primitives::aliases::U24;
use alloy::primitives::Address;
use alloy::sol_types::{SolCall, SolInterface};
use alloy_evm::EvmEnv;
use kabu_defi_abi::uniswap3::IUniswapV3Pool;
use kabu_defi_abi::uniswap3::IUniswapV3Pool::slot0Return;
use kabu_evm_db::KabuDBError;
use kabu_evm_utils::evm_call;
use revm::DatabaseRef;

pub struct UniswapV3EvmStateReader {}

impl UniswapV3EvmStateReader {
    pub fn factory<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, pool: Address) -> eyre::Result<Address> {
        let input = IUniswapV3Pool::IUniswapV3PoolCalls::factory(IUniswapV3Pool::factoryCall {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, EvmEnv::default(), pool, input)?;

        let call_return = IUniswapV3Pool::factoryCall::abi_decode_returns(&call_data_result)?;
        Ok(call_return)
    }

    pub fn token0<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, pool: Address) -> eyre::Result<Address> {
        let input = IUniswapV3Pool::IUniswapV3PoolCalls::token0(IUniswapV3Pool::token0Call {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, EvmEnv::default(), pool, input)?;

        let call_return = IUniswapV3Pool::token0Call::abi_decode_returns(&call_data_result)?;
        Ok(call_return)
    }

    pub fn token1<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, pool: Address) -> eyre::Result<Address> {
        let input = IUniswapV3Pool::IUniswapV3PoolCalls::token1(IUniswapV3Pool::token1Call {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, EvmEnv::default(), pool, input)?;

        let call_return = IUniswapV3Pool::token1Call::abi_decode_returns(&call_data_result)?;
        Ok(call_return)
    }

    pub fn fee<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, pool: Address) -> eyre::Result<U24> {
        let input = IUniswapV3Pool::IUniswapV3PoolCalls::fee(IUniswapV3Pool::feeCall {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, EvmEnv::default(), pool, input)?;

        let call_return = IUniswapV3Pool::feeCall::abi_decode_returns(&call_data_result)?;
        Ok(call_return)
    }

    pub fn tick_spacing<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, pool: Address) -> eyre::Result<u32> {
        let input = IUniswapV3Pool::IUniswapV3PoolCalls::tickSpacing(IUniswapV3Pool::tickSpacingCall {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, EvmEnv::default(), pool, input)?;

        let call_return = IUniswapV3Pool::tickSpacingCall::abi_decode_returns(&call_data_result)?;
        Ok(call_return.try_into()?)
    }

    pub fn slot0<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, pool: Address) -> eyre::Result<slot0Return> {
        let input = IUniswapV3Pool::IUniswapV3PoolCalls::slot0(IUniswapV3Pool::slot0Call {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, EvmEnv::default(), pool, input)?;

        let call_return = IUniswapV3Pool::slot0Call::abi_decode_returns(&call_data_result)?;
        Ok(call_return)
    }
    pub fn liquidity<DB: DatabaseRef<Error = KabuDBError> + ?Sized>(db: &DB, pool: Address) -> eyre::Result<u128> {
        let input = IUniswapV3Pool::IUniswapV3PoolCalls::liquidity(IUniswapV3Pool::liquidityCall {}).abi_encode();
        let (call_data_result, _, _) = evm_call(db, EvmEnv::default(), pool, input)?;

        let call_return = IUniswapV3Pool::liquidityCall::abi_decode_returns(&call_data_result)?;
        Ok(call_return)
    }
}
