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
	address::EncodedAddress,
	evm::{api::EvmCall, tokenizable::Tokenizable},
};
use cf_primitives::{Asset, AssetAmount};
use codec::{Decode, Encode};
use ethabi::Token;
use frame_support::sp_runtime::RuntimeDebug;
use scale_info::TypeInfo;
use sp_core::U256;
use sp_std::{vec, vec::Vec};

/// Represents all the arguments required to build the call to Vault's 'XCallNative'
/// function.
#[derive(Encode, Decode, TypeInfo, Clone, RuntimeDebug, PartialEq, Eq)]
pub struct XCallNative {
	// Add fields here
}

impl XCallNative {
	pub fn new(
		// add fields required here
	) -> Self {
		todo!()
	}
}

impl EvmCall for XCallNative {
	const FUNCTION_NAME: &'static str = "xCallNative";

	fn function_params() -> Vec<(&'static str, ethabi::ParamType)> {
		// match the fields with fn name + types
		todo!()
	}

	fn function_call_args(&self) -> Vec<Token> {
		todo!()
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use crate::{
		eth::api::abi::load_abi,
		evm::api::{vault_swaps::test_utils::*, EvmTransactionBuilder},
	};
	use cf_primitives::ForeignChain;

	#[test]
	fn test_payload() {
		// Use other test_payload() unit test as reference
	}
}
