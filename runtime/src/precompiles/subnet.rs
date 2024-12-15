use crate::precompiles::{dispatch, get_method_id, get_slice};
use crate::{ Runtime, RuntimeCall};
use pallet_evm::{AddressMapping, ExitError, ExitSucceed, PrecompileFailure, PrecompileOutput, PrecompileHandle, PrecompileResult, HashedAddressMapping };
use sp_std::vec;
use sp_runtime::AccountId32;
use sp_core::{ U256};

use sp_runtime::traits::BlakeTwo256;

pub const SUBNET_PRECOMPILE_INDEX: u64 = 2051;
// three bytes with max lenght 1K
pub const MAX_PARAMETER_SIZE: usize = 3 * 1024;

// this is staking smart contract's(0x0000000000000000000000000000000000000803) sr25519 address
pub const STAKING_CONTRACT_ADDRESS: &str = "5DPSUCb5mZFfizvBDSnRoAqmxV5Bmov2CS3xV773qU6VP1w2";

pub struct SubnetPrecompile;

impl SubnetPrecompile {
    pub fn execute(handle: &mut impl PrecompileHandle) -> PrecompileResult {
        let txdata = handle.input();
        if txdata.len() > MAX_PARAMETER_SIZE {
            log::error!("the length of subnet call is {} ", txdata.len());
            return Err(PrecompileFailure::Error {
                exit_status: ExitError::InvalidRange,
            });
        }
        let method_id = get_slice(txdata, 0, 4)?;
        let method_input = txdata
            .get(4..)
            .map_or_else(vec::Vec::new, |slice| slice.to_vec()); // Avoiding borrowing conflicts

        match method_id {
            id if id == get_method_id("registerNetwork(bytes,bytes,bytes)") => {
                Self::register_network(handle, &method_input)
            }
			id if id == get_method_id("burnedRegister(uint16)") => {
                Self::burned_register(handle, &method_input)
            }
            id if id == get_method_id("registerNetwork()") => {
                Self::register_network(handle, &[0_u8; 0])
            }
            id if id == get_method_id("getTempo(uint16)") => {
                Self::get_tempo(&method_input)
            }
            _ => Err(PrecompileFailure::Error {
                exit_status: ExitError::InvalidRange,
            }),
        }
    }

	fn burned_register(handle: &mut impl PrecompileHandle, data: &[u8]) -> PrecompileResult {
        let netuid = Self::parse_netuid(data)?;
		let account_id =
        <HashedAddressMapping<BlakeTwo256> as AddressMapping<AccountId32>>::into_account_id(
            handle.context().caller,
        );

		// Based on the netuid, we try to burned register the subnet
		let call = RuntimeCall::SubtensorModule(
			pallet_subtensor::Call::<Runtime>::burned_register { netuid: netuid, hotkey: account_id }
		);

        // Dispatch the register_network call
        dispatch(handle, call, STAKING_CONTRACT_ADDRESS)
	}

	fn get_tempo(data: &[u8]) -> PrecompileResult {
		let netuid = Self::parse_netuid(data)?;
		let tempo = pallet_subtensor::Pallet::<Runtime>::get_tempo(netuid);
        let result_u256 = U256::from(tempo);
        let mut result = [0_u8; 32];
        U256::to_big_endian(&result_u256, &mut result);
        Ok(PrecompileOutput {
            exit_status: ExitSucceed::Returned,
            output: result.into(),
        })
	}

    fn parse_netuid(data: &[u8]) -> Result<u16, PrecompileFailure> {
        if data.len() < 32 {
            return Err(PrecompileFailure::Error {
                exit_status: ExitError::InvalidRange,
            });
        }
        let mut netuid = [0u8; 2];
        netuid.copy_from_slice(get_slice(data, 30, 32)?);
        let result = u16::from_be_bytes(netuid);
        Ok(result)
    }


    fn register_network(handle: &mut impl PrecompileHandle, data: &[u8]) -> PrecompileResult {
        let call = if data.is_empty() {
            RuntimeCall::SubtensorModule(
                pallet_subtensor::Call::<Runtime>::register_network_with_identity {
                    identity: None,
                },
            )
        } else {
            let (subnet_name, github_repo, subnet_contact) =
                Self::parse_register_network_parameters(data)?;

            let identity: pallet_subtensor::SubnetIdentityOf = pallet_subtensor::SubnetIdentityOf {
                subnet_name,
                github_repo,
                subnet_contact,
            };

            // Create the register_network callcle
            RuntimeCall::SubtensorModule(
                pallet_subtensor::Call::<Runtime>::register_network_with_identity {
                    identity: Some(identity),
                },
            )
        };

        // Dispatch the register_network call
        dispatch(handle, call, STAKING_CONTRACT_ADDRESS)
    }

    fn parse_register_network_parameters(
        data: &[u8],
    ) -> Result<(vec::Vec<u8>, vec::Vec<u8>, vec::Vec<u8>), PrecompileFailure> {
        let mut buf = [0_u8; 4];

        // get all start point for three data items: name, repo and contact
        buf.copy_from_slice(get_slice(data, 28, 32)?);
        let subnet_name_start: usize = u32::from_be_bytes(buf) as usize;

        buf.copy_from_slice(get_slice(data, 60, 64)?);
        let github_repo_start: usize = u32::from_be_bytes(buf) as usize;

        buf.copy_from_slice(get_slice(data, 92, 96)?);
        let subnet_contact_start: usize = u32::from_be_bytes(buf) as usize;

        // get name
        buf.copy_from_slice(get_slice(
            data,
            subnet_name_start + 28,
            subnet_name_start + 32,
        )?);
        let subnet_name_len: usize = u32::from_be_bytes(buf) as usize;

        let mut name_vec = vec![0; subnet_name_len];
        name_vec.copy_from_slice(get_slice(
            data,
            subnet_name_start + 32,
            subnet_name_start + subnet_name_len + 32,
        )?);

        // get repo data
        buf.copy_from_slice(get_slice(
            data,
            github_repo_start + 28,
            github_repo_start + 32,
        )?);
        let github_repo_len: usize = u32::from_be_bytes(buf) as usize;

        let mut repo_vec = vec![0; github_repo_len];
        repo_vec.copy_from_slice(get_slice(
            data,
            github_repo_start + 32,
            github_repo_start + github_repo_len + 32,
        )?);

        // get contact data
        buf.copy_from_slice(get_slice(
            data,
            subnet_contact_start + 28,
            subnet_contact_start + 32,
        )?);
        let subnet_contact_len: usize = u32::from_be_bytes(buf) as usize;

        let mut contact_vec = vec![0; subnet_contact_len];
        contact_vec.copy_from_slice(get_slice(
            data,
            subnet_contact_start + 32,
            subnet_contact_start + subnet_contact_len + 32,
        )?);

        Ok((name_vec, repo_vec, contact_vec))
    }
}
