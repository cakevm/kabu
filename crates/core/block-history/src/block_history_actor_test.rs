#[cfg(test)]
mod test {
    use alloy_eips::BlockNumberOrTag;
    use alloy_network::Ethereum;
    use alloy_node_bindings::Anvil;
    use alloy_primitives::{Address, B256, U256};
    use alloy_provider::Provider;
    use alloy_provider::ProviderBuilder;
    use alloy_provider::ext::AnvilApi;
    use alloy_rpc_client::ClientBuilder;
    use eyre::eyre;
    use kabu_core_blockchain::{Blockchain, BlockchainState};
    use kabu_evm_db::{DatabaseKabuExt, KabuDB, KabuDBType};
    use kabu_evm_utils::geth_state_update::{
        account_state_add_storage, account_state_with_nonce_and_balance, geth_state_update_add_account,
    };
    use kabu_node_debug_provider::DebugProviderExt;
    use kabu_types_blockchain::{ChainParameters, GethStateUpdate, GethStateUpdateVec};
    use kabu_types_market::MarketState;
    use reth_chain_state::{CanonStateNotification, CanonStateSubscriptions};
    use reth_ethereum_primitives::EthPrimitives;
    use reth_execution_types::Chain;
    use reth_primitives_traits::block::RecoveredBlock;
    use reth_rpc_convert::TryFromBlockResponse;
    use revm::DatabaseRef;
    use revm::database::BundleState;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::broadcast;
    use tracing::error;
    use tracing::info;

    // Mock provider that can send canonical state notifications for testing
    #[derive(Clone)]
    struct MockCanonicalProvider<P> {
        _phantom: std::marker::PhantomData<P>,
        canon_state_sender: broadcast::Sender<CanonStateNotification<EthPrimitives>>,
    }

    impl<P> MockCanonicalProvider<P> {
        fn new(_inner: P) -> Self {
            let (canon_state_sender, _) = broadcast::channel(256);
            Self { _phantom: std::marker::PhantomData, canon_state_sender }
        }

        async fn send_block_as_canonical(
            &self,
            block: reth_ethereum_primitives::Block,
            _state_update: Option<GethStateUpdateVec>,
        ) -> eyre::Result<()> {
            // Convert block to RecoveredBlock (with empty senders for testing)
            let senders = vec![Address::ZERO; block.body.transactions.len()];
            let recovered_block = RecoveredBlock::new_unhashed(block, senders);

            // Create a simple execution outcome for testing
            let bundle_state = BundleState::default();
            let execution_outcome = reth_execution_types::ExecutionOutcome {
                bundle: bundle_state,
                receipts: vec![vec![]],
                first_block: recovered_block.header().number,
                requests: vec![],
            };

            // Create and send the canonical state notification
            let chain = Chain::new(vec![recovered_block], execution_outcome, None);
            let notification = CanonStateNotification::Commit { new: Arc::new(chain) };

            let _ = self.canon_state_sender.send(notification);
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(())
        }
    }

    impl<P> CanonStateSubscriptions for MockCanonicalProvider<P>
    where
        P: Send + Sync,
        MockCanonicalProvider<P>: reth_storage_api::NodePrimitivesProvider<Primitives = EthPrimitives>,
    {
        fn subscribe_to_canonical_state(&self) -> reth_chain_state::CanonStateNotifications<EthPrimitives> {
            self.canon_state_sender.subscribe()
        }
    }

    // Implement NodePrimitivesProvider trait for MockCanonicalProvider with EthPrimitives
    impl<P> reth_storage_api::NodePrimitivesProvider for MockCanonicalProvider<P>
    where
        P: Send + Sync,
    {
        type Primitives = EthPrimitives;
    }

    async fn broadcast_latest_block<P>(
        provider: &P,
        mock_canonical: &MockCanonicalProvider<P>,
        _bc: &Blockchain<EthPrimitives>,
        state_update: Option<GethStateUpdateVec>,
    ) -> eyre::Result<()>
    where
        P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    {
        let block_response = provider.get_block_by_number(BlockNumberOrTag::Latest).full().await?.unwrap();

        // Convert RPC block to primitive block
        let block = <reth_ethereum_primitives::Block as TryFromBlockResponse<Ethereum>>::from_block_response(block_response.clone())
            .map_err(|e| eyre!("Failed to convert block response: {}", e))?;

        // Send the block through canonical state notifications
        mock_canonical.send_block_as_canonical(block, state_update).await
    }

    async fn test_actor_block_history_actor_chain_head_worker<P>(
        provider: P,
        mock_canonical: MockCanonicalProvider<P>,
        bc: Blockchain<EthPrimitives>,
        state: BlockchainState<KabuDB, EthPrimitives>,
    ) -> eyre::Result<()>
    where
        P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    {
        const ADDR_01: Address = Address::repeat_byte(1);
        let cell_01: B256 = B256::from(U256::from_limbs([1, 0, 0, 0]));
        let value_02: B256 = B256::from(U256::from_limbs([2, 0, 0, 0]));
        let value_03: B256 = B256::from(U256::from_limbs([3, 0, 0, 0]));

        let account_1 = account_state_add_storage(account_state_with_nonce_and_balance(1, U256::from(2)), cell_01, value_02);

        let state_0 = geth_state_update_add_account(GethStateUpdate::default(), ADDR_01, account_1);

        let state_update_0 = vec![state_0];

        let mut db = KabuDBType::default();
        db.apply_geth_update_vec(state_update_0);

        state.market_state().write().await.state_db = db;

        let account_01 = state.market_state().read().await.state_db.clone().load_account(ADDR_01).cloned()?;
        assert_eq!(account_01.info.nonce, 1);
        assert_eq!(account_01.info.balance, U256::from(2));
        for (k, v) in account_01.storage.iter() {
            print!("{k} {v}")
        }
        let state_1 =
            geth_state_update_add_account(GethStateUpdate::default(), ADDR_01, account_state_with_nonce_and_balance(2, U256::from(3)));

        broadcast_latest_block(&provider, &mock_canonical, &bc, Some(vec![state_1])).await?; // block 0

        // Check state after first block update
        tokio::time::sleep(Duration::from_millis(1000)).await;
        //let account_01 = bc.market_state().read().await.state_db.clone().load_account(ADDR_01).cloned()?;
        assert_eq!(state.market_state().read().await.state_db.basic_ref(ADDR_01)?.unwrap().nonce, 2);
        assert_eq!(state.market_state().read().await.state_db.basic_ref(ADDR_01)?.unwrap().balance, U256::from(3));
        assert_eq!(state.market_state().read().await.state_db.storage_ref(ADDR_01, U256::from(1))?, U256::from(2));

        let snap = provider.anvil_snapshot().await?;
        provider.anvil_mine(Some(1), None).await?; // mine block 1#0
        broadcast_latest_block(&provider, &mock_canonical, &bc, None).await?; // broadcast 1#0

        provider.anvil_mine(Some(1), None).await?; // mine block 2#0
        let block_2_0 = provider.get_block_by_number(BlockNumberOrTag::Latest).full().await?.unwrap();

        broadcast_latest_block(&provider, &mock_canonical, &bc, None).await?; // broadcast 2#0

        provider.anvil_mine(Some(1), None).await?; // mine block 3#0
        let block_3_0 = provider.get_block_by_number(BlockNumberOrTag::Latest).full().await?.unwrap();

        provider.anvil_revert(snap).await?;
        provider.anvil_mine(Some(1), None).await?; // mine block 1#1

        let account_1_1 = account_state_add_storage(account_state_with_nonce_and_balance(4, U256::from(5)), cell_01, value_03);
        let state_1_1 = geth_state_update_add_account(GethStateUpdate::default(), ADDR_01, account_1_1);
        broadcast_latest_block(&provider, &mock_canonical, &bc, Some(vec![state_1_1])).await?; // broadcast 1#1

        assert_eq!(state.market_state().read().await.state_db.basic_ref(ADDR_01)?.unwrap().nonce, 2);
        assert_eq!(state.market_state().read().await.state_db.basic_ref(ADDR_01)?.unwrap().balance, U256::from(3));
        assert_eq!(state.market_state().read().await.state_db.storage_ref(ADDR_01, U256::from(1))?, U256::from(2));

        provider.anvil_mine(Some(1), None).await?; // mine block 2#1
        let block_2_1 = provider.get_block_by_number(BlockNumberOrTag::Latest).full().await?.unwrap();

        broadcast_latest_block(&provider, &mock_canonical, &bc, None).await?; // broadcast 2#1, chain_head must change

        tokio::time::sleep(Duration::from_millis(1000)).await;

        assert_eq!(state.market_state().read().await.state_db.basic_ref(ADDR_01)?.unwrap().nonce, 4);
        assert_eq!(state.market_state().read().await.state_db.basic_ref(ADDR_01)?.unwrap().balance, U256::from(5));
        assert_eq!(state.market_state().read().await.state_db.storage_ref(ADDR_01, U256::from(1))?, U256::from(3));

        assert_eq!(state.block_history().read().await.latest_block_number, block_2_1.header.number);
        assert_eq!(
            state.block_history().read().await.get_block_hash_for_block_number(block_2_1.header.number).unwrap(),
            block_2_1.header.hash
        );

        // Convert RPC block to primitive block
        let block_3_0_primitive =
            <reth_ethereum_primitives::Block as TryFromBlockResponse<Ethereum>>::from_block_response(block_3_0.clone())
                .map_err(|e| eyre!("Failed to convert block response: {}", e))?;

        // Send block 3#0 through canonical state to trigger reorg
        mock_canonical.send_block_as_canonical(block_3_0_primitive, Some(vec![])).await?; // broadcast 3#0, chain_head must change

        assert_eq!(state.block_history().read().await.latest_block_number, block_3_0.header.number);
        assert_eq!(
            state.block_history().read().await.get_block_hash_for_block_number(block_3_0.header.number).unwrap(),
            block_3_0.header.hash
        );
        assert_eq!(
            state.block_history().read().await.get_block_hash_for_block_number(block_2_0.header.number).unwrap(),
            block_2_0.header.hash
        );
        tokio::time::sleep(Duration::from_millis(1000)).await;

        assert_eq!(state.market_state().read().await.state_db.basic_ref(ADDR_01)?.unwrap().nonce, 2);
        assert_eq!(state.market_state().read().await.state_db.basic_ref(ADDR_01)?.unwrap().balance, U256::from(3));
        assert_eq!(state.market_state().read().await.state_db.storage_ref(ADDR_01, U256::from(1))?, U256::from(2));

        Ok(())
    }

    #[tokio::test]
    async fn test_actor_block_history_actor_chain_head() -> eyre::Result<()> {
        let _ = env_logger::try_init_from_env(env_logger::Env::default().default_filter_or(
            "debug,kabu_types_entities::block_history=trace,tokio_tungstenite=off,tungstenite=off,hyper_util=off,alloy_transport_http=off",
        ));

        let anvil = Anvil::new().try_spawn()?;
        let client_anvil = ClientBuilder::default().http(anvil.endpoint_url());
        let provider = ProviderBuilder::new().disable_recommended_fillers().connect_client(client_anvil);

        let blockchain = Blockchain::new(1);
        let market_state = MarketState::new(KabuDB::empty());
        let bc_state = BlockchainState::<KabuDB, EthPrimitives>::new_with_market_state(market_state);

        // Create mock canonical provider for testing
        let mock_canonical = MockCanonicalProvider::new(provider.clone());

        // Start the block history component with the canonical provider
        let chain_parameters = ChainParameters::ethereum();
        let market_state_arc = bc_state.market_state();
        let block_history = bc_state.block_history();
        let market_events_tx = blockchain.market_events_channel();

        let mock_canonical_clone = mock_canonical.clone();
        let provider_clone = provider.clone();
        tokio::task::spawn(async move {
            use crate::block_history_actor::new_block_history_worker_canonical;
            if let Err(e) = new_block_history_worker_canonical(
                mock_canonical_clone,
                provider_clone,
                chain_parameters,
                market_state_arc,
                block_history,
                market_events_tx,
            )
            .await
            {
                error!("Block history worker failed: {}", e);
            }
        });

        // Run the test worker
        let bc = blockchain.clone();
        tokio::task::spawn(async move {
            if let Err(e) = test_actor_block_history_actor_chain_head_worker(provider, mock_canonical, bc, bc_state).await {
                error!("test_worker : {}", e);
            } else {
                info!("test_worker finished");
            }
        });

        let mut rx = blockchain.market_events_channel().subscribe();
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    info!("{:?}", msg)
                }
                _ = tokio::time::sleep(Duration::from_millis(10000)) => {
                    break;
                }
            }
        }

        Ok(())
    }

    async fn test_actor_block_history_actor_reorg_worker<P>(
        provider: P,
        mock_canonical: MockCanonicalProvider<P>,
        bc: Blockchain<EthPrimitives>,
    ) -> eyre::Result<()>
    where
        P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    {
        let snap = provider.anvil_snapshot().await?;

        broadcast_latest_block(&provider, &mock_canonical, &bc, None).await?; // block 0
        provider.anvil_mine(Some(1), None).await?; // mine block 1#0
        provider.anvil_mine(Some(1), None).await?; // mine block 2#0
        broadcast_latest_block(&provider, &mock_canonical, &bc, None).await?; // block 2#0

        provider.anvil_revert(snap).await?;

        provider.anvil_mine(Some(1), None).await?; // mine block 1#1
        broadcast_latest_block(&provider, &mock_canonical, &bc, None).await?;
        provider.anvil_mine(Some(1), None).await?; // mine block 2#1
        broadcast_latest_block(&provider, &mock_canonical, &bc, None).await?;
        provider.anvil_mine(Some(1), None).await.map_err(|_| eyre!("3#1"))?; // mine block 3#1
        broadcast_latest_block(&provider, &mock_canonical, &bc, None).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_actor_block_history_actor_reorg() -> eyre::Result<()> {
        let _ = env_logger::try_init_from_env(env_logger::Env::default().default_filter_or("info,tokio_tungstenite=off,tungstenite=off"));

        let anvil = Anvil::new().try_spawn()?;
        let client_anvil = ClientBuilder::default().http(anvil.endpoint_url());
        let provider = ProviderBuilder::new().disable_recommended_fillers().connect_client(client_anvil);

        let blockchain = Blockchain::new(1);
        let market_state = MarketState::new(KabuDB::empty());
        let bc_state = BlockchainState::<KabuDB, EthPrimitives>::new_with_market_state(market_state);

        // Create mock canonical provider for testing
        let mock_canonical = MockCanonicalProvider::new(provider.clone());

        // Start the block history component with the canonical provider
        let chain_parameters = ChainParameters::ethereum();
        let market_state_arc = bc_state.market_state();
        let block_history = bc_state.block_history();
        let market_events_tx = blockchain.market_events_channel();

        let mock_canonical_clone = mock_canonical.clone();
        let provider_clone = provider.clone();
        tokio::task::spawn(async move {
            use crate::block_history_actor::new_block_history_worker_canonical;
            if let Err(e) = new_block_history_worker_canonical(
                mock_canonical_clone,
                provider_clone,
                chain_parameters,
                market_state_arc,
                block_history,
                market_events_tx,
            )
            .await
            {
                error!("Block history worker failed: {}", e);
            }
        });

        let bc = blockchain.clone();
        tokio::task::spawn(async move {
            if let Err(e) = test_actor_block_history_actor_reorg_worker(provider, mock_canonical, bc).await {
                error!("test_worker : {}", e);
            } else {
                info!("test_worker finished");
            }
        });

        let mut rx = blockchain.market_events_channel().subscribe();
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    info!("{:?}", msg)
                }
                _ = tokio::time::sleep(Duration::from_millis(1000)) => {
                    break;
                }
            }
        }

        // Give the worker time to process all the canonical state notifications
        tokio::time::sleep(Duration::from_millis(500)).await;

        let block_history = bc_state.block_history().clone();
        let block_history = block_history.read().await;
        assert_eq!(block_history.len(), 6);

        Ok(())
    }
}
