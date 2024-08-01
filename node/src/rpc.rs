//! A collection of node-specific RPC methods.
//! Substrate provides the `sc-rpc` crate, which defines the core RPC layer
//! used by Substrate nodes. This file extends those RPC definitions with
//! capabilities that are specific to this project's runtime configuration.

#![warn(missing_docs)]

use std::sync::Arc;

use jsonrpsee::RpcModule;
use node_subtensor_runtime::{opaque::Block, AccountId, Balance, BlockNumber, Hash, Index, Nonce};
use sc_consensus_grandpa::FinalityProofProvider;
use sc_transaction_pool_api::TransactionPool;
use sp_api::ProvideRuntimeApi;
use sp_block_builder::BlockBuilder;
use sp_blockchain::{Error as BlockChainError, HeaderBackend, HeaderMetadata};
use sc_transaction_pool::ChainApi;
pub use crate::eth::{create_eth, EthDeps};
use sp_inherents::CreateInherentDataProviders;
use sc_client_api::{
	backend::{Backend, StorageProvider},
	client::BlockchainEvents,
	AuxStore, UsageProvider,
};
use sp_runtime::traits::Block as BlockT;
use sc_rpc::SubscriptionTaskExecutor;
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_runtime::traits::BlakeTwo256;
use sp_runtime::OpaqueExtrinsic;

pub use sc_rpc_api::DenyUnsafe;

/// A type representing all RPC extensions.
pub type RpcExtension = jsonrpsee::RpcModule<()>;

/// Dependencies for GRANDPA
pub struct GrandpaDeps<B> {
    /// Voting round info.
    pub shared_voter_state: sc_consensus_grandpa::SharedVoterState,
    /// Authority set info.
    pub shared_authority_set: sc_consensus_grandpa::SharedAuthoritySet<Hash, BlockNumber>,
    /// Receives notifications about justification events from Grandpa.
    pub justification_stream: sc_consensus_grandpa::GrandpaJustificationStream<Block>,
    /// Executor to drive the subscription manager in the Grandpa RPC handler.
    pub subscription_executor: sc_rpc::SubscriptionTaskExecutor,
    /// Finality proof provider.
    pub finality_provider: Arc<FinalityProofProvider<B, Block>>,
}

/// Full client dependencies.
pub struct FullDeps<C, P, B, A: ChainApi, CT, CIDP> {
    /// The client instance to use.
    pub client: Arc<C>,
    /// Transaction pool instance.
    pub pool: Arc<P>,
    /// Whether to deny unsafe calls
    pub deny_unsafe: DenyUnsafe,
    /// Grandpa block import setup.
    pub grandpa: GrandpaDeps<B>,
    /// Backend used by the node.
    pub _backend: Arc<B>,
	/// Ethereum-compatibility specific dependencies.
	pub eth: EthDeps<Block, C, P, A, CT, CIDP>,
}

pub struct DefaultEthConfig<C, BE>(std::marker::PhantomData<(C, BE)>);

impl<C, BE> fc_rpc::EthConfig<Block, C> for DefaultEthConfig<C, BE>
where
    C: StorageProvider<Block, BE> + Sync + Send + 'static,
    BE: Backend<Block> + 'static,
{
    type EstimateGasAdapter = ();
    type RuntimeStorageOverride =
        fc_rpc::frontier_backend_client::SystemAccountId20StorageOverride<Block, C, BE>;
}
    
/// Instantiate all full RPC extensions.
pub fn create_full<C, P, B, BE, A, CT, CIDP>(
    deps: FullDeps<C, P, B, A, CT, CIDP>,
    subscription_task_executor: SubscriptionTaskExecutor,
	pubsub_notification_sinks: Arc<
		fc_mapping_sync::EthereumBlockNotificationSinks<
			fc_mapping_sync::EthereumBlockNotification<Block>,
		>,
	>,    
) -> Result<RpcModule<()>, Box<dyn std::error::Error + Send + Sync>>
where
    C: sp_api::CallApiAt<Block> + ProvideRuntimeApi<Block>,
    C: HeaderBackend<Block> + HeaderMetadata<Block, Error = BlockChainError> + 'static,
    C: Send + Sync + 'static,
    C: BlockchainEvents<Block> + AuxStore + UsageProvider<Block> + StorageProvider<Block, BE>,
    C::Api: substrate_frame_rpc_system::AccountNonceApi<Block, AccountId, Index>,
    C::Api: pallet_transaction_payment_rpc::TransactionPaymentRuntimeApi<sp_runtime::generic::Block<sp_runtime::generic::Header<u32, BlakeTwo256>, OpaqueExtrinsic>, u64>,
    C::Api: BlockBuilder<Block>,
    C::Api: subtensor_custom_rpc_runtime_api::DelegateInfoRuntimeApi<Block>,
    C::Api: subtensor_custom_rpc_runtime_api::NeuronInfoRuntimeApi<Block>,
    C::Api: subtensor_custom_rpc_runtime_api::SubnetInfoRuntimeApi<Block>,
    C::Api: subtensor_custom_rpc_runtime_api::SubnetRegistrationRuntimeApi<Block>,
    C::Api: sp_consensus_aura::AuraApi<Block, AuraId>,
    C::Api: fp_rpc::ConvertTransactionRuntimeApi<Block>,
    C::Api: fp_rpc::EthereumRuntimeRPCApi<Block>,
    B: sc_client_api::Backend<Block> + Send + Sync + 'static + sp_runtime::traits::Block,
    P: TransactionPool<Block = Block> + 'static,
    BE: Backend<Block> + 'static,
    A: ChainApi<Block = Block> + 'static,
    CIDP: sp_inherents::CreateInherentDataProviders<Block, ()> + Send + 'static,
	CT: fp_rpc::ConvertTransaction<<Block as BlockT>::Extrinsic> + Send + Sync + 'static,
{
    use pallet_transaction_payment_rpc::{TransactionPayment, TransactionPaymentApiServer};
    use sc_consensus_grandpa_rpc::{Grandpa, GrandpaApiServer};
    use substrate_frame_rpc_system::{System, SystemApiServer};
    use subtensor_custom_rpc::{SubtensorCustom, SubtensorCustomApiServer};

    let mut module = RpcModule::new(());
    let FullDeps {
        client,
        pool,
        deny_unsafe,
        grandpa,
        _backend: _,
        eth
    } = deps;

    // Custom RPC methods for Paratensor
    module.merge(SubtensorCustom::new(client.clone()).into_rpc())?;

    module.merge(System::new(client.clone(), pool.clone(), deny_unsafe).into_rpc())?;
    module.merge(TransactionPayment::new(client).into_rpc())?;

    let GrandpaDeps {
        shared_voter_state,
        shared_authority_set,
        justification_stream,
        subscription_executor,
        finality_provider,
    } = grandpa;

    module.merge(
        Grandpa::new(
            subscription_executor,
            shared_authority_set.clone(),
            shared_voter_state,
            justification_stream,
            finality_provider,
        )
        .into_rpc(),
    )?;

    // Extend this RPC with a custom API by using the following syntax.
    // `YourRpcStruct` should have a reference to a client, which is needed
    // to call into the runtime.
    // `module.merge(YourRpcTrait::into_rpc(YourRpcStruct::new(ReferenceToClient, ...)))?;`

	// Ethereum compatibility RPCs
	let module = create_eth::<_, _, _, _, _, _, _, DefaultEthConfig<C, BE>>(
		module,
		eth,
		subscription_task_executor,
        pubsub_notification_sinks,
	)?;

    Ok(module)
}