use alloy_primitives::{Address, U256, hex};
use eyre::{Report, eyre};
use kabu_types_market::SwapPath;
use kabu_types_market::{PoolError, PoolId};
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub struct EstimationError {
    pub msg: String,
    pub swap_path: SwapPath,
}

impl PartialEq<Self> for EstimationError {
    fn eq(&self, other: &Self) -> bool {
        self.swap_path == other.swap_path
    }
}

impl Eq for EstimationError {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SwapErrorKind {
    // Amount-related errors
    ZeroOutAmount,
    AlmostZeroOutAmount,
    ZeroInAmount,
    MaxInAmount,
    InsufficientLiquidity,

    // Pool state errors
    UninitializedPool,
    ZeroReserves,
    InvalidReserves,
    PoolNotFound,

    // ABI/Contract errors
    AbiEncodingFailed,
    AbiDecodingFailed { method: &'static str, reason: AbiError },
    ContractCallFailed { method: &'static str, revert_reason: Option<Vec<u8>> },

    // Calculation errors
    OverflowInCalculation,
    UnderflowInCalculation,
    InvalidK,
    InvalidSqrtPrice,

    // Token-related errors
    InvalidTokenOrder,
    TokenNotInPool,
    SameTokenSwap,

    // UniswapV3 specific
    InvalidTickRange,
    NoLiquidityInRange,
    PriceSlippageExceeded,

    // Curve specific
    InvalidAmplificationParameter,
    ConvergenceFailure,
    InvalidPoolType,

    // Generic pool errors
    UnsupportedPoolType,
    InvalidSwapPath,

    // Catch-all for other errors
    CalculationError(String),
}

impl From<PoolError> for SwapErrorKind {
    fn from(err: PoolError) -> Self {
        use kabu_types_market::{CurveError, MaverickError, UniswapV2Error, UniswapV3Error};

        match err {
            // UniswapV2 errors
            PoolError::UniswapV2(e) => match e {
                UniswapV2Error::OutAmountIsZero => SwapErrorKind::ZeroOutAmount,
                UniswapV2Error::InAmountIsZero => SwapErrorKind::ZeroInAmount,
                UniswapV2Error::ReserveExceeded | UniswapV2Error::ReserveOutExceeded => SwapErrorKind::InsufficientLiquidity,
                UniswapV2Error::AmountInWithFeeOverflow
                | UniswapV2Error::NumeratorOverflow
                | UniswapV2Error::NumeratorOverflowFee
                | UniswapV2Error::DenominatorOverflow
                | UniswapV2Error::DenominatorOverflowFee
                | UniswapV2Error::InAmountOverflow => SwapErrorKind::OverflowInCalculation,
                UniswapV2Error::DenominatorUnderflow => SwapErrorKind::UnderflowInCalculation,
                UniswapV2Error::CannotCalculateZeroReserve => SwapErrorKind::ZeroReserves,
            },

            // UniswapV3 errors
            PoolError::UniswapV3(e) => match e {
                UniswapV3Error::OutAmountIsZero => SwapErrorKind::ZeroOutAmount,
                UniswapV3Error::InAmountIsZero => SwapErrorKind::ZeroInAmount,
                UniswapV3Error::InvalidTick => SwapErrorKind::InvalidTickRange,
                UniswapV3Error::NoLiquidityInRange => SwapErrorKind::NoLiquidityInRange,
                UniswapV3Error::PriceOutOfBounds => SwapErrorKind::PriceSlippageExceeded,
                UniswapV3Error::InvalidSqrtPrice | UniswapV3Error::BadPriceStep => SwapErrorKind::InvalidSqrtPrice,
                UniswapV3Error::OverflowInCalculation => SwapErrorKind::OverflowInCalculation,
                UniswapV3Error::UnderflowInCalculation => SwapErrorKind::UnderflowInCalculation,
            },

            // Curve errors
            PoolError::Curve(e) => match e {
                CurveError::OutAmountIsZero => SwapErrorKind::ZeroOutAmount,
                CurveError::InAmountIsZero => SwapErrorKind::ZeroInAmount,
                CurveError::TokenNotInPool { .. } => SwapErrorKind::TokenNotInPool,
                CurveError::InvalidPoolState | CurveError::CannotGetBalance | CurveError::UnderlyingCoinNotFound => {
                    SwapErrorKind::InvalidReserves
                }
            },

            // Maverick errors
            PoolError::Maverick(e) => match e {
                MaverickError::MaxInAmount => SwapErrorKind::MaxInAmount,
                MaverickError::OutAmountIsZero => SwapErrorKind::ZeroOutAmount,
                MaverickError::InAmountIsZero => SwapErrorKind::ZeroInAmount,
                MaverickError::ReserveOutExceeded => SwapErrorKind::InsufficientLiquidity,
            },

            // ABI errors
            PoolError::AbiDecodingError { method, source } => SwapErrorKind::AbiDecodingFailed {
                method,
                reason: match source {
                    alloy_sol_types::Error::Overrun => AbiError::BufferOverrun,
                    alloy_sol_types::Error::BufferNotEmpty => AbiError::BufferOverrun,
                    alloy_sol_types::Error::UnknownSelector { .. } => AbiError::InvalidSelector,
                    alloy_sol_types::Error::TypeCheckFail { .. } => AbiError::Other,
                    _ => AbiError::Other,
                },
            },

            // EVM revert errors
            PoolError::EvmReverted { reason, .. } => SwapErrorKind::CalculationError(reason.clone()),

            // Generic errors
            PoolError::NotImplemented => SwapErrorKind::UnsupportedPoolType,
            PoolError::InvalidInput { reason } => SwapErrorKind::CalculationError(reason.to_string()),
            PoolError::DatabaseError(e) => SwapErrorKind::CalculationError(format!("Database error: {e}")),
            PoolError::GeneralError(e) => SwapErrorKind::CalculationError(e.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AbiError {
    BufferOverrun,
    InvalidReturnDataSize { expected: usize, actual: usize },
    InvalidSelector,
    MissingReturnData,
    InvalidUtf8,
    Other,
}

impl fmt::Display for AbiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AbiError::BufferOverrun => write!(f, "buffer overrun while deserializing"),
            AbiError::InvalidReturnDataSize { expected, actual } => {
                write!(f, "invalid return data size: expected {expected}, got {actual}")
            }
            AbiError::InvalidSelector => write!(f, "invalid function selector"),
            AbiError::MissingReturnData => write!(f, "missing return data"),
            AbiError::InvalidUtf8 => write!(f, "invalid UTF-8 in return data"),
            AbiError::Other => write!(f, "unknown ABI error"),
        }
    }
}

impl fmt::Display for SwapErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Amount errors
            SwapErrorKind::ZeroOutAmount => write!(f, "output amount is zero"),
            SwapErrorKind::AlmostZeroOutAmount => write!(f, "output amount below minimum threshold"),
            SwapErrorKind::ZeroInAmount => write!(f, "input amount is zero"),
            SwapErrorKind::MaxInAmount => write!(f, "input amount is MAX_UINT256"),
            SwapErrorKind::InsufficientLiquidity => write!(f, "insufficient liquidity for swap"),

            // Pool state errors
            SwapErrorKind::UninitializedPool => write!(f, "pool is not initialized"),
            SwapErrorKind::ZeroReserves => write!(f, "pool has zero reserves"),
            SwapErrorKind::InvalidReserves => write!(f, "pool reserves are invalid"),
            SwapErrorKind::PoolNotFound => write!(f, "pool not found"),

            // ABI/Contract errors
            SwapErrorKind::AbiEncodingFailed => write!(f, "ABI encoding failed"),
            SwapErrorKind::AbiDecodingFailed { method, reason } => write!(f, "ABI decoding failed for {method}: {reason}"),
            SwapErrorKind::ContractCallFailed { method, revert_reason } => {
                if let Some(data) = revert_reason {
                    write!(f, "contract call failed for {}: 0x{}", method, hex::encode(data))
                } else {
                    write!(f, "contract call failed for {method}")
                }
            }

            // Calculation errors
            SwapErrorKind::OverflowInCalculation => write!(f, "overflow in calculation"),
            SwapErrorKind::UnderflowInCalculation => write!(f, "underflow in calculation"),
            SwapErrorKind::InvalidK => write!(f, "invalid constant product K"),
            SwapErrorKind::InvalidSqrtPrice => write!(f, "invalid sqrt price"),

            // Token errors
            SwapErrorKind::InvalidTokenOrder => write!(f, "invalid token order"),
            SwapErrorKind::TokenNotInPool => write!(f, "token not in pool"),
            SwapErrorKind::SameTokenSwap => write!(f, "cannot swap same token"),

            // UniswapV3 specific
            SwapErrorKind::InvalidTickRange => write!(f, "invalid tick range"),
            SwapErrorKind::NoLiquidityInRange => write!(f, "no liquidity in tick range"),
            SwapErrorKind::PriceSlippageExceeded => write!(f, "price slippage exceeded"),

            // Curve specific
            SwapErrorKind::InvalidAmplificationParameter => write!(f, "invalid amplification parameter"),
            SwapErrorKind::ConvergenceFailure => write!(f, "numerical convergence failure"),
            SwapErrorKind::InvalidPoolType => write!(f, "invalid Curve pool type"),

            // Generic
            SwapErrorKind::UnsupportedPoolType => write!(f, "unsupported pool type"),
            SwapErrorKind::InvalidSwapPath => write!(f, "invalid swap path"),
            SwapErrorKind::CalculationError(msg) => write!(f, "{msg}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SwapError {
    pub kind: SwapErrorKind,
    pub pool: PoolId,
    pub token_from: Address,
    pub token_to: Address,
    pub is_in_amount: bool,
    pub amount: U256,
}

impl From<SwapError> for Report {
    fn from(value: SwapError) -> Self {
        eyre!("SwapError::{:?} at pool {:?}: {}", value.kind, value.pool, value.kind)
    }
}

impl fmt::Display for SwapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SwapError {{ kind: {}, pool: {:?}, {} -> {}, amount: {} }}",
            self.kind, self.pool, self.token_from, self.token_to, self.amount
        )
    }
}

impl Hash for SwapError {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pool.hash(state);
        self.token_from.hash(state);
        self.token_to.hash(state);
    }
}

impl PartialEq<Self> for SwapError {
    fn eq(&self, other: &Self) -> bool {
        self.pool == other.pool && self.token_to == other.token_to && self.token_from == other.token_from
    }
}

impl Eq for SwapError {}
