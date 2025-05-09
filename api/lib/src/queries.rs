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

use super::*;
use cf_chains::{address::ToHumanreadableAddress, instances::ChainInstanceFor, Chain};
use cf_primitives::{AssetAmount, EpochIndex, FlipBalance};
use cf_rpc_types::SwapChannelInfo;
use cf_utilities::task_scope;
use chainflip_engine::state_chain_observer::client::{
	chain_api::ChainApi, storage_api::StorageApi,
};
use codec::Decode;
use custom_rpc::CustomApiClient;
use frame_support::sp_runtime::DigestItem;
use jsonrpsee::core::ClientError;
use pallet_cf_ingress_egress::DepositChannelDetails;
use pallet_cf_validator::RotationPhase;
use sp_consensus_aura::{Slot, AURA_ENGINE_ID};
use state_chain_runtime::runtime_apis::FailingWitnessValidators;
use std::{collections::BTreeMap, ops::Deref, sync::Arc};
use tracing::log;

type RpcResult<T> = Result<T, ClientError>;

pub struct PreUpdateStatus {
	pub rotation: bool,
	pub is_authority: bool,
	pub next_block_in: Option<usize>,
}

pub struct QueryApi {
	pub(crate) state_chain_client: Arc<StateChainClient>,
}

impl QueryApi {
	pub async fn connect(
		scope: &task_scope::Scope<'_, anyhow::Error>,
		state_chain_settings: &settings::StateChain,
	) -> Result<QueryApi> {
		log::debug!("Connecting to state chain at: {}", state_chain_settings.ws_endpoint);

		let (.., state_chain_client) = StateChainClient::connect_with_account(
			scope,
			&state_chain_settings.ws_endpoint,
			&state_chain_settings.signing_key_file,
			AccountRole::Unregistered,
			false,
			false,
			None,
		)
		.await?;

		Ok(Self { state_chain_client })
	}

	pub async fn get_open_swap_channels<C: Chain>(
		&self,
		block_hash: Option<state_chain_runtime::Hash>,
	) -> RpcResult<Vec<SwapChannelInfo<C>>>
	where
		state_chain_runtime::Runtime:
			pallet_cf_ingress_egress::Config<ChainInstanceFor<C>, TargetChain = C>,
	{
		let block_hash =
			block_hash.unwrap_or_else(|| self.state_chain_client.latest_finalized_block().hash);

		let (channels, network_environment) = tokio::try_join!(
				self.state_chain_client
					.storage_map::<pallet_cf_ingress_egress::DepositChannelLookup<
						state_chain_runtime::Runtime,
						ChainInstanceFor<C>,
					>, Vec<_>>(block_hash),
				self.state_chain_client
					.storage_value::<pallet_cf_environment::ChainflipNetworkEnvironment<
						state_chain_runtime::Runtime,
					>>(block_hash),
			)?;

		Ok(channels
			.into_iter()
			.filter_map(|(_, DepositChannelDetails { action, deposit_channel, .. })| match action {
				pallet_cf_ingress_egress::ChannelAction::Swap { destination_asset, .. } =>
					Some(SwapChannelInfo {
						deposit_address: deposit_channel
							.address
							.to_humanreadable(network_environment),
						source_asset: deposit_channel.asset.into(),
						destination_asset,
					}),
				_ => None,
			})
			.collect::<Vec<_>>())
	}

	pub async fn get_balances(
		&self,
		block_hash: Option<state_chain_runtime::Hash>,
	) -> Result<BTreeMap<Asset, AssetAmount>> {
		let block_hash =
			block_hash.unwrap_or_else(|| self.state_chain_client.latest_finalized_block().hash);

		futures::future::join_all(Asset::all().map(|asset| async move {
			Ok((
				asset,
				self.state_chain_client
					.storage_double_map_entry::<pallet_cf_asset_balances::FreeBalances<state_chain_runtime::Runtime>>(
						block_hash,
						&self.state_chain_client.account_id(),
						&asset,
					)
					.await?,
			))
		}))
		.await
		.into_iter()
		.collect()
	}

	pub async fn get_bound_redeem_address(
		&self,
		block_hash: Option<state_chain_runtime::Hash>,
		account_id: Option<state_chain_runtime::AccountId>,
	) -> Result<Option<EthereumAddress>, anyhow::Error> {
		let block_hash =
			block_hash.unwrap_or_else(|| self.state_chain_client.latest_finalized_block().hash);
		let account_id = account_id.unwrap_or_else(|| self.state_chain_client.account_id());

		Ok(self
			.state_chain_client
			.storage_map_entry::<pallet_cf_funding::BoundRedeemAddress<state_chain_runtime::Runtime>>(
				block_hash,
				&account_id,
			)
			.await?)
	}

	pub async fn get_bound_executor_address(
		&self,
		block_hash: Option<state_chain_runtime::Hash>,
		account_id: Option<state_chain_runtime::AccountId>,
	) -> Result<Option<EthereumAddress>, anyhow::Error> {
		let block_hash =
			block_hash.unwrap_or_else(|| self.state_chain_client.latest_finalized_block().hash);
		let account_id = account_id.unwrap_or_else(|| self.state_chain_client.account_id());

		Ok(self
			.state_chain_client
			.storage_map_entry::<pallet_cf_funding::BoundExecutorAddress<state_chain_runtime::Runtime>>(
				block_hash,
				&account_id,
			)
			.await?)
	}

	pub async fn get_restricted_balances(
		&self,
		block_hash: Option<state_chain_runtime::Hash>,
		account_id: Option<state_chain_runtime::AccountId>,
	) -> Result<BTreeMap<EthereumAddress, FlipBalance>> {
		let block_hash =
			block_hash.unwrap_or_else(|| self.state_chain_client.latest_finalized_block().hash);
		let account_id = account_id.unwrap_or_else(|| self.state_chain_client.account_id());

		Ok(self
			.state_chain_client
			.storage_map_entry::<pallet_cf_funding::RestrictedBalances<state_chain_runtime::Runtime>>(
				block_hash,
				&account_id,
			)
			.await?)
	}

	pub async fn pre_update_check(
		&self,
		block_hash: Option<state_chain_runtime::Hash>,
		account_id: Option<state_chain_runtime::AccountId>,
	) -> Result<PreUpdateStatus, anyhow::Error> {
		let block_hash =
			block_hash.unwrap_or_else(|| self.state_chain_client.latest_finalized_block().hash);
		let account_id = account_id.unwrap_or_else(|| self.state_chain_client.account_id());

		let mut result =
			PreUpdateStatus { rotation: false, is_authority: false, next_block_in: None };

		if self
			.state_chain_client
			.storage_value::<pallet_cf_validator::CurrentRotationPhase<state_chain_runtime::Runtime>>(
				block_hash,
			)
			.await? != RotationPhase::Idle
		{
			result.rotation = true;
		}

		let current_validators = self
			.state_chain_client
			.storage_value::<pallet_cf_validator::CurrentAuthorities<state_chain_runtime::Runtime>>(
				block_hash,
			)
			.await?;

		if current_validators.contains(&account_id) {
			result.is_authority = true;
		} else {
			return Ok(result)
		}

		let header = self.state_chain_client.base_rpc_client.block_header(block_hash).await?;

		let slot: usize =
			*extract_slot_from_digest_item(&header.digest.logs[0]).unwrap().deref() as usize;

		let validator_len = current_validators.len();
		let current_relative_slot = slot % validator_len;
		let index = current_validators.iter().position(|account| account == &account_id).unwrap();

		result.next_block_in = Some(compute_distance(index, current_relative_slot, validator_len));
		Ok(result)
	}

	pub async fn check_witnesses(
		&self,
		block_hash: Option<state_chain_runtime::Hash>,
		hash: state_chain_runtime::Hash,
		epoch_index: Option<EpochIndex>,
	) -> Result<Option<FailingWitnessValidators>, anyhow::Error> {
		let result = self
			.state_chain_client
			.base_rpc_client
			.raw_rpc_client
			.cf_witness_count(hash, epoch_index, block_hash)
			.await?;

		Ok(result)
	}
}

// https://github.com/chainflip-io/substrate/blob/c172d0f683fab3792b90d876fd6ca27056af9fe9/frame/aura/src/lib.rs#L179
fn extract_slot_from_digest_item(item: &DigestItem) -> Option<Slot> {
	item.as_pre_runtime().and_then(|(id, mut data)| {
		if id == AURA_ENGINE_ID {
			Slot::decode(&mut data).ok()
		} else {
			None
		}
	})
}

fn compute_distance(index: usize, slot: usize, len: usize) -> usize {
	if index >= slot {
		index - slot
	} else {
		len - slot + index
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use codec::Encode;

	#[test]
	fn test_slot_extraction() {
		let slot = Slot::from(42);
		assert_eq!(
			Some(slot),
			extract_slot_from_digest_item(&DigestItem::PreRuntime(
				AURA_ENGINE_ID,
				Encode::encode(&slot)
			))
		);
		assert_eq!(
			None,
			extract_slot_from_digest_item(&DigestItem::PreRuntime(*b"BORA", Encode::encode(&slot)))
		);
		assert_eq!(
			None,
			extract_slot_from_digest_item(&DigestItem::Other(b"SomethingElse".to_vec()))
		);
	}

	#[test]
	fn test_compute_distance() {
		let index: usize = 5;
		let slot: usize = 7;
		let len: usize = 15;

		assert_eq!(compute_distance(index, slot, len), 13);

		let index: usize = 18;
		let slot: usize = 7;
		let len: usize = 24;

		assert_eq!(compute_distance(index, slot, len), 11);
	}
}
