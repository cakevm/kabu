use alloy_evm::EvmEnv;
use alloy_primitives::U256;
use alloy_primitives::utils::parse_units;
use kabu_evm_db::KabuDBError;
use kabu_types_swap::{SwapError, SwapErrorKind, SwapLine};
use lazy_static::lazy_static;
use revm::DatabaseRef;

lazy_static! {
    static ref START_OPTIMIZE_INPUT: U256 = parse_units("0.01", "ether").unwrap().get_absolute();
}

pub struct SwapCalculator {}

impl SwapCalculator {
    #[inline]
    #[allow(clippy::result_large_err)]
    pub fn calculate<'a, DB: DatabaseRef<Error = KabuDBError>>(
        path: &'a mut SwapLine,
        db: &DB,
        evm_env: EvmEnv,
    ) -> eyre::Result<&'a mut SwapLine, SwapError> {
        let first_token = path.get_first_token().unwrap();
        if let Some(amount_in) = first_token.calc_token_value_from_eth(*START_OPTIMIZE_INPUT) {
            //trace!("calculate : {} amount in : {}",first_token.get_symbol(), first_token.to_float(amount_in) );
            path.optimize_with_in_amount(db, evm_env, amount_in)
        } else {
            Err(path.to_error(SwapErrorKind::CalculationError("PRICE_NOT_SET".to_string())))
        }
    }
}
