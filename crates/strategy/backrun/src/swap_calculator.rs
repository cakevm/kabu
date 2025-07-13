use alloy_primitives::utils::parse_units;
use alloy_primitives::U256;
use lazy_static::lazy_static;
use loom_evm_utils::LoomExecuteEvm;
use loom_types_entities::{SwapError, SwapLine};

lazy_static! {
    static ref START_OPTIMIZE_INPUT: U256 = parse_units("0.01", "ether").unwrap().get_absolute();
}

pub struct SwapCalculator {}

impl SwapCalculator {
    #[inline]
    #[allow(clippy::result_large_err)]
    pub fn calculate<'a>(path: &'a mut SwapLine, evm: &mut dyn LoomExecuteEvm) -> eyre::Result<&'a mut SwapLine, SwapError> {
        let first_token = path.get_first_token().unwrap();
        if let Some(amount_in) = first_token.calc_token_value_from_eth(*START_OPTIMIZE_INPUT) {
            //trace!("calculate : {} amount in : {}",first_token.get_symbol(), first_token.to_float(amount_in) );
            path.optimize_with_in_amount(evm, amount_in)
        } else {
            Err(path.to_error("PRICE_NOT_SET".to_string()))
        }
    }
}
