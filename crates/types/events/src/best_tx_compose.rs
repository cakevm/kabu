use crate::SwapComposeData;
use alloy_primitives::U256;
use reth_ethereum_primitives::EthPrimitives;
use reth_node_types::NodePrimitives;

#[derive(Default)]
pub struct BestTxSwapCompose<DB, N: NodePrimitives = EthPrimitives> {
    validity_pct: Option<U256>,
    best_profit_swap: Option<SwapComposeData<DB, N>>,
    best_profit_gas_ratio_swap: Option<SwapComposeData<DB, N>>,
    best_tips_swap: Option<SwapComposeData<DB, N>>,
    best_tips_gas_ratio_swap: Option<SwapComposeData<DB, N>>,
}

impl<DB: Clone + Default + 'static, N: NodePrimitives> BestTxSwapCompose<DB, N> {
    pub fn new_with_pct<T: Into<U256>>(validity_pct: T) -> Self {
        BestTxSwapCompose {
            validity_pct: Some(validity_pct.into()),
            best_profit_swap: None,
            best_profit_gas_ratio_swap: None,
            best_tips_swap: None,
            best_tips_gas_ratio_swap: None,
        }
    }

    pub fn check(&mut self, request: &SwapComposeData<DB, N>) -> bool {
        let mut is_ok = false;

        match &self.best_profit_swap {
            None => {
                self.best_profit_swap = Some(request.clone());
                is_ok = true;
            }
            Some(best_swap) => {
                if best_swap.swap.arb_profit_eth() < request.swap.arb_profit_eth() {
                    self.best_profit_swap = Some(request.clone());
                    is_ok = true;
                } else if let Some(pct) = self.validity_pct
                    && (best_swap.swap.arb_profit_eth() * pct) / U256::from(10000) < request.swap.arb_profit_eth()
                {
                    is_ok = true
                }
            }
        }

        if !is_ok && request.tips.is_some() {
            match &self.best_tips_swap {
                Some(best_swap) => {
                    if best_swap.tips.unwrap_or_default() < request.tips.unwrap_or_default() {
                        self.best_tips_swap = Some(request.clone());
                        is_ok = true;
                    } else if let Some(pct) = self.validity_pct
                        && (best_swap.tips.unwrap_or_default() * pct) / U256::from(10000) < request.tips.unwrap_or_default()
                    {
                        is_ok = true
                    }
                }
                None => {
                    self.best_tips_swap = Some(request.clone());
                    is_ok = true;
                }
            }
        }

        if !is_ok && request.tx_compose.gas != 0 {
            match &self.best_tips_gas_ratio_swap {
                Some(best_swap) => {
                    if best_swap.tips_gas_ratio() < request.tips_gas_ratio() {
                        self.best_tips_gas_ratio_swap = Some(request.clone());
                        is_ok = true;
                    } else if let Some(pct) = self.validity_pct
                        && (best_swap.tips_gas_ratio() * pct) / U256::from(10000) < request.tips_gas_ratio()
                    {
                        is_ok = true
                    }
                }
                None => {
                    self.best_tips_gas_ratio_swap = Some(request.clone());
                    is_ok = true;
                }
            }

            match &self.best_profit_gas_ratio_swap {
                Some(best_swap) => {
                    if best_swap.profit_eth_gas_ratio() < request.profit_eth_gas_ratio() {
                        self.best_profit_gas_ratio_swap = Some(request.clone());
                        is_ok = true;
                    } else if let Some(pct) = self.validity_pct
                        && (best_swap.profit_eth_gas_ratio() * pct) / U256::from(10000) < request.profit_eth_gas_ratio()
                    {
                        is_ok = true
                    }
                }
                None => {
                    self.best_profit_gas_ratio_swap = Some(request.clone());
                    is_ok = true;
                }
            }
        }
        is_ok
    }
}
