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
use cf_chains::Ethereum;
use cf_primitives::{AssetAmount, EthAmount, FLIPPERINOS_PER_FLIP, MAX_BASIS_POINTS};

use sp_std::collections::btree_set::BTreeSet;

type AccountId = u32;
type TestPool = BoostPool<AccountId, Ethereum>;
type Amount = <Ethereum as cf_chains::Chain>::ChainAmount;

const BOOSTER_1: AccountId = 1;
const BOOSTER_2: AccountId = 2;
const BOOSTER_3: AccountId = 3;

const BOOST_1: PrewitnessedDepositId = 1;
const BOOST_2: PrewitnessedDepositId = 2;

const NO_DEDUCTION: Percent = Percent::from_percent(0);

#[test]
fn check_fee_math() {
	type Amount = ScaledAmount<Ethereum>;

	let boosted_amount = Amount::from_raw(1_000_000);
	assert_eq!(super::fee_from_boosted_amount(boosted_amount, 10), Amount::from_raw(1_000));

	let provided_amount = Amount::from_raw(1_000_000);
	assert_eq!(super::fee_from_provided_amount(provided_amount, 10), Ok(Amount::from_raw(1_001)));
}

#[track_caller]
pub fn check_pool(pool: &TestPool, amounts: impl IntoIterator<Item = (AccountId, Amount)>) {
	assert_eq!(
		BTreeMap::from_iter(
			pool.amounts.iter().map(|(id, amount)| (*id, amount.into_chain_amount()))
		),
		BTreeMap::from_iter(amounts.into_iter()),
		"mismatch in booster amounts"
	);
	let total_amount: ScaledAmount<Ethereum> = pool
		.amounts
		.values()
		.fold(Default::default(), |acc, x| acc.checked_add(*x).unwrap());
	assert_eq!(pool.available_amount, total_amount);
}

#[track_caller]
fn check_pending_boosts(
	pool: &TestPool,
	boosts: impl IntoIterator<Item = (PrewitnessedDepositId, Vec<(AccountId, Amount, Amount)>)>,
) {
	let expected_boosts: BTreeMap<_, _> = boosts.into_iter().collect();

	assert_eq!(
		BTreeSet::from_iter(pool.pending_boosts.keys().copied()),
		BTreeSet::from_iter(expected_boosts.keys().copied()),
		"mismatch in pending boosts ids"
	);

	for (prewitnessed_deposit_id, boost_amounts) in &pool.pending_boosts {
		let expected_amounts = &expected_boosts[prewitnessed_deposit_id];

		assert_eq!(
			BTreeMap::from_iter(
				expected_amounts.iter().map(|(id, total, fee)| (*id, (*total, *fee)))
			),
			BTreeMap::from_iter(boost_amounts.iter().map(|(id, owed_amount)| (
				*id,
				(owed_amount.total.into_chain_amount(), owed_amount.fee.into_chain_amount())
			)))
		)
	}
}

#[track_caller]
fn check_pending_withdrawals(
	pool: &TestPool,
	withdrawals: impl IntoIterator<Item = (AccountId, Vec<PrewitnessedDepositId>)>,
) {
	let expected_withdrawals: BTreeMap<_, BTreeSet<_>> = withdrawals
		.into_iter()
		.map(|(account_id, prewitnessed_deposit_ids)| {
			(account_id, prewitnessed_deposit_ids.into_iter().collect())
		})
		.collect();

	assert_eq!(pool.pending_withdrawals, expected_withdrawals, "mismatch in pending withdrawals");
}

#[test]
fn test_scaled_amount() {
	use cf_chains::Ethereum;
	// This shows that we can have unreasonably large amounts in chains with
	// a large number of decimals and still fit into u128 after scaling up:

	// 1 trillion FLIP (or ETH; other chains have smaller number of decimals)
	let original: EthAmount = 1_000_000_000_000 * FLIPPERINOS_PER_FLIP;
	let scaled: ScaledAmount<Ethereum> = ScaledAmount::from_chain_amount(original);
	let recovered: EthAmount = scaled.into_chain_amount();
	assert_eq!(original, recovered);
}

#[test]
fn adding_funds() {
	let mut pool = TestPool::new(5);

	pool.add_funds(BOOSTER_1, 1000);
	check_pool(&pool, [(BOOSTER_1, 1000)]);

	pool.add_funds(BOOSTER_1, 500);
	check_pool(&pool, [(BOOSTER_1, 1500)]);

	pool.add_funds(BOOSTER_2, 800);
	check_pool(&pool, [(BOOSTER_1, 1500), (BOOSTER_2, 800)]);
}

#[test]
fn withdrawing_funds() {
	let mut pool = TestPool::new(5);
	pool.add_funds(BOOSTER_1, 1000);
	pool.add_funds(BOOSTER_2, 900);
	pool.add_funds(BOOSTER_3, 800);
	check_pool(&pool, [(BOOSTER_1, 1000), (BOOSTER_2, 900), (BOOSTER_3, 800)]);

	// No pending to receive, should be able to withdraw in full
	assert_eq!(pool.stop_boosting(BOOSTER_1), Ok((1000, Default::default())));
	check_pool(&pool, [(BOOSTER_2, 900), (BOOSTER_3, 800)]);
	check_pending_withdrawals(&pool, []);

	assert_eq!(pool.stop_boosting(BOOSTER_2), Ok((900, Default::default())));
	check_pool(&pool, [(BOOSTER_3, 800)]);

	assert_eq!(pool.stop_boosting(BOOSTER_3), Ok((800, Default::default())));
	check_pool(&pool, []);
}

#[test]
fn withdrawing_twice_is_no_op() {
	const AMOUNT_1: AssetAmount = 1000;
	const AMOUNT_2: AssetAmount = 750;

	let mut pool = TestPool::new(0);
	pool.add_funds(BOOSTER_1, AMOUNT_1);
	pool.add_funds(BOOSTER_2, AMOUNT_2);

	assert_eq!(pool.stop_boosting(BOOSTER_1), Ok((AMOUNT_1, Default::default())));

	check_pool(&pool, [(BOOSTER_2, AMOUNT_2)]);

	assert_eq!(pool.stop_boosting(BOOSTER_1), Err(Error::AccountNotFoundInBoostPool));

	// No changes:
	check_pool(&pool, [(BOOSTER_2, AMOUNT_2)]);
}

#[test]
fn boosting_with_fees() {
	let mut pool = TestPool::new(100);

	pool.add_funds(BOOSTER_1, 1000);
	pool.add_funds(BOOSTER_2, 2000);

	check_pool(&pool, [(BOOSTER_1, 1000), (BOOSTER_2, 2000)]);

	assert_eq!(pool.provide_funds_for_boosting(BOOST_1, 1010, NO_DEDUCTION), Ok((1010, 10)));

	// The recorded amounts include fees (1 is missing due to rounding errors in *test* code)
	check_pending_boosts(
		&pool,
		[(BOOST_1, vec![(BOOSTER_1, 333 + 3, 3), (BOOSTER_2, 667 + 6, 6)])],
	);

	assert_eq!(
		pool.process_deposit_as_finalised(BOOST_1),
		DepositFinalisationOutcomeForPool {
			amount_credited_to_boosters: 1010,
			unlocked_funds: vec![]
		}
	);

	check_pool(&pool, [(BOOSTER_1, 1003), (BOOSTER_2, 2006)]);
}

#[test]
fn boosting_with_max_network_fee_deduction() {
	const BOOST_FEE_BPS: u16 = 100;
	const INIT_BOOSTER_AMOUNT: u128 = 2000;
	const NETWORK_FEE_PORTION_PERCENT: u8 = 100;

	let mut pool = TestPool::new(BOOST_FEE_BPS);

	pool.add_funds(BOOSTER_1, INIT_BOOSTER_AMOUNT);

	check_pool(&pool, [(BOOSTER_1, INIT_BOOSTER_AMOUNT)]);

	const DEPOSIT_AMOUNT: u128 = 2000;
	const FULL_BOOST_FEE: u128 = DEPOSIT_AMOUNT * BOOST_FEE_BPS as u128 / MAX_BASIS_POINTS as u128;
	const PROVIDED_AMOUNT: u128 = DEPOSIT_AMOUNT - FULL_BOOST_FEE;

	// NOTE: Full 1% boost fee is charged from the deposit
	assert_eq!(
		pool.provide_funds_for_boosting(
			BOOST_1,
			DEPOSIT_AMOUNT,
			Percent::from_percent(NETWORK_FEE_PORTION_PERCENT)
		),
		Ok((DEPOSIT_AMOUNT, FULL_BOOST_FEE))
	);

	// Booster's contribution is recorded, but they earn 0 fees:
	check_pending_boosts(&pool, [(BOOST_1, vec![(BOOSTER_1, PROVIDED_AMOUNT, 0)])]);

	assert_eq!(
		pool.process_deposit_as_finalised(BOOST_1),
		DepositFinalisationOutcomeForPool {
			amount_credited_to_boosters: PROVIDED_AMOUNT,
			unlocked_funds: vec![]
		}
	);

	// No change in the boost pool after deposit is finalised:
	check_pool(&pool, [(BOOSTER_1, INIT_BOOSTER_AMOUNT)]);
}

#[test]
fn boosting_with_fees_including_network_fee_portion() {
	const NETWORK_FEE_PORTION_PERCENT: u8 = 30;
	const BOOST_FEE_BPS: u16 = 100;

	let mut pool = TestPool::new(BOOST_FEE_BPS);

	pool.add_funds(BOOSTER_1, 1000);
	pool.add_funds(BOOSTER_2, 2000);

	check_pool(&pool, [(BOOSTER_1, 1000), (BOOSTER_2, 2000)]);

	const PROVIDED_AMOUNT: u128 = 1000;
	const FULL_BOOST_FEE: u128 =
		(PROVIDED_AMOUNT * BOOST_FEE_BPS as u128) / MAX_BASIS_POINTS as u128;

	const DEPOSIT_AMOUNT: u128 = PROVIDED_AMOUNT + FULL_BOOST_FEE;

	// NOTE: Full 1% boost fee is charged from the deposit
	assert_eq!(
		pool.provide_funds_for_boosting(
			BOOST_1,
			DEPOSIT_AMOUNT,
			Percent::from_percent(NETWORK_FEE_PORTION_PERCENT)
		),
		Ok((DEPOSIT_AMOUNT, FULL_BOOST_FEE))
	);

	const BOOSTER_1_FEE: u128 = 2;
	const BOOSTER_2_FEE: u128 = 4;

	const TOTAL_BOOSTERS_FEE: u128 = BOOSTER_1_FEE + BOOSTER_2_FEE;

	const NETWORK_FEE_FROM_BOOST: u128 = FULL_BOOST_FEE * NETWORK_FEE_PORTION_PERCENT as u128 / 100;

	// Sanity check: network fee and boosters fee should make up the full boost fee.
	// Note that we subtract 1 to account for a rounding "error" (in real code any
	// remaining fee will be used as network fee, so all atomic units will be accounted for).
	assert_eq!(TOTAL_BOOSTERS_FEE, FULL_BOOST_FEE - NETWORK_FEE_FROM_BOOST - 1);

	// The recorded amounts include fees
	check_pending_boosts(
		&pool,
		[(
			BOOST_1,
			vec![
				(BOOSTER_1, 333 + BOOSTER_1_FEE, BOOSTER_1_FEE),
				(BOOSTER_2, 667 + BOOSTER_2_FEE, BOOSTER_2_FEE),
			],
		)],
	);

	assert_eq!(
		pool.process_deposit_as_finalised(BOOST_1),
		DepositFinalisationOutcomeForPool {
			amount_credited_to_boosters: PROVIDED_AMOUNT + TOTAL_BOOSTERS_FEE,
			unlocked_funds: vec![]
		}
	);

	check_pool(&pool, [(BOOSTER_1, 1000 + BOOSTER_1_FEE), (BOOSTER_2, 2000 + BOOSTER_2_FEE)]);
}

#[test]
fn adding_funds_during_pending_withdrawal_from_same_booster() {
	const AMOUNT_1: AssetAmount = 1000;
	const AMOUNT_2: AssetAmount = 3000;
	const DEPOSIT_AMOUNT: AssetAmount = 2000;

	let mut pool = TestPool::new(0);

	pool.add_funds(BOOSTER_1, AMOUNT_1);
	pool.add_funds(BOOSTER_2, AMOUNT_2);

	assert_eq!(
		pool.provide_funds_for_boosting(BOOST_1, DEPOSIT_AMOUNT, NO_DEDUCTION),
		Ok((DEPOSIT_AMOUNT, 0))
	);
	check_pool(&pool, [(BOOSTER_1, 500), (BOOSTER_2, 1500)]);

	check_pending_boosts(&pool, [(BOOST_1, vec![(BOOSTER_1, 500, 0), (BOOSTER_2, 1500, 0)])]);

	assert_eq!(pool.stop_boosting(BOOSTER_1), Ok((500, BTreeSet::from_iter([BOOST_1]))));

	check_pool(&pool, [(BOOSTER_2, 1500)]);
	check_pending_boosts(&pool, [(BOOST_1, vec![(BOOSTER_1, 500, 0), (BOOSTER_2, 1500, 0)])]);
	check_pending_withdrawals(&pool, [(BOOSTER_1, vec![BOOST_1])]);

	// Booster 1 has a pending withdrawal, but they add more funds, so we assume they
	// no longer want to withdraw:
	pool.add_funds(BOOSTER_1, 1000);
	check_pending_withdrawals(&pool, []);

	// Booster 1 is no longer withdrawing, so pending funds go into available pool
	// on finalisation:
	assert_eq!(
		pool.process_deposit_as_finalised(BOOST_1),
		DepositFinalisationOutcomeForPool {
			amount_credited_to_boosters: DEPOSIT_AMOUNT,
			unlocked_funds: vec![]
		}
	);
	check_pool(&pool, [(BOOSTER_1, 1500), (BOOSTER_2, AMOUNT_2)]);
}

#[test]
fn withdrawing_funds_before_finalisation() {
	let mut pool = TestPool::new(0);
	pool.add_funds(BOOSTER_1, 1000);
	pool.add_funds(BOOSTER_2, 1000);

	assert_eq!(pool.provide_funds_for_boosting(BOOST_1, 1000, NO_DEDUCTION), Ok((1000, 0)));
	check_pool(&pool, [(BOOSTER_1, 500), (BOOSTER_2, 500)]);

	// Only some of the funds are available immediately, and some are in pending withdrawals:
	assert_eq!(pool.stop_boosting(BOOSTER_1), Ok((500, BTreeSet::from_iter([BOOST_1]))));
	check_pool(&pool, [(BOOSTER_2, 500)]);

	assert_eq!(
		pool.process_deposit_as_finalised(BOOST_1),
		DepositFinalisationOutcomeForPool {
			amount_credited_to_boosters: 1000,
			unlocked_funds: vec![(BOOSTER_1, 500)]
		}
	);
	check_pool(&pool, [(BOOSTER_2, 1000)]);
}

#[test]
fn adding_funds_with_pending_withdrawals() {
	let mut pool = TestPool::new(0);
	pool.add_funds(BOOSTER_1, 1000);
	pool.add_funds(BOOSTER_2, 1000);

	assert_eq!(pool.provide_funds_for_boosting(BOOST_1, 1000, NO_DEDUCTION), Ok((1000, 0)));

	check_pool(&pool, [(BOOSTER_1, 500), (BOOSTER_2, 500)]);

	// Only some of the funds are available immediately, and some are in pending withdrawals:
	assert_eq!(pool.stop_boosting(BOOSTER_1), Ok((500, BTreeSet::from_iter([BOOST_1]))));
	check_pool(&pool, [(BOOSTER_2, 500)]);

	pool.add_funds(BOOSTER_3, 1000);
	check_pool(&pool, [(BOOSTER_2, 500), (BOOSTER_3, 1000)]);

	assert_eq!(
		pool.process_deposit_as_finalised(BOOST_1),
		DepositFinalisationOutcomeForPool {
			amount_credited_to_boosters: 1000,
			unlocked_funds: vec![(BOOSTER_1, 500)]
		}
	);

	check_pool(&pool, [(BOOSTER_2, 1000), (BOOSTER_3, 1000)]);
}

#[test]
fn deposit_is_lost_no_withdrawal() {
	let mut pool = TestPool::new(0);
	pool.add_funds(BOOSTER_1, 1000);
	pool.add_funds(BOOSTER_2, 1000);
	check_pool(&pool, [(BOOSTER_1, 1000), (BOOSTER_2, 1000)]);

	assert_eq!(pool.provide_funds_for_boosting(BOOST_1, 1000, NO_DEDUCTION), Ok((1000, 0)));
	pool.process_deposit_as_lost(BOOST_1);
	check_pool(&pool, [(BOOSTER_1, 500), (BOOSTER_2, 500)]);
}

#[test]
fn deposit_is_lost_while_withdrawing() {
	let mut pool = TestPool::new(0);
	pool.add_funds(BOOSTER_1, 1000);
	pool.add_funds(BOOSTER_2, 1000);
	assert_eq!(pool.provide_funds_for_boosting(BOOST_1, 1000, NO_DEDUCTION), Ok((1000, 0)));
	assert_eq!(pool.stop_boosting(BOOSTER_1), Ok((500, BTreeSet::from_iter([BOOST_1]))));

	check_pool(&pool, [(BOOSTER_2, 500)]);
	check_pending_boosts(&pool, [(BOOST_1, vec![(BOOSTER_1, 500, 0), (BOOSTER_2, 500, 0)])]);
	check_pending_withdrawals(&pool, [(BOOSTER_1, vec![BOOST_1])]);

	pool.process_deposit_as_lost(BOOST_1);

	check_pool(&pool, [(BOOSTER_2, 500)]);
	// BOOSTER_1 is not considered "withdrawing" because they no longer await
	// for any deposits to finalise:
	check_pending_boosts(&pool, []);
}

#[test]
fn partially_losing_pending_withdrawals() {
	let mut pool = TestPool::new(0);
	pool.add_funds(BOOSTER_1, 1000);
	pool.add_funds(BOOSTER_2, 1000);

	assert_eq!(pool.provide_funds_for_boosting(BOOST_1, 500, NO_DEDUCTION), Ok((500, 0)));
	assert_eq!(pool.provide_funds_for_boosting(BOOST_2, 1000, NO_DEDUCTION), Ok((1000, 0)));

	check_pool(&pool, [(BOOSTER_1, 250), (BOOSTER_2, 250)]);

	assert_eq!(pool.stop_boosting(BOOSTER_1), Ok((250, BTreeSet::from_iter([BOOST_1, BOOST_2]))));

	check_pending_withdrawals(&pool, [(BOOSTER_1, vec![BOOST_1, BOOST_2])]);

	check_pool(&pool, [(BOOSTER_2, 250)]);
	check_pending_boosts(
		&pool,
		[
			(BOOST_1, vec![(BOOSTER_1, 250, 0), (BOOSTER_2, 250, 0)]),
			(BOOST_2, vec![(BOOSTER_1, 500, 0), (BOOSTER_2, 500, 0)]),
		],
	);

	// Deposit of 500 is finalised, BOOSTER 1 gets 250 here, the other 250 goes into
	// Booster 2's available boost amount:
	{
		assert_eq!(
			pool.process_deposit_as_finalised(BOOST_1),
			DepositFinalisationOutcomeForPool {
				amount_credited_to_boosters: 500,
				unlocked_funds: vec![(BOOSTER_1, 250)]
			}
		);

		check_pool(&pool, [(BOOSTER_2, 500)]);
		check_pending_withdrawals(&pool, [(BOOSTER_1, vec![BOOST_2])]);
		check_pending_boosts(&pool, [(BOOST_2, vec![(BOOSTER_1, 500, 0), (BOOSTER_2, 500, 0)])]);
	}

	// The other deposit is lost:
	{
		pool.process_deposit_as_lost(BOOST_2);
		check_pool(&pool, [(BOOSTER_2, 500)]);

		// BOOSTER_1 is no longer withdrawing:
		check_pending_withdrawals(&pool, []);

		check_pending_boosts(&pool, []);
	}
}

#[test]
fn booster_joins_then_funds_lost() {
	let mut pool = TestPool::new(0);
	pool.add_funds(BOOSTER_1, 1000);
	pool.add_funds(BOOSTER_2, 1000);

	assert_eq!(pool.provide_funds_for_boosting(BOOST_1, 500, NO_DEDUCTION), Ok((500, 0)));
	assert_eq!(pool.provide_funds_for_boosting(BOOST_2, 1000, NO_DEDUCTION), Ok((1000, 0)));

	assert_eq!(pool.stop_boosting(BOOSTER_1), Ok((250, BTreeSet::from_iter([BOOST_1, BOOST_2]))));
	check_pool(&pool, [(BOOSTER_2, 250)]);

	// New booster joins while we have a pending withdrawal:
	pool.add_funds(BOOSTER_3, 1000);
	check_pool(&pool, [(BOOSTER_2, 250), (BOOSTER_3, 1000)]);

	// Deposit of 500 is finalised. Importantly this doesn't affect Booster 3 as they
	// didn't participate in the boost:
	assert_eq!(
		pool.process_deposit_as_finalised(BOOST_1),
		DepositFinalisationOutcomeForPool {
			amount_credited_to_boosters: 500,
			unlocked_funds: vec![(BOOSTER_1, 250)]
		}
	);

	check_pool(&pool, [(BOOSTER_2, 500), (BOOSTER_3, 1000)]);

	// The other deposit is lost, which removes the pending withdrawal and
	// inactive amount from the pool. Booster 3 is not affected:
	pool.process_deposit_as_lost(BOOST_2);
	check_pool(&pool, [(BOOSTER_2, 500), (BOOSTER_3, 1000)]);
}

#[test]
fn booster_joins_between_boosts() {
	let mut pool = TestPool::new(200);
	pool.add_funds(BOOSTER_1, 1000);
	pool.add_funds(BOOSTER_2, 1000);

	assert_eq!(pool.provide_funds_for_boosting(BOOST_1, 500, NO_DEDUCTION), Ok((500, 10)));
	check_pool(&pool, [(BOOSTER_1, 755), (BOOSTER_2, 755)]);
	check_pending_boosts(&pool, [(BOOST_1, vec![(BOOSTER_1, 250, 5), (BOOSTER_2, 250, 5)])]);

	assert_eq!(pool.stop_boosting(BOOSTER_1), Ok((755, BTreeSet::from_iter([BOOST_1]))));
	check_pool(&pool, [(BOOSTER_2, 755)]);

	// New booster joins while we have a pending withdrawal:
	pool.add_funds(BOOSTER_3, 2000);
	check_pool(&pool, [(BOOSTER_2, 755), (BOOSTER_3, 2000)]);

	// The amount used for boosting from a given booster is proportional
	// to their share in the available pool:
	assert_eq!(pool.provide_funds_for_boosting(BOOST_2, 1000, NO_DEDUCTION), Ok((1000, 20)));
	check_pool(&pool, [(BOOSTER_2, 486), (BOOSTER_3, 1288)]);
	check_pending_boosts(
		&pool,
		[
			(BOOST_1, vec![(BOOSTER_1, 250, 5), (BOOSTER_2, 250, 5)]),
			(BOOST_2, vec![(BOOSTER_2, 274, 5), (BOOSTER_3, 725, 14)]),
		],
	);

	// Deposit of 500 is finalised, 250 goes to Booster 1's free balance, and the
	// remaining 250 goes to Booster 2; Booster 3 joined after this boost, so they
	// get nothing; there is only one pending boost now (Boost 2):
	assert_eq!(
		pool.process_deposit_as_finalised(BOOST_1),
		DepositFinalisationOutcomeForPool {
			amount_credited_to_boosters: 500,
			unlocked_funds: vec![(BOOSTER_1, 250)]
		},
	);
	check_pool(&pool, [(BOOSTER_2, 736), (BOOSTER_3, 1288)]);
	check_pending_boosts(&pool, [(BOOST_2, vec![(BOOSTER_2, 274, 5), (BOOSTER_3, 725, 14)])]);

	{
		// Scenario A: the second deposit is lost; available amounts remain the same,
		// but there is no more pending boosts:
		let mut pool = pool.clone();
		pool.process_deposit_as_lost(BOOST_2);
		check_pool(&pool, [(BOOSTER_2, 736), (BOOSTER_3, 1288)]);
		check_pending_boosts(&pool, []);
	}

	{
		// Scenario B: the second deposit is received and distributed back between
		// the contributed boosters:
		let mut pool = pool.clone();
		assert_eq!(
			pool.process_deposit_as_finalised(BOOST_2),
			DepositFinalisationOutcomeForPool {
				amount_credited_to_boosters: 1000,
				unlocked_funds: vec![]
			}
		);
		check_pool(&pool, [(BOOSTER_2, 1010), (BOOSTER_3, 2014)]);
		check_pending_boosts(&pool, []);
	}
}

/// Check that boosters with small contributions can boost can earn rewards that
/// can be accumulated to non-zero chain amounts
#[test]
fn small_rewards_accumulate() {
	// Booster 2 only owns a small fraction of the pool:
	let mut pool = TestPool::new(100);
	pool.add_funds(BOOSTER_1, 1000);
	pool.add_funds(BOOSTER_2, 50);

	const SMALL_DEPOSIT: AssetAmount = 500;

	assert_eq!(
		pool.provide_funds_for_boosting(BOOST_1, SMALL_DEPOSIT, NO_DEDUCTION),
		Ok((SMALL_DEPOSIT, 5))
	);
	assert_eq!(
		pool.process_deposit_as_finalised(BOOST_1),
		DepositFinalisationOutcomeForPool {
			amount_credited_to_boosters: SMALL_DEPOSIT,
			unlocked_funds: vec![]
		}
	);

	// BOOSTER 2 earns ~0.25 (it is rounded down when converted to AssetAmount,
	// but the fractional part isn't lost)
	check_pool(&pool, [(BOOSTER_1, 1004), (BOOSTER_2, 50)]);

	// 4 more boost like that and BOOSTER 2 should have withdrawable fees:
	for prewitnessed_deposit_id in 1..=4 {
		assert_eq!(
			pool.provide_funds_for_boosting(prewitnessed_deposit_id, SMALL_DEPOSIT, NO_DEDUCTION),
			Ok((SMALL_DEPOSIT, 5))
		);
		assert_eq!(
			pool.process_deposit_as_finalised(prewitnessed_deposit_id),
			DepositFinalisationOutcomeForPool {
				amount_credited_to_boosters: SMALL_DEPOSIT,
				unlocked_funds: vec![]
			}
		);
	}

	// Note the increase in Booster 2's balance:
	check_pool(&pool, [(BOOSTER_1, 1023), (BOOSTER_2, 51)]);
}

#[test]
fn use_max_available_amount() {
	let mut pool = TestPool::new(100);
	pool.add_funds(BOOSTER_1, 1_000_000);

	// Note that we request more liquidity than is available. This is fine, and
	// expected because the test is from the perspective of a single pool, and
	// finding more funds is another component's responsibility.
	assert_eq!(
		pool.provide_funds_for_boosting(BOOST_1, 2_000_000, NO_DEDUCTION),
		Ok((1_010_101, 10_101))
	);

	check_pool(&pool, [(BOOSTER_1, 0)]);

	assert_eq!(pool.stop_boosting(BOOSTER_1), Ok((0, BTreeSet::from_iter([BOOST_1]))));

	pool.add_funds(BOOSTER_1, 200);

	assert_eq!(
		pool.process_deposit_as_finalised(BOOST_1),
		DepositFinalisationOutcomeForPool {
			amount_credited_to_boosters: 1_010_101,
			unlocked_funds: vec![]
		}
	);

	check_pool(&pool, [(BOOSTER_1, 1_010_301)]);
}

#[test]
fn handling_rounding_errors() {
	type C = Ethereum;
	const FEE_BPS: u16 = 100;
	let mut pool = TestPool::new(100);

	const DEPOSIT_AMOUNT: AssetAmount = 1;
	// A number of boosters that would lead to rounding errors:
	const BOOSTER_COUNT: u32 = 7;
	const BOOSTER_FUNDS: AssetAmount = 1;

	for booster_id in 1..=BOOSTER_COUNT {
		pool.add_funds(booster_id, BOOSTER_FUNDS);
	}

	assert_eq!(
		pool.provide_funds_for_boosting(BOOST_1, DEPOSIT_AMOUNT, NO_DEDUCTION),
		Ok((DEPOSIT_AMOUNT, 0))
	);

	// Note that one of the values is larger than the rest, due to how we handle rounding errors:
	const EXPECTED_REMAINING_AMOUNTS: [u128; 7] = [858, 858, 858, 858, 858, 862, 858];

	assert_eq!(
		&pool.amounts.values().map(|scaled_amount| scaled_amount.val).collect::<Vec<_>>(),
		&EXPECTED_REMAINING_AMOUNTS
	);

	// Despite rounding errors, we the total available amount in the pool is as expected:
	let deposit_amount = ScaledAmount::<C>::from_chain_amount(DEPOSIT_AMOUNT).val;
	{
		let booster_funds = ScaledAmount::<C>::from_chain_amount(BOOSTER_FUNDS).val;
		let fee = deposit_amount * FEE_BPS as u128 / 10_000;
		let expected_total_amount = BOOSTER_COUNT as u128 * booster_funds - deposit_amount + fee;

		assert_eq!(EXPECTED_REMAINING_AMOUNTS.into_iter().sum::<u128>(), expected_total_amount);
	}

	// Again, one of the values is larger than the rest due to rounding errors:
	const EXPECTED_AMOUNTS_TO_RECEIVE: [u128; 7] = [142, 142, 142, 142, 142, 148, 142];

	assert_eq!(
		&pool.pending_boosts[&BOOST_1]
			.values()
			.map(|scaled_amount| scaled_amount.total.val)
			.collect::<Vec<_>>(),
		&EXPECTED_AMOUNTS_TO_RECEIVE
	);

	// Despite rounding errors, the total amount to receive is as expected:
	assert_eq!(EXPECTED_AMOUNTS_TO_RECEIVE.into_iter().sum::<u128>(), deposit_amount);
}
