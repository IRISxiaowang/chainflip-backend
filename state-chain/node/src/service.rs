// Copyright 2025 Chainflip Labs GmbH
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.
use cf_rpc_apis::{broker::BrokerRpcApiServer, lp::LpRpcApiServer};
use custom_rpc::{
	broker::{broker_crypto, BrokerSignedRpc},
	lp::{lp_crypto, LpSignedRpc},
	monitoring::MonitoringApiServer,
	CustomApiServer, CustomRpc,
};
use futures::FutureExt;
use jsonrpsee::RpcModule;
use sc_client_api::{Backend, BlockBackend};
use sc_consensus_aura::{ImportQueueParams, SlotProportion, StartAuraParams};
use sc_consensus_grandpa::SharedVoterState;
use sc_keystore::Keystore;
use sc_rpc_spec_v2::{chain_spec as chain_spec_rpc, chain_spec::ChainSpecApiServer};
use sc_service::{error::Error as ServiceError, Configuration, TaskManager, WarpSyncParams};
use sc_telemetry::{Telemetry, TelemetryWorker};
use sc_transaction_pool_api::OffchainTransactionPoolFactory;
use sp_consensus_aura::sr25519::AuthorityPair as AuraPair;
use state_chain_runtime::{self, opaque::Block, RuntimeApi};
use std::{sync::Arc, time::Duration};

pub(crate) type FullClient = sc_service::TFullClient<
	Block,
	RuntimeApi,
	sc_executor::WasmExecutor<sp_io::SubstrateHostFunctions>,
>;

type FullBackend = sc_service::TFullBackend<Block>;
type FullSelectChain = sc_consensus::LongestChain<FullBackend, Block>;

/// The minimum period of blocks on which justifications will be
/// imported and generated.
const GRANDPA_JUSTIFICATION_PERIOD: u32 = 512;

pub type Service = sc_service::PartialComponents<
	FullClient,
	FullBackend,
	FullSelectChain,
	sc_consensus::DefaultImportQueue<Block>,
	sc_transaction_pool::FullPool<Block, FullClient>,
	(
		sc_consensus_grandpa::GrandpaBlockImport<FullBackend, Block, FullClient, FullSelectChain>,
		sc_consensus_grandpa::LinkHalf<Block, FullClient, FullSelectChain>,
		Option<Telemetry>,
	),
>;

pub fn new_partial(config: &Configuration) -> Result<Service, ServiceError> {
	let telemetry = config
		.telemetry_endpoints
		.clone()
		.filter(|x| !x.is_empty())
		.map(|endpoints| -> Result<_, sc_telemetry::Error> {
			let worker = TelemetryWorker::new(16)?;
			let telemetry = worker.handle().new_telemetry(endpoints);
			Ok((worker, telemetry))
		})
		.transpose()?;

	let executor = sc_service::new_wasm_executor::<sp_io::SubstrateHostFunctions>(config);

	let (client, backend, keystore_container, task_manager) =
		sc_service::new_full_parts::<Block, RuntimeApi, _>(
			config,
			telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
			executor,
		)?;
	let client = Arc::new(client);

	let telemetry = telemetry.map(|(worker, telemetry)| {
		task_manager.spawn_handle().spawn("telemetry", None, worker.run());
		telemetry
	});

	let select_chain = sc_consensus::LongestChain::new(backend.clone());

	let transaction_pool = sc_transaction_pool::BasicPool::new_full(
		config.transaction_pool.clone(),
		config.role.is_authority().into(),
		config.prometheus_registry(),
		task_manager.spawn_essential_handle(),
		client.clone(),
	);

	let (grandpa_block_import, grandpa_link) = sc_consensus_grandpa::block_import(
		client.clone(),
		GRANDPA_JUSTIFICATION_PERIOD,
		&client,
		select_chain.clone(),
		telemetry.as_ref().map(|x| x.handle()),
	)?;

	let cidp_client = client.clone();

	let import_queue =
		sc_consensus_aura::import_queue::<AuraPair, _, _, _, _, _>(ImportQueueParams {
			block_import: grandpa_block_import.clone(),
			justification_import: Some(Box::new(grandpa_block_import.clone())),
			client: client.clone(),
			create_inherent_data_providers: move |parent_hash, _| {
				let cidp_client = cidp_client.clone();
				async move {
					let slot_duration = sc_consensus_aura::standalone::slot_duration_at(
						&*cidp_client,
						parent_hash,
					)?;
					let timestamp = sp_timestamp::InherentDataProvider::from_system_time();

					let slot =
						sp_consensus_aura::inherents::InherentDataProvider::from_timestamp_and_slot_duration(
							*timestamp,
							slot_duration,
						);

					Ok((slot, timestamp))
				}
			},
			spawner: &task_manager.spawn_essential_handle(),
			registry: config.prometheus_registry(),
			check_for_equivocation: Default::default(),
			telemetry: telemetry.as_ref().map(|x| x.handle()),
			compatibility_mode: Default::default(),
		})?;

	Ok(sc_service::PartialComponents {
		client,
		backend,
		task_manager,
		import_queue,
		keystore_container,
		select_chain,
		transaction_pool,
		other: (grandpa_block_import, grandpa_link, telemetry),
	})
}

/// Builds a new service for a full client.
pub fn new_full<
	N: sc_network::NetworkBackend<Block, <Block as sp_runtime::traits::Block>::Hash>,
>(
	config: Configuration,
) -> Result<TaskManager, ServiceError> {
	use sc_consensus_grandpa_rpc::{Grandpa, GrandpaApiServer};

	let sc_service::PartialComponents {
		client,
		backend,
		mut task_manager,
		import_queue,
		keystore_container,
		select_chain,
		transaction_pool,
		other: (block_import, grandpa_link, mut telemetry),
	} = new_partial(&config)?;

	let mut net_config = sc_network::config::FullNetworkConfiguration::<
		Block,
		<Block as sp_runtime::traits::Block>::Hash,
		N,
	>::new(&config.network);
	let metrics = N::register_notification_metrics(config.prometheus_registry());

	let peer_store_handle = net_config.peer_store_handle();
	let grandpa_protocol_name = sc_consensus_grandpa::protocol_standard_name(
		&client.block_hash(0).ok().flatten().expect("Genesis block exists; qed"),
		&config.chain_spec,
	);
	let (grandpa_protocol_config, grandpa_notification_service) =
		sc_consensus_grandpa::grandpa_peers_set_config::<_, N>(
			grandpa_protocol_name.clone(),
			metrics.clone(),
			peer_store_handle,
		);
	net_config.add_notification_protocol(grandpa_protocol_config);

	let warp_sync = Arc::new(sc_consensus_grandpa::warp_proof::NetworkProvider::new(
		backend.clone(),
		grandpa_link.shared_authority_set().clone(),
		Vec::default(),
	));

	let (network, system_rpc_tx, tx_handler_controller, network_starter, sync_service) =
		sc_service::build_network(sc_service::BuildNetworkParams {
			config: &config,
			net_config,
			client: client.clone(),
			transaction_pool: transaction_pool.clone(),
			spawn_handle: task_manager.spawn_handle(),
			import_queue,
			block_announce_validator_builder: None,
			warp_sync_params: Some(WarpSyncParams::WithProvider(warp_sync)),
			block_relay: None,
			metrics,
		})?;

	if config.offchain_worker.enabled {
		task_manager.spawn_handle().spawn(
			"offchain-workers-runner",
			"offchain-worker",
			sc_offchain::OffchainWorkers::new(sc_offchain::OffchainWorkerOptions {
				runtime_api_provider: client.clone(),
				is_validator: config.role.is_authority(),
				keystore: Some(keystore_container.keystore()),
				offchain_db: backend.offchain_storage(),
				transaction_pool: Some(OffchainTransactionPoolFactory::new(
					transaction_pool.clone(),
				)),
				network_provider: Arc::new(network.clone()),
				enable_http_requests: true,
				custom_extensions: |_| vec![],
			})
			.run(client.clone(), task_manager.spawn_handle())
			.boxed(),
		);
	}

	// [CF] Required for Grandpa RPC.
	let (shared_voter_state, shared_authority_set, justification_stream, finality_provider) = {
		let (_grandpa_block_import, grandpa_link) = sc_consensus_grandpa::block_import(
			client.clone(),
			GRANDPA_JUSTIFICATION_PERIOD,
			&(client.clone() as Arc<_>),
			select_chain.clone(),
			telemetry.as_ref().map(|x| x.handle()),
		)?;

		(
			sc_consensus_grandpa::SharedVoterState::empty(),
			grandpa_link.shared_authority_set().clone(),
			grandpa_link.justification_stream(),
			sc_consensus_grandpa::FinalityProofProvider::new_for_service(
				backend.clone(),
				Some(grandpa_link.shared_authority_set().clone()),
			),
		)
	};

	// [CF] Required for Chainspec RPC.
	let genesis_hash = client.block_hash(0).ok().flatten().expect("Genesis block exists; qed");

	let role = config.role.clone();
	let force_authoring = config.force_authoring;
	let backoff_authoring_blocks: Option<()> = None;
	let name = config.network.node_name.clone();
	let enable_grandpa = !config.disable_grandpa;
	let prometheus_registry = config.prometheus_registry().cloned();

	let rpc_builder = {
		let client = client.clone();
		let backend = backend.clone();
		let pool = transaction_pool.clone();
		let executor = Arc::new(task_manager.spawn_handle());
		let chain_spec = config.chain_spec.cloned_box();
		let keystore = keystore_container.local_keystore().clone();

		// try to get the broker key pair from the node keystore
		let broker_key_pair =
			match keystore.sr25519_public_keys(broker_crypto::BROKER_KEY_TYPE_ID).as_slice() {
				[pub_key] => {
					let pub_key = broker_crypto::Public::from(*pub_key);

					keystore
						.key_pair(&pub_key)
						.ok()
						.flatten()
						.map(|pair: broker_crypto::Pair| pair.into_inner())
				},
				[] => None, // No BROKER_KEY_TYPE_ID keys found
				_ => {
					log::warn!("Found more than one broker keys in the node keystore. Disabling broker API ...");
					None
				},
			};

		// try to get the lp key pair from the node keystore
		let lp_key_pair = match keystore.sr25519_public_keys(lp_crypto::LP_KEY_TYPE_ID).as_slice() {
			[pub_key] => {
				let pub_key = lp_crypto::Public::from(*pub_key);

				keystore
					.key_pair(&pub_key)
					.ok()
					.flatten()
					.map(|pair: lp_crypto::Pair| pair.into_inner())
			},
			[] => None, // No LP_KEY_TYPE_ID keys found
			_ => {
				log::warn!("Found more than one lp provider keys in the node keystore. Disabling LP API ...");
				None
			},
		};

		Box::new(move |deny_unsafe, subscription_executor| {
			let build = || {
				let mut module = RpcModule::new(());

				module.merge(substrate_frame_rpc_system::SystemApiServer::into_rpc(
					substrate_frame_rpc_system::System::new(
						client.clone(),
						pool.clone(),
						deny_unsafe,
					),
				))?;

				module.merge(
					pallet_transaction_payment_rpc::TransactionPaymentApiServer::into_rpc(
						pallet_transaction_payment_rpc::TransactionPayment::new(client.clone()),
					),
				)?;

				module.merge(
					chain_spec_rpc::ChainSpec::new(
						chain_spec.name().to_string(),
						genesis_hash,
						chain_spec.properties(),
					)
					.into_rpc(),
				)?;

				module.merge(
					Grandpa::new(
						subscription_executor,
						shared_authority_set.clone(),
						shared_voter_state.clone(),
						justification_stream.clone(),
						finality_provider.clone(),
					)
					.into_rpc(),
				)?;

				// Implement custom RPC extensions
				module.merge(CustomApiServer::into_rpc(CustomRpc::new(
					client.clone(),
					backend.clone(),
					executor.clone(),
				)))?;

				// Implement custom RPC extensions
				module.merge(MonitoringApiServer::into_rpc(CustomRpc::new(
					client.clone(),
					backend.clone(),
					executor.clone(),
				)))?;

				// Add broker RPCs if broker key was found
				if let Some(pair) = broker_key_pair.clone() {
					module.merge(BrokerRpcApiServer::into_rpc(BrokerSignedRpc::new(
						client.clone(),
						backend.clone(),
						executor.clone(),
						pool.clone(),
						pair.clone(),
					)))?;
				}

				// Add lp RPCs if lp key was found
				if let Some(pair) = lp_key_pair.clone() {
					module.merge(LpRpcApiServer::into_rpc(LpSignedRpc::new(
						client.clone(),
						backend.clone(),
						executor.clone(),
						pool.clone(),
						pair.clone(),
					)))?;
				}

				Ok(module)
			};
			build().map_err(sc_service::Error::Application)
		})
	};

	let _rpc_handlers = sc_service::spawn_tasks(sc_service::SpawnTasksParams {
		network: network.clone(),
		client: client.clone(),
		keystore: keystore_container.keystore(),
		task_manager: &mut task_manager,
		transaction_pool: transaction_pool.clone(),
		rpc_builder,
		backend,
		system_rpc_tx,
		tx_handler_controller,
		sync_service: sync_service.clone(),
		config,
		telemetry: telemetry.as_mut(),
	})?;

	if role.is_authority() {
		let proposer_factory = sc_basic_authorship::ProposerFactory::new(
			task_manager.spawn_handle(),
			client.clone(),
			transaction_pool.clone(),
			prometheus_registry.as_ref(),
			telemetry.as_ref().map(|x| x.handle()),
		);

		let slot_duration = sc_consensus_aura::slot_duration(&*client)?;

		let aura = sc_consensus_aura::start_aura::<AuraPair, _, _, _, _, _, _, _, _, _, _>(
			StartAuraParams {
				slot_duration,
				client,
				select_chain,
				block_import,
				proposer_factory,
				create_inherent_data_providers: move |_, ()| async move {
					let timestamp = sp_timestamp::InherentDataProvider::from_system_time();

					let slot =
						sp_consensus_aura::inherents::InherentDataProvider::from_timestamp_and_slot_duration(
							*timestamp,
							slot_duration,
						);

					Ok((slot, timestamp))
				},
				force_authoring,
				backoff_authoring_blocks,
				keystore: keystore_container.keystore(),
				sync_oracle: sync_service.clone(),
				justification_sync_link: sync_service.clone(),
				block_proposal_slot_portion: SlotProportion::new(2f32 / 3f32),
				max_block_proposal_slot_portion: None,
				telemetry: telemetry.as_ref().map(|x| x.handle()),
				compatibility_mode: Default::default(),
			},
		)?;

		// the AURA authoring task is considered essential, i.e. if it
		// fails we take down the service with it.
		task_manager
			.spawn_essential_handle()
			.spawn_blocking("aura", Some("block-authoring"), aura);
	}

	if enable_grandpa {
		// if the node isn't actively participating in consensus then it doesn't
		// need a keystore, regardless of which protocol we use below.
		let keystore = if role.is_authority() { Some(keystore_container.keystore()) } else { None };

		let grandpa_config = sc_consensus_grandpa::Config {
			// FIXME #1578 make this available through chainspec
			gossip_duration: Duration::from_millis(333),
			justification_generation_period: GRANDPA_JUSTIFICATION_PERIOD,
			name: Some(name),
			observer_enabled: false,
			keystore,
			local_role: role,
			telemetry: telemetry.as_ref().map(|x| x.handle()),
			protocol_name: grandpa_protocol_name,
		};

		// start the full GRANDPA voter
		// NOTE: non-authorities could run the GRANDPA observer protocol, but at
		// this point the full voter should provide better guarantees of block
		// and vote data availability than the observer. The observer has not
		// been tested extensively yet and having most nodes in a network run it
		// could lead to finality stalls.
		let grandpa_config = sc_consensus_grandpa::GrandpaParams {
			config: grandpa_config,
			link: grandpa_link,
			network,
			sync: Arc::new(sync_service),
			notification_service: grandpa_notification_service,
			voting_rule: sc_consensus_grandpa::VotingRulesBuilder::default().build(),
			prometheus_registry,
			shared_voter_state: SharedVoterState::empty(),
			telemetry: telemetry.as_ref().map(|x| x.handle()),
			offchain_tx_pool_factory: OffchainTransactionPoolFactory::new(transaction_pool),
		};

		// the GRANDPA voter task is considered infallible, i.e.
		// if it fails we take down the service with it.
		task_manager.spawn_essential_handle().spawn_blocking(
			"grandpa-voter",
			None,
			sc_consensus_grandpa::run_grandpa_voter(grandpa_config)?,
		);
	}

	network_starter.start_network();
	Ok(task_manager)
}
