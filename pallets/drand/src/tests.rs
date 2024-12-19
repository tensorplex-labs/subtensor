/*
 * Copyright 2024 by Ideal Labs, LLC
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use crate::{
    mock::*, BeaconConfig, BeaconConfigurationPayload, BeaconInfoResponse, Call, DrandResponseBody,
    Error, Pulse, Pulses, PulsesPayload, ENDPOINTS, QUICKNET_CHAIN_HASH,
};
use codec::Encode;
use frame_support::{
    assert_noop, assert_ok,
    pallet_prelude::{InvalidTransaction, TransactionSource},
};
use frame_system::RawOrigin;
use sp_runtime::{
    offchain::{
        testing::{PendingRequest, TestOffchainExt},
        OffchainWorkerExt,
    },
    traits::ValidateUnsigned,
};

// The round number used to collect drand pulses
pub const ROUND_NUMBER: u64 = 1000;

// Quicknet parameters
pub const DRAND_PULSE: &str = "{\"round\":1000,\"randomness\":\"fe290beca10872ef2fb164d2aa4442de4566183ec51c56ff3cd603d930e54fdd\",\"signature\":\"b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39\"}";
pub const DRAND_INFO_RESPONSE: &str = "{\"public_key\":\"83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a\",\"period\":3,\"genesis_time\":1692803367,\"hash\":\"52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971\",\"groupHash\":\"f477d5c89f21a17c863a7f937c6a6d15859414d2be09cd448d4279af331c5d3e\",\"schemeID\":\"bls-unchained-g1-rfc9380\",\"metadata\":{\"beaconID\":\"quicknet\"}}";
const INVALID_JSON: &str = r#"{"round":1000,"randomness":"not base64??","signature":}"#;

#[test]
fn it_can_submit_valid_pulse_when_beacon_config_exists() {
    new_test_ext().execute_with(|| {
        let u_p: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();
        let p: Pulse = u_p.try_into_pulse().unwrap();

        let alice = sp_keyring::Sr25519Keyring::Alice;
        let block_number = 1;
        System::set_block_number(block_number);

        // Set the beacon config
        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config_payload = BeaconConfigurationPayload {
            block_number,
            config: info.clone().try_into_beacon_config().unwrap(),
            public: alice.public(),
        };

        // The signature doesn't really matter here because the signature is validated in the
        // transaction validation phase not in the dispatchable itself.
        let signature = None;
        assert_ok!(Drand::set_beacon_config(
            RuntimeOrigin::root(),
            config_payload,
            signature
        ));

        let pulses_payload = PulsesPayload {
            pulses: vec![p.clone()],
            block_number,
            public: alice.public(),
        };

        // Dispatch an unsigned extrinsic.
        assert_ok!(Drand::write_pulse(
            RuntimeOrigin::none(),
            pulses_payload,
            signature
        ));

        // Read pallet storage and assert an expected result.
        let pulse = Pulses::<Test>::get(ROUND_NUMBER);
        assert!(pulse.is_some());
        assert_eq!(pulse, Some(p));
    });
}

#[test]
fn it_rejects_invalid_pulse_due_to_bad_signature() {
    new_test_ext().execute_with(|| {
        let alice = sp_keyring::Sr25519Keyring::Alice;
        let block_number = 1;
        System::set_block_number(block_number);

        // Set the beacon config using Root origin
        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config_payload = BeaconConfigurationPayload {
            block_number,
            config: info.try_into_beacon_config().unwrap(),
            public: alice.public(),
        };
        // Signature is not required for Root origin
        let config_signature = None;
        assert_ok!(Drand::set_beacon_config(
            RuntimeOrigin::root(),
            config_payload.clone(),
            config_signature
        ));

        // Get a bad pulse (invalid signature within the pulse data)
        let bad_http_response = "{\"round\":1000,\"randomness\":\"87f03ef5f62885390defedf60d5b8132b4dc2115b1efc6e99d166a37ab2f3a02\",\"signature\":\"b0a8b04e009cf72534321aca0f50048da596a3feec1172a0244d9a4a623a3123d0402da79854d4c705e94bc73224c341\"}";
        let u_p: DrandResponseBody = serde_json::from_str(bad_http_response).unwrap();
        let p: Pulse = u_p.try_into_pulse().unwrap();

        // Prepare the pulses payload
        let pulses_payload = PulsesPayload {
            pulses: vec![p.clone()],
            block_number,
            public: alice.public(),
        };
        let pulses_signature = alice.sign(&pulses_payload.encode());

        assert_noop!(
            Drand::write_pulse(
                RawOrigin::None.into(),
                pulses_payload.clone(),
                Some(pulses_signature)
            ),
            Error::<Test>::PulseVerificationError
        );

        let pulse = Pulses::<Test>::get(ROUND_NUMBER);
        assert!(pulse.is_none());
    });
}

#[test]
fn it_rejects_pulses_with_non_incremental_round_numbers() {
    new_test_ext().execute_with(|| {
        let block_number = 1;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);

        // Set the beacon config
        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config_payload = BeaconConfigurationPayload {
            block_number,
            config: info.clone().try_into_beacon_config().unwrap(),
            public: alice.public(),
        };
        // The signature doesn't really matter here because the signature is validated in the
        // transaction validation phase not in the dispatchable itself.
        let signature = None;
        assert_ok!(Drand::set_beacon_config(
            RuntimeOrigin::root(),
            config_payload,
            signature
        ));

        let u_p: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();
        let p: Pulse = u_p.try_into_pulse().unwrap();
        let pulses_payload = PulsesPayload {
            pulses: vec![p.clone()],
            block_number,
            public: alice.public(),
        };

        // Dispatch an unsigned extrinsic.
        assert_ok!(Drand::write_pulse(
            RuntimeOrigin::none(),
            pulses_payload.clone(),
            signature
        ));
        let pulse = Pulses::<Test>::get(ROUND_NUMBER);
        assert!(pulse.is_some());

        System::set_block_number(2);

        // Attempt to submit the same pulse again, which should fail
        assert_noop!(
            Drand::write_pulse(RuntimeOrigin::none(), pulses_payload, signature),
            Error::<Test>::InvalidRoundNumber,
        );
    });
}

#[test]
fn it_blocks_non_root_from_submit_beacon_info() {
    new_test_ext().execute_with(|| {
        let block_number = 1;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);

        // Prepare the beacon configuration payload
        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config_payload = BeaconConfigurationPayload {
            block_number,
            config: info.try_into_beacon_config().unwrap(),
            public: alice.public(),
        };

        // Signature is not required when using Root origin, but we'll include it for completeness
        let signature = None;

        // Attempt to set the beacon config with a non-root origin (signed by Alice)
        // Expect it to fail with BadOrigin
        assert_noop!(
            Drand::set_beacon_config(
                RuntimeOrigin::signed(alice.public()),
                config_payload.clone(),
                signature
            ),
            sp_runtime::DispatchError::BadOrigin
        );

        // Attempt to set the beacon config with an unsigned origin
        // Expect it to fail with BadOrigin
        assert_noop!(
            Drand::set_beacon_config(RuntimeOrigin::none(), config_payload.clone(), signature),
            sp_runtime::DispatchError::BadOrigin
        );

        // Now attempt to set the beacon config with Root origin
        // Expect it to succeed
        assert_ok!(Drand::set_beacon_config(
            RuntimeOrigin::root(),
            config_payload,
            signature
        ));

        // Verify that the BeaconConfig storage item has been updated
        let stored_config = BeaconConfig::<Test>::get();
        assert_eq!(stored_config, info.try_into_beacon_config().unwrap());
    });
}

#[test]
fn signed_cannot_submit_beacon_info() {
    new_test_ext().execute_with(|| {
        let block_number = 1;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);

        // Set the beacon config
        let info: BeaconInfoResponse = serde_json::from_str(DRAND_INFO_RESPONSE).unwrap();
        let config_payload = BeaconConfigurationPayload {
            block_number,
            config: info.clone().try_into_beacon_config().unwrap(),
            public: alice.public(),
        };
        // The signature doesn't really matter here because the signature is validated in the
        // transaction validation phase not in the dispatchable itself.
        let signature = None;
        // Dispatch a signed extrinsic
        assert_noop!(
            Drand::set_beacon_config(
                RuntimeOrigin::signed(alice.public()),
                config_payload,
                signature
            ),
            sp_runtime::DispatchError::BadOrigin
        );
    });
}

#[test]
fn test_validate_unsigned_write_pulse() {
    new_test_ext().execute_with(|| {
        let block_number = 1;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);
        let pulses_payload = PulsesPayload {
            block_number,
            pulses: vec![],
            public: alice.public(),
        };
        let signature = alice.sign(&pulses_payload.encode());

        let call = Call::write_pulse {
            pulses_payload: pulses_payload.clone(),
            signature: Some(signature),
        };

        let source = TransactionSource::External;
        let validity = Drand::validate_unsigned(source, &call);

        assert_ok!(validity);
    });
}

#[test]
fn test_not_validate_unsigned_write_pulse_with_bad_proof() {
    new_test_ext().execute_with(|| {
        let block_number = 1;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);
        let pulses_payload = PulsesPayload {
            block_number,
            pulses: vec![],
            public: alice.public(),
        };

        // Bad signature
        let signature = <Test as frame_system::offchain::SigningTypes>::Signature::default();
        let call = Call::write_pulse {
            pulses_payload: pulses_payload.clone(),
            signature: Some(signature),
        };

        let source = TransactionSource::External;
        let validity = Drand::validate_unsigned(source, &call);

        assert_noop!(validity, InvalidTransaction::BadProof);
    });
}

#[test]
fn test_not_validate_unsigned_write_pulse_with_no_payload_signature() {
    new_test_ext().execute_with(|| {
        let block_number = 1;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);
        let pulses_payload = PulsesPayload {
            block_number,
            pulses: vec![],
            public: alice.public(),
        };

        // No signature
        let signature = None;
        let call = Call::write_pulse {
            pulses_payload: pulses_payload.clone(),
            signature,
        };

        let source = TransactionSource::External;
        let validity = Drand::validate_unsigned(source, &call);

        assert_noop!(validity, InvalidTransaction::BadSigner);
    });
}

#[test]
fn can_execute_and_handle_valid_http_responses() {
    use serde_json;

    let expected_pulse: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();

    let (offchain, state) = TestOffchainExt::new();
    let mut t = sp_io::TestExternalities::default();
    t.register_extension(OffchainWorkerExt::new(offchain));

    {
        let mut state = state.write();

        for endpoint in ENDPOINTS.iter() {
            state.expect_request(PendingRequest {
                method: "GET".into(),
                uri: format!("{}/{}/public/1000", endpoint, QUICKNET_CHAIN_HASH),
                response: Some(DRAND_PULSE.as_bytes().to_vec()),
                sent: true,
                ..Default::default()
            });
        }

        for endpoint in ENDPOINTS.iter() {
            state.expect_request(PendingRequest {
                method: "GET".into(),
                uri: format!("{}/{}/public/latest", endpoint, QUICKNET_CHAIN_HASH),
                response: Some(DRAND_PULSE.as_bytes().to_vec()),
                sent: true,
                ..Default::default()
            });
        }
    }

    t.execute_with(|| {
        let actual_specific = Drand::fetch_drand_by_round(1000u64).unwrap();
        assert_eq!(actual_specific, expected_pulse);

        let actual_pulse = Drand::fetch_drand_latest().unwrap();
        assert_eq!(actual_pulse, expected_pulse);
    });
}

#[test]
fn validate_unsigned_rejects_future_block_number() {
    new_test_ext().execute_with(|| {
        let block_number = 1;
        let future_block_number = 100;
        let alice = sp_keyring::Sr25519Keyring::Alice;
        System::set_block_number(block_number);
        let pulses_payload = PulsesPayload {
            block_number: future_block_number,
            pulses: vec![],
            public: alice.public(),
        };
        let signature = alice.sign(&pulses_payload.encode());

        let call = Call::write_pulse {
            pulses_payload: pulses_payload.clone(),
            signature: Some(signature),
        };

        let source = TransactionSource::External;
        let validity = Drand::validate_unsigned(source, &call);

        assert_noop!(validity, InvalidTransaction::Future);
    });
}

#[test]
fn test_all_endpoints_fail() {
    let (offchain, state) = TestOffchainExt::new();
    let mut t = sp_io::TestExternalities::default();
    t.register_extension(OffchainWorkerExt::new(offchain));

    {
        let mut state = state.write();
        let endpoints = ENDPOINTS;

        for endpoint in endpoints.iter() {
            state.expect_request(PendingRequest {
                method: "GET".into(),
                uri: format!("{}/{}/public/1000", endpoint, QUICKNET_CHAIN_HASH),
                response: Some(INVALID_JSON.as_bytes().to_vec()),
                sent: true,
                ..Default::default()
            });
        }
    }

    t.execute_with(|| {
        let result = Drand::fetch_drand_by_round(1000u64);
        assert!(
            result.is_err(),
            "All endpoints should fail due to invalid JSON responses"
        );
    });
}

#[test]
fn test_eventual_success() {
    let expected_pulse: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();

    let (offchain, state) = TestOffchainExt::new();
    let mut t = sp_io::TestExternalities::default();
    t.register_extension(OffchainWorkerExt::new(offchain));

    {
        let mut state = state.write();
        let endpoints = ENDPOINTS;

        // We'll make all endpoints except the last return invalid JSON.
        // Since no meta is provided, these are "200 OK" but invalid JSON, causing decode failures.
        // The last endpoint returns the valid DRAND_PULSE JSON, leading to success.

        // Endpoint 0: Invalid JSON (decode fail)
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[0], QUICKNET_CHAIN_HASH),
            response: Some(INVALID_JSON.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });

        // Endpoint 1: Invalid JSON
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[1], QUICKNET_CHAIN_HASH),
            response: Some(Vec::new()),
            sent: true,
            ..Default::default()
        });

        // Endpoint 2: Invalid JSON
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[2], QUICKNET_CHAIN_HASH),
            response: Some(INVALID_JSON.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });

        // Endpoint 3: Invalid JSON
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[3], QUICKNET_CHAIN_HASH),
            response: Some(INVALID_JSON.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });

        // Endpoint 4: Valid JSON (success)
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[4], QUICKNET_CHAIN_HASH),
            response: Some(DRAND_PULSE.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });
    }

    t.execute_with(|| {
        let actual = Drand::fetch_drand_by_round(1000u64).unwrap();
        assert_eq!(
            actual, expected_pulse,
            "Should succeed on the last endpoint after failing at the previous ones"
        );
    });
}

#[test]
fn test_invalid_json_then_success() {
    let expected_pulse: DrandResponseBody = serde_json::from_str(DRAND_PULSE).unwrap();

    let (offchain, state) = TestOffchainExt::new();
    let mut t = sp_io::TestExternalities::default();
    t.register_extension(OffchainWorkerExt::new(offchain));

    {
        let mut state = state.write();

        let endpoints = ENDPOINTS;

        // Endpoint 1: Invalid JSON
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[0], QUICKNET_CHAIN_HASH),
            response: Some(INVALID_JSON.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });

        // Endpoint 2: Valid response
        state.expect_request(PendingRequest {
            method: "GET".into(),
            uri: format!("{}/{}/public/1000", endpoints[1], QUICKNET_CHAIN_HASH),
            response: Some(DRAND_PULSE.as_bytes().to_vec()),
            sent: true,
            ..Default::default()
        });
    }

    t.execute_with(|| {
        let actual = Drand::fetch_drand_by_round(1000u64).unwrap();
        assert_eq!(actual, expected_pulse);
    });
}
