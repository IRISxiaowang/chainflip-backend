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

use crate::{
	chainflip::{
		address_derivation::btc::derive_btc_vault_deposit_addresses, AddressConverter,
		ChainAddressConverter, EvmEnvironment, SolEnvironment,
	},
	runtime_apis::{DispatchErrorWithMessage, EvmVaultSwapDetails, VaultSwapDetails},
	AccountId, BlockNumber, Environment, Runtime, Swapping,
};

use cf_chains::{
	address::EncodedAddress,
	btc::vault_swap_encoding::{
		encode_swap_params_in_nulldata_payload, BtcCfParameters, UtxoEncodedData,
	},
	cf_parameters::build_cf_parameters,
	evm::api::{EvmCall, EvmEnvironmentProvider},
	sol::{
		api::SolanaEnvironment, instruction_builder::SolanaInstructionBuilder,
		sol_tx_core::address_derivation::derive_associated_token_account, SolAmount, SolPubkey,
	},
	Arbitrum, CcmChannelMetadata, ChannelRefundParametersEncoded, Ethereum, ForeignChain, Solana,
};
use cf_primitives::{
	AffiliateAndFee, Affiliates, Asset, AssetAmount, BasisPoints, DcaParameters, SWAP_DELAY_BLOCKS,
};
use cf_traits::AffiliateRegistry;
use scale_info::prelude::string::String;
use sp_core::U256;
use sp_std::vec::Vec;

fn to_affiliate_and_fees(
	broker_id: &AccountId,
	affiliates: Affiliates<AccountId>,
) -> Result<Vec<AffiliateAndFee>, DispatchErrorWithMessage> {
	let mapping = <Swapping as AffiliateRegistry>::reverse_mapping(broker_id);
	affiliates
		.into_iter()
		.map(|beneficiary| {
			Ok(AffiliateAndFee {
				affiliate: *mapping
					.get(&beneficiary.account)
					.ok_or(pallet_cf_swapping::Error::<Runtime>::AffiliateNotRegisteredForBroker)?,
				fee: beneficiary
					.bps
					.try_into()
					.map_err(|_| pallet_cf_swapping::Error::<Runtime>::AffiliateFeeTooHigh)?,
			})
		})
		.collect::<Result<Vec<AffiliateAndFee>, _>>()
}

pub fn bitcoin_vault_swap(
	broker_id: AccountId,
	destination_asset: Asset,
	destination_address: EncodedAddress,
	broker_commission: BasisPoints,
	min_output_amount: AssetAmount,
	retry_duration: BlockNumber,
	boost_fee: u8,
	affiliate_fees: Affiliates<AccountId>,
	dca_parameters: Option<DcaParameters>,
) -> Result<VaultSwapDetails<String>, DispatchErrorWithMessage> {
	let private_channel_id =
		pallet_cf_swapping::BrokerPrivateBtcChannels::<Runtime>::get(&broker_id)
			.ok_or(pallet_cf_swapping::Error::<Runtime>::NoPrivateChannelExistsForBroker)?;
	let params = UtxoEncodedData {
		output_asset: destination_asset,
		output_address: destination_address,
		parameters: BtcCfParameters {
			retry_duration: retry_duration
				.try_into()
				.map_err(|_| pallet_cf_swapping::Error::<Runtime>::SwapRequestDurationTooLong)?,
			min_output_amount,
			number_of_chunks: dca_parameters
				.as_ref()
				.map(|params| params.number_of_chunks)
				.unwrap_or(1)
				.try_into()
				.map_err(|_| pallet_cf_swapping::Error::<Runtime>::InvalidDcaParameters)?,
			chunk_interval: dca_parameters
				.as_ref()
				.map(|params| params.chunk_interval)
				.unwrap_or(SWAP_DELAY_BLOCKS)
				.try_into()
				.map_err(|_| pallet_cf_swapping::Error::<Runtime>::InvalidDcaParameters)?,
			boost_fee,
			broker_fee: broker_commission
				.try_into()
				.map_err(|_| pallet_cf_swapping::Error::<Runtime>::BrokerFeeTooHigh)?,
			affiliates: to_affiliate_and_fees(&broker_id, affiliate_fees)?
				.try_into()
				.map_err(|_| "Too many affiliates.")?,
		},
	};

	Ok(VaultSwapDetails::Bitcoin {
		nulldata_payload: encode_swap_params_in_nulldata_payload(params),
		deposit_address: derive_btc_vault_deposit_addresses(private_channel_id).current_address(),
	})
}

pub fn evm_vault_swap<A>(
	broker_id: AccountId,
	source_asset: Asset,
	amount: AssetAmount,
	destination_asset: Asset,
	destination_address: EncodedAddress,
	broker_commission: BasisPoints,
	refund_params: ChannelRefundParametersEncoded,
	boost_fee: u8,
	affiliate_fees: Affiliates<AccountId>,
	dca_parameters: Option<DcaParameters>,
	channel_metadata: Option<cf_chains::CcmChannelMetadata>,
) -> Result<VaultSwapDetails<A>, DispatchErrorWithMessage> {
	// map the refund parameter

	// Map the affiliates

	// encode cf_parameters

	let calldata = match source_asset {
		Asset::Eth | Asset::ArbEth =>
			if let Some(ccm) = channel_metadata {
				todo!("Native Eth + CCM = XCallNative")
			} else {
				todo!("Native Eth + no CCM = XSwapNative")
			},
		Asset::Flip | Asset::Usdc | Asset::Usdt | Asset::ArbUsdc => {
			// Lookup Token addresses depending on the Chain
			
			// Create the encoded ApiCall
			if let Some(ccm) = channel_metadata {
				todo!("Token + CCM = XCallToken")
			} else {
				todo!("Token + no CCM = XSwapToken")
			}
		},
		_ => Err(DispatchErrorWithMessage::from(
			"Only EVM chains should execute this branch of logic. This error should never happen",
		)),
	}?;

	todo!("Finalize the call into `VaultSwapDetails` depending on the source chain (Ethereum/Arbitrum)")
}

pub fn solana_vault_swap<A>(
	broker_id: AccountId,
	input_amount: AssetAmount,
	source_asset: Asset,
	destination_asset: Asset,
	destination_address: EncodedAddress,
	broker_commission: BasisPoints,
	refund_parameters: ChannelRefundParametersEncoded,
	channel_metadata: Option<CcmChannelMetadata>,
	boost_fee: u8,
	affiliate_fees: Affiliates<AccountId>,
	dca_parameters: Option<DcaParameters>,
	from: EncodedAddress,
	event_data_account: EncodedAddress,
	from_token_account: Option<EncodedAddress>,
) -> Result<VaultSwapDetails<A>, DispatchErrorWithMessage> {
	// Load up environment variables.

	// Derive `swap_endpoint_native_vault`
	
	// map the affiliate fees

	// Map the refund parameters

	// Encode the cf_parameters

	// Create the encoded call
	// let instruction = 
	match source_asset {
		Asset::Sol => todo!("Create the Instruction using the SolanaInstructionBuilder"),
		Asset::SolUsdc => {
			// Derive the `token_supported_account`
			
			// Derive the `from_token_account` if not supplied

			todo!("Create the Instruction using the SolanaInstructionBuilder")
		},
		_ => {} //Err("Invalid source_asset: Not a Solana asset."),
	};

	todo!("Create the final VaultSwapDetails")
}
