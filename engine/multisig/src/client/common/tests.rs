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
	client::{helpers::ACCOUNT_IDS, keygen::generate_key_data},
	eth::EvmCryptoScheme,
};

use rand::{rngs::StdRng, SeedableRng};
use std::collections::BTreeSet;

#[test]
fn ensure_keygen_result_info_serialization_is_consistent() {
	// Test against pre-computed values to ensure that
	// serialization does not change unintentionally.
	// Any change will require a database migration.

	let mut rng = StdRng::from_seed([0; 32]);

	let expected_bytes = [
		3, 121, 201, 158, 126, 155, 226, 97, 84, 230, 87, 8, 58, 89, 22, 223, 14, 207, 238, 172,
		231, 198, 219, 48, 126, 211, 76, 2, 226, 154, 227, 36, 1, 60, 101, 27, 228, 255, 192, 133,
		190, 220, 89, 12, 166, 160, 134, 204, 6, 154, 21, 95, 12, 6, 83, 8, 103, 90, 101, 137, 132,
		177, 88, 160, 170, 4, 0, 0, 0, 0, 0, 0, 0, 48, 0, 0, 0, 0, 0, 0, 0, 53, 67, 54, 50, 67,
		107, 52, 85, 114, 70, 80, 105, 66, 116, 111, 67, 109, 101, 83, 114, 103, 70, 55, 120, 57,
		121, 118, 57, 109, 110, 51, 56, 52, 52, 54, 100, 104, 67, 112, 115, 105, 50, 109, 76, 72,
		105, 70, 84, 2, 30, 38, 239, 121, 45, 20, 37, 87, 104, 73, 25, 115, 12, 146, 95, 91, 88,
		186, 124, 155, 66, 13, 252, 81, 46, 130, 136, 54, 226, 12, 57, 163, 48, 0, 0, 0, 0, 0, 0,
		0, 53, 67, 55, 76, 89, 112, 80, 50, 90, 72, 51, 116, 112, 75, 98, 118, 86, 118, 119, 105,
		86, 101, 53, 52, 65, 97, 112, 120, 69, 114, 100, 80, 66, 98, 118, 107, 89, 104, 101, 54,
		121, 57, 90, 66, 107, 113, 87, 116, 2, 144, 90, 232, 163, 35, 128, 241, 13, 8, 247, 15,
		101, 196, 216, 209, 175, 123, 0, 38, 59, 20, 199, 235, 65, 26, 70, 83, 100, 91, 137, 190,
		247, 48, 0, 0, 0, 0, 0, 0, 0, 53, 67, 56, 101, 116, 116, 104, 97, 71, 74, 105, 53, 83, 107,
		81, 101, 69, 68, 83, 97, 75, 51, 50, 65, 66, 66, 106, 107, 104, 119, 68, 101, 75, 57, 107,
		115, 81, 67, 84, 76, 69, 71, 77, 51, 69, 72, 49, 52, 3, 144, 240, 88, 153, 103, 231, 98,
		27, 52, 152, 102, 88, 39, 34, 46, 148, 110, 35, 38, 211, 154, 74, 73, 213, 36, 112, 181,
		204, 55, 149, 24, 191, 48, 0, 0, 0, 0, 0, 0, 0, 53, 67, 57, 121, 69, 121, 50, 55, 121, 76,
		78, 71, 53, 66, 68, 77, 120, 86, 119, 83, 56, 82, 121, 71, 66, 110, 101, 90, 66, 49, 111,
		117, 83, 104, 97, 122, 70, 104, 71, 90, 86, 80, 56, 116, 104, 75, 53, 122, 3, 128, 123,
		121, 74, 235, 247, 44, 160, 220, 251, 255, 30, 184, 230, 60, 20, 21, 113, 161, 75, 172,
		249, 51, 213, 49, 233, 232, 142, 134, 212, 128, 190, 4, 0, 0, 0, 0, 0, 0, 0, 48, 0, 0, 0,
		0, 0, 0, 0, 53, 67, 54, 50, 67, 107, 52, 85, 114, 70, 80, 105, 66, 116, 111, 67, 109, 101,
		83, 114, 103, 70, 55, 120, 57, 121, 118, 57, 109, 110, 51, 56, 52, 52, 54, 100, 104, 67,
		112, 115, 105, 50, 109, 76, 72, 105, 70, 84, 48, 0, 0, 0, 0, 0, 0, 0, 53, 67, 55, 76, 89,
		112, 80, 50, 90, 72, 51, 116, 112, 75, 98, 118, 86, 118, 119, 105, 86, 101, 53, 52, 65, 97,
		112, 120, 69, 114, 100, 80, 66, 98, 118, 107, 89, 104, 101, 54, 121, 57, 90, 66, 107, 113,
		87, 116, 48, 0, 0, 0, 0, 0, 0, 0, 53, 67, 56, 101, 116, 116, 104, 97, 71, 74, 105, 53, 83,
		107, 81, 101, 69, 68, 83, 97, 75, 51, 50, 65, 66, 66, 106, 107, 104, 119, 68, 101, 75, 57,
		107, 115, 81, 67, 84, 76, 69, 71, 77, 51, 69, 72, 49, 52, 48, 0, 0, 0, 0, 0, 0, 0, 53, 67,
		57, 121, 69, 121, 50, 55, 121, 76, 78, 71, 53, 66, 68, 77, 120, 86, 119, 83, 56, 82, 121,
		71, 66, 110, 101, 90, 66, 49, 111, 117, 83, 104, 97, 122, 70, 104, 71, 90, 86, 80, 56, 116,
		104, 75, 53, 122, 4, 0, 0, 0, 2, 0, 0, 0,
	];

	let keygen_result_info =
		generate_key_data::<EvmCryptoScheme>(BTreeSet::from_iter(ACCOUNT_IDS.clone()), &mut rng)
			.1
			.get(&ACCOUNT_IDS[0])
			.expect("should get keygen for an account")
			.to_owned();

	let keygen_result_info_bytes = bincode::serialize(&keygen_result_info).unwrap();

	assert_eq!(expected_bytes.to_vec(), keygen_result_info_bytes);
}
