pub use fc_consensus::FrontierBlockImport;
use fc_rpc::{
    pending::AuraConsensusDataProvider, Debug, DebugApiServer, Eth, EthApiServer, EthConfig,
    EthDevSigner, EthFilter, EthFilterApiServer, EthPubSub, EthPubSubApiServer, EthSigner, EthTask,
    Net, NetApiServer, Web3, Web3ApiServer,
};
pub use fc_rpc_core::types::{FeeHistoryCache, FeeHistoryCacheLimit, FilterPool};
/// Frontier DB backend type.
pub use fc_storage::{StorageOverride, StorageOverrideHandler};
use fp_rpc::ConvertTransaction;
use futures::future;
use futures::StreamExt;
use jsonrpsee::RpcModule;
use node_subtensor_runtime::opaque::Block;
use sc_client_api::client::BlockchainEvents;
use sc_network_sync::SyncingService;
use sc_rpc::SubscriptionTaskExecutor;
use sc_service::{error::Error as ServiceError, Configuration, TaskManager};
use sc_transaction_pool::ChainApi;
use sc_transaction_pool_api::TransactionPool;
use sp_inherents::CreateInherentDataProviders;
use sp_runtime::traits::Block as BlockT;
use std::path::PathBuf;
use std::time::Duration;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use crate::client::{FullBackend, FullClient};
use crate::rpc::EthDeps;

pub type FrontierBackend = fc_db::Backend<Block, FullClient>;

/// Avalailable frontier backend types.
#[derive(Debug, Copy, Clone, Default, clap::ValueEnum)]
pub enum BackendType {
    /// Either RocksDb or ParityDb as per inherited from the global backend settings.
    #[default]
    KeyValue,
    /// Sql database with custom log indexing.
    Sql,
}

/// The ethereum-compatibility configuration used to run a node.
#[derive(Clone, Debug, clap::Parser)]
pub struct EthConfiguration {
    /// Maximum number of logs in a query.
    #[arg(long, default_value = "10000")]
    pub max_past_logs: u32,

    /// Maximum fee history cache size.
    #[arg(long, default_value = "2048")]
    pub fee_history_limit: u64,

    #[arg(long)]
    pub enable_dev_signer: bool,

    /// The dynamic-fee pallet target gas price set by block author
    #[arg(long, default_value = "1")]
    pub target_gas_price: u64,

    /// Maximum allowed gas limit will be `block.gas_limit * execute_gas_limit_multiplier`
    /// when using eth_call/eth_estimateGas.
    #[arg(long, default_value = "10")]
    pub execute_gas_limit_multiplier: u64,

    /// Size in bytes of the LRU cache for block data.
    #[arg(long, default_value = "50")]
    pub eth_log_block_cache: usize,

    /// Size in bytes of the LRU cache for transactions statuses data.
    #[arg(long, default_value = "50")]
    pub eth_statuses_cache: usize,

    /// Sets the frontier backend type (KeyValue or Sql)
    #[arg(long, value_enum, ignore_case = true, default_value_t = BackendType::default())]
    pub frontier_backend_type: BackendType,

    // Sets the SQL backend's pool size.
    #[arg(long, default_value = "100")]
    pub frontier_sql_backend_pool_size: u32,

    /// Sets the SQL backend's query timeout in number of VM ops.
    #[arg(long, default_value = "10000000")]
    pub frontier_sql_backend_num_ops_timeout: u32,

    /// Sets the SQL backend's auxiliary thread limit.
    #[arg(long, default_value = "4")]
    pub frontier_sql_backend_thread_count: u32,

    /// Sets the SQL backend's query timeout in number of VM ops.
    /// Default value is 200MB.
    #[arg(long, default_value = "209715200")]
    pub frontier_sql_backend_cache_size: u64,
}

pub fn db_config_dir(config: &Configuration) -> PathBuf {
    config.base_path.config_dir(config.chain_spec.id())
}

pub struct FrontierPartialComponents {
    pub filter_pool: Option<FilterPool>,
    pub fee_history_cache: FeeHistoryCache,
    pub fee_history_cache_limit: FeeHistoryCacheLimit,
}

pub fn new_frontier_partial(
    config: &EthConfiguration,
) -> Result<FrontierPartialComponents, ServiceError> {
    Ok(FrontierPartialComponents {
        filter_pool: Some(Arc::new(Mutex::new(BTreeMap::new()))),
        fee_history_cache: Arc::new(Mutex::new(BTreeMap::new())),
        fee_history_cache_limit: config.fee_history_limit,
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn spawn_frontier_tasks(
    task_manager: &TaskManager,
    client: Arc<FullClient>,
    backend: Arc<FullBackend>,
    frontier_backend: Arc<FrontierBackend>,
    filter_pool: Option<FilterPool>,
    storage_override: Arc<dyn StorageOverride<Block>>,
    fee_history_cache: FeeHistoryCache,
    fee_history_cache_limit: FeeHistoryCacheLimit,
    sync: Arc<SyncingService<Block>>,
    pubsub_notification_sinks: Arc<
        fc_mapping_sync::EthereumBlockNotificationSinks<
            fc_mapping_sync::EthereumBlockNotification<Block>,
        >,
    >,
) {
    // Spawn main mapping sync worker background task.
    match &*frontier_backend {
        fc_db::Backend::KeyValue(b) => {
            task_manager.spawn_essential_handle().spawn(
                "frontier-mapping-sync-worker",
                Some("frontier"),
                fc_mapping_sync::kv::MappingSyncWorker::new(
                    client.import_notification_stream(),
                    Duration::new(6, 0),
                    client.clone(),
                    backend,
                    storage_override.clone(),
                    b.clone(),
                    3,
                    0u32,
                    fc_mapping_sync::SyncStrategy::Normal,
                    sync,
                    pubsub_notification_sinks,
                )
                .for_each(|()| future::ready(())),
            );
        }
        fc_db::Backend::Sql(b) => {
            task_manager.spawn_essential_handle().spawn_blocking(
                "frontier-mapping-sync-worker",
                Some("frontier"),
                fc_mapping_sync::sql::SyncWorker::run(
                    client.clone(),
                    backend,
                    b.clone(),
                    client.import_notification_stream(),
                    fc_mapping_sync::sql::SyncWorkerConfig {
                        read_notification_timeout: Duration::from_secs(30),
                        check_indexed_blocks_interval: Duration::from_secs(60),
                    },
                    fc_mapping_sync::SyncStrategy::Parachain,
                    sync,
                    pubsub_notification_sinks,
                ),
            );
        }
    }

    // Spawn Frontier EthFilterApi maintenance task.
    if let Some(filter_pool) = filter_pool {
        // Each filter is allowed to stay in the pool for 100 blocks.
        const FILTER_RETAIN_THRESHOLD: u64 = 100;
        task_manager.spawn_essential_handle().spawn(
            "frontier-filter-pool",
            Some("frontier"),
            EthTask::filter_pool_task(client.clone(), filter_pool, FILTER_RETAIN_THRESHOLD),
        );
    }

    // Spawn Frontier FeeHistory cache maintenance task.
    task_manager.spawn_essential_handle().spawn(
        "frontier-fee-history",
        Some("frontier"),
        EthTask::fee_history_task(
            client,
            storage_override,
            fee_history_cache,
            fee_history_cache_limit,
        ),
    );
}

fn extend_rpc_aet_api<P, A, CT, CIDP, EC>(
    io: &mut RpcModule<()>,
    deps: &EthDeps<P, A, CT, CIDP>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    P: TransactionPool<Block = Block> + 'static,
    A: ChainApi<Block = Block> + 'static,
    CT: ConvertTransaction<<Block as BlockT>::Extrinsic> + Send + Sync + Clone + 'static,
    CIDP: CreateInherentDataProviders<Block, ()> + Send + Clone + 'static,
    EC: EthConfig<Block, FullClient>,
{
    let mut signers = Vec::new();
    if deps.enable_dev_signer {
        signers.push(Box::new(EthDevSigner::new()) as Box<dyn EthSigner>);
    }

    io.merge(
        Eth::<Block, FullClient, P, CT, FullBackend, A, CIDP, EC>::new(
            deps.client.clone(),
            deps.pool.clone(),
            deps.graph.clone(),
            deps.converter.clone(),
            deps.sync.clone(),
            signers,
            deps.storage_override.clone(),
            deps.frontier_backend.clone(),
            deps.is_authority,
            deps.block_data_cache.clone(),
            deps.fee_history_cache.clone(),
            deps.fee_history_cache_limit,
            deps.execute_gas_limit_multiplier,
            deps.forced_parent_hashes.clone(),
            deps.pending_create_inherent_data_providers.clone(),
            Some(Box::new(AuraConsensusDataProvider::new(
                deps.client.clone(),
            ))),
        )
        .replace_config::<EC>()
        .into_rpc(),
    )?;
    Ok(())
}

fn extend_rpc_eth_filter<P, A, CT, CIDP>(
    io: &mut RpcModule<()>,
    deps: &EthDeps<P, A, CT, CIDP>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    P: TransactionPool<Block = Block> + 'static,
    A: ChainApi<Block = Block> + 'static,
    CT: ConvertTransaction<<Block as BlockT>::Extrinsic> + Send + Sync + Clone + 'static,
    CIDP: CreateInherentDataProviders<Block, ()> + Send + Clone + 'static,
{
    if let Some(filter_pool) = deps.filter_pool.clone() {
        io.merge(
            EthFilter::new(
                deps.client.clone(),
                deps.frontier_backend.clone(),
                deps.graph.clone(),
                filter_pool,
                500_usize, // max stored filters
                deps.max_past_logs,
                deps.block_data_cache.clone(),
            )
            .into_rpc(),
        )?;
    }
    Ok(())
}

// Function for EthPubSub merge
fn extend_rpc_eth_pubsub<P, A, CT, CIDP>(
    io: &mut RpcModule<()>,
    deps: &EthDeps<P, A, CT, CIDP>,
    subscription_task_executor: SubscriptionTaskExecutor,
    pubsub_notification_sinks: Arc<
        fc_mapping_sync::EthereumBlockNotificationSinks<
            fc_mapping_sync::EthereumBlockNotification<Block>,
        >,
    >,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    P: TransactionPool<Block = Block> + 'static,
    A: ChainApi<Block = Block> + 'static,
    CT: ConvertTransaction<<Block as BlockT>::Extrinsic> + Send + Sync + 'static,
    CIDP: CreateInherentDataProviders<Block, ()> + Send + 'static,
{
    io.merge(
        EthPubSub::new(
            deps.pool.clone(),
            deps.client.clone(),
            deps.sync.clone(),
            subscription_task_executor,
            deps.storage_override.clone(),
            pubsub_notification_sinks,
        )
        .into_rpc(),
    )?;
    Ok(())
}

fn extend_rpc_net<P, A, CT, CIDP>(
    io: &mut RpcModule<()>,
    deps: &EthDeps<P, A, CT, CIDP>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    P: TransactionPool<Block = Block> + 'static,
    A: ChainApi<Block = Block> + 'static,
    CT: ConvertTransaction<<Block as BlockT>::Extrinsic> + Send + Sync + 'static,
    CIDP: CreateInherentDataProviders<Block, ()> + Send + 'static,
{
    io.merge(
        Net::new(
            deps.client.clone(),
            deps.network.clone(),
            true, // Whether to format the `peer_count` response as Hex (default) or not.
        )
        .into_rpc(),
    )?;
    Ok(())
}

fn extend_rpc_web3<P, A, CT, CIDP>(
    io: &mut RpcModule<()>,
    deps: &EthDeps<P, A, CT, CIDP>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    P: TransactionPool<Block = Block> + 'static,
    A: ChainApi<Block = Block> + 'static,
    CT: ConvertTransaction<<Block as BlockT>::Extrinsic> + Send + Sync + 'static,
    CIDP: CreateInherentDataProviders<Block, ()> + Send + 'static,
{
    io.merge(Web3::new(deps.client.clone()).into_rpc())?;
    Ok(())
}

fn extend_rpc_debug<P, A, CT, CIDP>(
    io: &mut RpcModule<()>,
    deps: &EthDeps<P, A, CT, CIDP>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    P: TransactionPool<Block = Block> + 'static,
    A: ChainApi<Block = Block> + 'static,
    CT: ConvertTransaction<<Block as BlockT>::Extrinsic> + Send + Sync + 'static,
    CIDP: CreateInherentDataProviders<Block, ()> + Send + 'static,
{
    io.merge(
        Debug::new(
            deps.client.clone(),
            deps.frontier_backend.clone(),
            deps.storage_override.clone(),
            deps.block_data_cache.clone(),
        )
        .into_rpc(),
    )?;
    Ok(())
}

/// Extend RpcModule with Eth RPCs
pub fn create_eth<P, A, CT, CIDP, EC>(
    mut io: RpcModule<()>,
    deps: EthDeps<P, A, CT, CIDP>,
    subscription_task_executor: SubscriptionTaskExecutor,
    pubsub_notification_sinks: Arc<
        fc_mapping_sync::EthereumBlockNotificationSinks<
            fc_mapping_sync::EthereumBlockNotification<Block>,
        >,
    >,
) -> Result<RpcModule<()>, Box<dyn std::error::Error + Send + Sync>>
where
    P: TransactionPool<Block = Block> + 'static,
    A: ChainApi<Block = Block> + 'static,
    CT: ConvertTransaction<<Block as BlockT>::Extrinsic> + Send + Sync + Clone + 'static,
    CIDP: CreateInherentDataProviders<Block, ()> + Send + Clone + 'static,
    EC: EthConfig<Block, FullClient>,
{
    extend_rpc_aet_api::<P, A, CT, CIDP, EC>(&mut io, &deps)?;
    extend_rpc_eth_filter::<P, A, CT, CIDP>(&mut io, &deps)?;
    extend_rpc_eth_pubsub::<P, A, CT, CIDP>(
        &mut io,
        &deps,
        subscription_task_executor,
        pubsub_notification_sinks,
    )?;
    extend_rpc_net::<P, A, CT, CIDP>(&mut io, &deps)?;
    extend_rpc_web3::<P, A, CT, CIDP>(&mut io, &deps)?;
    extend_rpc_debug::<P, A, CT, CIDP>(&mut io, &deps)?;

    Ok(io)
}
