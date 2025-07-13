use alloy_primitives::{Address, U256};

use serde::{Deserialize, Serialize};
use utoipa::openapi::schema::SchemaType;
use utoipa::openapi::{Array, Object, ToArray, Type};
use utoipa::PartialSchema;
use utoipa::{schema, ToSchema};

#[derive(Debug, Serialize, ToSchema)]
pub struct PoolResponse {
    pub pools: Vec<Pool>,
    pub total: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PoolDetailsResponse {
    #[schema(schema_with = String::schema)]
    pub address: Address,
    pub protocol: PoolProtocol,
    pub pool_class: PoolClass,
    #[schema(schema_with = String::schema)]
    pub fee: U256,
    #[schema(schema_with = array_of_strings)]
    pub tokens: Vec<Address>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Pool {
    #[schema(schema_with = String::schema)]
    pub address: Address,
    #[schema(schema_with = String::schema)]
    pub fee: U256,
    #[schema(schema_with = array_of_strings)]
    pub tokens: Vec<Address>,
    pub protocol: PoolProtocol,
    pub pool_class: PoolClass,
}

pub fn array_of_strings() -> Array {
    Object::with_type(SchemaType::Type(Type::String)).to_array()
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PoolClass {
    Unknown,
    UniswapV2,
    UniswapV3,
    UniswapV4,
    PancakeV3,
    Maverick,
    MaverickV2,
    Curve,
    LidoStEth,
    LidoWstEth,
    RocketPool,
    BalancerV1,
    BalancerV2,
    Custom(u64),
}
impl From<kabu_types_entities::PoolClass> for PoolClass {
    fn from(pool_class: kabu_types_entities::PoolClass) -> Self {
        match pool_class {
            kabu_types_entities::PoolClass::Unknown => PoolClass::Unknown,
            kabu_types_entities::PoolClass::UniswapV2 => PoolClass::UniswapV2,
            kabu_types_entities::PoolClass::UniswapV3 => PoolClass::UniswapV3,
            kabu_types_entities::PoolClass::UniswapV4 => PoolClass::UniswapV4,
            kabu_types_entities::PoolClass::PancakeV3 => PoolClass::PancakeV3,
            kabu_types_entities::PoolClass::Maverick => PoolClass::Maverick,
            kabu_types_entities::PoolClass::MaverickV2 => PoolClass::MaverickV2,
            kabu_types_entities::PoolClass::Curve => PoolClass::Curve,
            kabu_types_entities::PoolClass::LidoStEth => PoolClass::LidoStEth,
            kabu_types_entities::PoolClass::LidoWstEth => PoolClass::LidoWstEth,
            kabu_types_entities::PoolClass::RocketPool => PoolClass::RocketPool,
            kabu_types_entities::PoolClass::BalancerV1 => PoolClass::BalancerV1,
            kabu_types_entities::PoolClass::BalancerV2 => PoolClass::BalancerV2,
            kabu_types_entities::PoolClass::Custom(id) => PoolClass::Custom(id),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PoolProtocol {
    Unknown,
    AaveV2,
    AaveV3,
    UniswapV2,
    UniswapV2Like,
    NomiswapStable,
    Sushiswap,
    SushiswapV3,
    DooarSwap,
    Safeswap,
    Miniswap,
    Shibaswap,
    UniswapV3,
    UniswapV3Like,
    UniswapV4,
    PancakeV3,
    Integral,
    Maverick,
    MaverickV2,
    Curve,
    LidoStEth,
    LidoWstEth,
    RocketEth,
    OgPepe,
    AntFarm,
    BalancerV1,
    BalancerV2,
    Custom(u64),
}

impl From<kabu_types_entities::PoolProtocol> for PoolProtocol {
    fn from(protocol: kabu_types_entities::PoolProtocol) -> Self {
        match protocol {
            kabu_types_entities::PoolProtocol::AaveV2 => PoolProtocol::AaveV2,
            kabu_types_entities::PoolProtocol::AaveV3 => PoolProtocol::AaveV3,
            kabu_types_entities::PoolProtocol::Unknown => PoolProtocol::Unknown,
            kabu_types_entities::PoolProtocol::UniswapV2 => PoolProtocol::UniswapV2,
            kabu_types_entities::PoolProtocol::UniswapV2Like => PoolProtocol::UniswapV2Like,
            kabu_types_entities::PoolProtocol::NomiswapStable => PoolProtocol::NomiswapStable,
            kabu_types_entities::PoolProtocol::Sushiswap => PoolProtocol::Sushiswap,
            kabu_types_entities::PoolProtocol::SushiswapV3 => PoolProtocol::SushiswapV3,
            kabu_types_entities::PoolProtocol::DooarSwap => PoolProtocol::DooarSwap,
            kabu_types_entities::PoolProtocol::Safeswap => PoolProtocol::Safeswap,
            kabu_types_entities::PoolProtocol::Miniswap => PoolProtocol::Miniswap,
            kabu_types_entities::PoolProtocol::Shibaswap => PoolProtocol::Shibaswap,
            kabu_types_entities::PoolProtocol::UniswapV3 => PoolProtocol::UniswapV3,
            kabu_types_entities::PoolProtocol::UniswapV3Like => PoolProtocol::UniswapV3Like,
            kabu_types_entities::PoolProtocol::UniswapV4 => PoolProtocol::UniswapV4,
            kabu_types_entities::PoolProtocol::PancakeV3 => PoolProtocol::PancakeV3,
            kabu_types_entities::PoolProtocol::Integral => PoolProtocol::Integral,
            kabu_types_entities::PoolProtocol::Maverick => PoolProtocol::Maverick,
            kabu_types_entities::PoolProtocol::MaverickV2 => PoolProtocol::MaverickV2,
            kabu_types_entities::PoolProtocol::Curve => PoolProtocol::Curve,
            kabu_types_entities::PoolProtocol::LidoStEth => PoolProtocol::LidoStEth,
            kabu_types_entities::PoolProtocol::LidoWstEth => PoolProtocol::LidoWstEth,
            kabu_types_entities::PoolProtocol::RocketEth => PoolProtocol::RocketEth,
            kabu_types_entities::PoolProtocol::OgPepe => PoolProtocol::OgPepe,
            kabu_types_entities::PoolProtocol::AntFarm => PoolProtocol::AntFarm,
            kabu_types_entities::PoolProtocol::BalancerV1 => PoolProtocol::BalancerV1,
            kabu_types_entities::PoolProtocol::BalancerV2 => PoolProtocol::BalancerV2,
            kabu_types_entities::PoolProtocol::Custom(id) => PoolProtocol::Custom(id),
        }
    }
}

impl From<&PoolProtocol> for kabu_types_entities::PoolProtocol {
    fn from(protocol: &PoolProtocol) -> Self {
        match protocol {
            PoolProtocol::Unknown => kabu_types_entities::PoolProtocol::Unknown,
            PoolProtocol::AaveV2 => kabu_types_entities::PoolProtocol::AaveV2,
            PoolProtocol::AaveV3 => kabu_types_entities::PoolProtocol::AaveV3,
            PoolProtocol::UniswapV2 => kabu_types_entities::PoolProtocol::UniswapV2,
            PoolProtocol::UniswapV2Like => kabu_types_entities::PoolProtocol::UniswapV2Like,
            PoolProtocol::NomiswapStable => kabu_types_entities::PoolProtocol::NomiswapStable,
            PoolProtocol::Sushiswap => kabu_types_entities::PoolProtocol::Sushiswap,
            PoolProtocol::SushiswapV3 => kabu_types_entities::PoolProtocol::SushiswapV3,
            PoolProtocol::DooarSwap => kabu_types_entities::PoolProtocol::DooarSwap,
            PoolProtocol::Safeswap => kabu_types_entities::PoolProtocol::Safeswap,
            PoolProtocol::Miniswap => kabu_types_entities::PoolProtocol::Miniswap,
            PoolProtocol::Shibaswap => kabu_types_entities::PoolProtocol::Shibaswap,
            PoolProtocol::UniswapV3 => kabu_types_entities::PoolProtocol::UniswapV3,
            PoolProtocol::UniswapV3Like => kabu_types_entities::PoolProtocol::UniswapV3Like,
            PoolProtocol::UniswapV4 => kabu_types_entities::PoolProtocol::UniswapV4,
            PoolProtocol::PancakeV3 => kabu_types_entities::PoolProtocol::PancakeV3,
            PoolProtocol::Integral => kabu_types_entities::PoolProtocol::Integral,
            PoolProtocol::Maverick => kabu_types_entities::PoolProtocol::Maverick,
            PoolProtocol::MaverickV2 => kabu_types_entities::PoolProtocol::MaverickV2,
            PoolProtocol::Curve => kabu_types_entities::PoolProtocol::Curve,
            PoolProtocol::LidoStEth => kabu_types_entities::PoolProtocol::LidoStEth,
            PoolProtocol::LidoWstEth => kabu_types_entities::PoolProtocol::LidoWstEth,
            PoolProtocol::RocketEth => kabu_types_entities::PoolProtocol::RocketEth,
            PoolProtocol::OgPepe => kabu_types_entities::PoolProtocol::OgPepe,
            PoolProtocol::AntFarm => kabu_types_entities::PoolProtocol::AntFarm,
            PoolProtocol::BalancerV1 => kabu_types_entities::PoolProtocol::BalancerV1,
            PoolProtocol::BalancerV2 => kabu_types_entities::PoolProtocol::BalancerV2,
            PoolProtocol::Custom(id) => kabu_types_entities::PoolProtocol::Custom(*id),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MarketStats {
    pub total_pools: usize,
}
