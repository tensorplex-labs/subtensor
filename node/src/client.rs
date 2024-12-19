use node_subtensor_runtime::{opaque::Block, RuntimeApi};
use sc_executor::WasmExecutor;

/// Full backend.
pub type FullBackend = sc_service::TFullBackend<Block>;
/// Full client.
pub type FullClient = sc_service::TFullClient<Block, RuntimeApi, RuntimeExecutor>;
/// Always enable runtime benchmark host functions, the genesis state
/// was built with them so we're stuck with them forever.
///
/// They're just a noop, never actually get used if the runtime was not compiled with
/// `runtime-benchmarks`.
pub type HostFunctions = (
    sp_io::SubstrateHostFunctions,
    frame_benchmarking::benchmarking::HostFunctions,
    sp_crypto_ec_utils::bls12_381::host_calls::HostFunctions,
);
pub type RuntimeExecutor = WasmExecutor<HostFunctions>;
