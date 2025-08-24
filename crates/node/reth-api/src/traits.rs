use reth_chain_state::CanonStateSubscriptions;
use reth_node_types::{BlockTy, HeaderTy, NodeTypesWithDB, ReceiptTy, TxTy};
use reth_provider::{
    BlockHashReader, BlockIdReader, BlockNumReader, BlockReader, ChainSpecProvider, DatabaseProviderFactory, HeaderProvider,
    NodePrimitivesProvider, ReceiptProvider, StateProviderFactory, TransactionsProvider,
};
use reth_storage_api::{BlockBodyIndicesProvider, BlockReaderIdExt, CanonChainTracker, ReceiptProviderIdExt};
use std::fmt::Debug;

/// A trait that matches exactly what `reth_storage_rpc_provider::RpcBlockchainProvider` implements as minimum requirements.
/// This provides all the necessary blockchain access methods for Kabu components.
pub trait KabuRethFullProvider<N: NodeTypesWithDB>:
    // Block and header reading
    BlockHashReader
    + BlockNumReader
    + BlockIdReader
    + HeaderProvider
    + BlockBodyIndicesProvider
    + BlockReader
    + BlockReaderIdExt<
        Transaction = TxTy<N>,
        Block = BlockTy<N>,
        Receipt = ReceiptTy<N>,
        Header = HeaderTy<N>,
    >
    // Receipt and transaction access
    + ReceiptProvider
    + ReceiptProviderIdExt
    + TransactionsProvider
    // State access
    + StateProviderFactory
    + DatabaseProviderFactory<DB = N::DB>
    // Chain tracking and subscriptions
    + CanonChainTracker
    + CanonStateSubscriptions
    // Node configuration
    + NodePrimitivesProvider<Primitives = N::Primitives>
    + ChainSpecProvider<ChainSpec = N::ChainSpec>
    // Base requirements
    + Clone
    + Debug
    + Send
    + Sync
    + 'static
{
}

/// Blanket implementation for any type that implements all the required traits
impl<T, N> KabuRethFullProvider<N> for T
where
    N: NodeTypesWithDB,
    T: BlockHashReader
        + BlockNumReader
        + BlockIdReader
        + HeaderProvider
        + BlockBodyIndicesProvider
        + BlockReader
        + BlockReaderIdExt<Transaction = TxTy<N>, Block = BlockTy<N>, Receipt = ReceiptTy<N>, Header = HeaderTy<N>>
        + ReceiptProvider
        + ReceiptProviderIdExt
        + TransactionsProvider
        + StateProviderFactory
        + DatabaseProviderFactory<DB = N::DB>
        + CanonChainTracker
        + CanonStateSubscriptions
        + NodePrimitivesProvider<Primitives = N::Primitives>
        + ChainSpecProvider<ChainSpec = N::ChainSpec>
        + Clone
        + Debug
        + Send
        + Sync
        + 'static,
{
}
