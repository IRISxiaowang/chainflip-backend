use cf_amm::math::price_at_tick;
use cf_primitives::{AccountRole, Asset, AuthorityCount, Tick, FLIPPERINOS_PER_FLIP};
use cf_traits::IncreaseOrDecrease;
use codec::Encode;
use frame_support::pallet_prelude::TransactionValidityError;
// use pallet_cf_flip::{FeeScalingRate, FeeScalingRateConfig};
use pallet_cf_pools::{AssetAmounts, CloseOrder, RangeOrderSize};
use sp_block_builder::runtime_decl_for_block_builder::BlockBuilderV6;
use sp_keyring::test::AccountKeyring;
use sp_core::{ConstU32, Pair};
use sp_runtime::{generic::Era, BoundedVec, MultiSignature};
use state_chain_runtime::{Balance, Flip, Runtime, RuntimeCall, SignedPayload, System};
use cf_amm::common::Side;

use crate::{network::register_refund_addresses, swapping::{credit_account, new_pool}};

const POSITION: core::ops::Range<Tick> = -100_000..100_000;

pub fn apply_extrinsic_and_calculate_gas_fee(
    caller: AccountKeyring,
    call: RuntimeCall,
) -> Result<(Balance, Balance), TransactionValidityError> {
    let caller_account_id = caller.to_account_id();
    let before = Flip::total_balance_of(&caller_account_id);

    let extra = (
        frame_system::CheckNonZeroSender::<Runtime>::new(),
        frame_system::CheckSpecVersion::<Runtime>::new(),
        frame_system::CheckTxVersion::<Runtime>::new(),
        frame_system::CheckGenesis::<Runtime>::new(),
        frame_system::CheckEra::<Runtime>::from(Era::Immortal),
        frame_system::CheckNonce::<Runtime>::from(System::account_nonce(&caller_account_id)),
        frame_system::CheckWeight::<Runtime>::new(),
        pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(0u128),
        frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(false),
    );

    let signed_payload = SignedPayload::new(call.clone(), extra.clone()).unwrap();
    let signature = MultiSignature::Ed25519(caller.sign(&signed_payload.encode()));
    let ext = sp_runtime::generic::UncheckedExtrinsic::new_signed(
        call,
        caller_account_id.clone().into(),
        signature,
        extra,
    );

    let _ = Runtime::apply_extrinsic(ext)?;

    let after = Flip::total_balance_of(&caller_account_id);

    Ok((before - after, after))
}



#[test]
fn basic_pool_setup_provision_and_swap() {
    let alice = AccountKeyring::Alice;
    let alice_id = alice.to_account_id();
	super::genesis::with_test_defaults()
		.with_additional_accounts(&[
			(alice_id.clone(), AccountRole::LiquidityProvider, 5 * FLIPPERINOS_PER_FLIP),
			// (ZION, AccountRole::Broker, 5 * FLIPPERINOS_PER_FLIP),
		])
		.build()
		.execute_with(|| {
			new_pool(Asset::Eth, 0, price_at_tick(0).unwrap());
			register_refund_addresses(&alice_id);

			// Use the same decimals amount for all assets.
			const DECIMALS: u128 = 10u128.pow(18);
			credit_account(&alice_id, Asset::Eth, 10_000_000 * DECIMALS);
			credit_account(&alice_id, Asset::Usdc, 10_000_000 * DECIMALS);

            for i in 0..8 {

                let call = RuntimeCall::LiquidityPools(pallet_cf_pools::Call::<Runtime>::set_limit_order {
                    base_asset: Asset::Eth,
                    quote_asset: Asset::Usdc,
                    side: Side::Sell,
                    id: i as u64,
                    option_tick: Some(0),
                    sell_amount: 1_000 * DECIMALS,
                });
                
                let res = apply_extrinsic_and_calculate_gas_fee(alice, call);
    
                println!("The result is {:?}", res);

            }
            
		});
}

#[test]
fn test_four_order_calls_can_scaling_fee() {
    let alice = AccountKeyring::Alice;
    let alice_id = alice.to_account_id();
	super::genesis::with_test_defaults()
		.with_additional_accounts(&[
			(alice_id.clone(), AccountRole::LiquidityProvider, 5 * FLIPPERINOS_PER_FLIP),
		])
		.build()
		.execute_with(|| {
			new_pool(Asset::Eth, 0, price_at_tick(0).unwrap());
			register_refund_addresses(&alice_id);

			// Use the same decimals amount for all assets.
			const DECIMALS: u128 = 10u128.pow(18);
			credit_account(&alice_id, Asset::Eth, 10_000_000 * DECIMALS);
			credit_account(&alice_id, Asset::Usdc, 10_000_000 * DECIMALS);

            for i in 0..2 {

                let call = RuntimeCall::LiquidityPools(pallet_cf_pools::Call::<Runtime>::set_limit_order {
                    base_asset: Asset::Eth,
                    quote_asset: Asset::Usdc,
                    side: Side::Sell,
                    id: i as u64,
                    option_tick: Some(0),
                    sell_amount: 1_000 * DECIMALS,
                });
                
                let res = apply_extrinsic_and_calculate_gas_fee(alice, call);
    
                println!("The result is {:?}", res);

            }
            
            for i in 2..4 {

                let call = RuntimeCall::LiquidityPools(pallet_cf_pools::Call::<Runtime>::update_limit_order {
                    base_asset: Asset::Eth,
                    quote_asset: Asset::Usdc,
                    side: Side::Sell,
                    id: i as u64,
                    option_tick: Some(0),
                    amount_change: cf_traits::IncreaseOrDecrease::Increase(100 * DECIMALS),
                });
                
                let res = apply_extrinsic_and_calculate_gas_fee(alice, call);
    
                println!("The result is {:?}", res);

            }

            for i in 4..6 {

                let call = RuntimeCall::LiquidityPools(pallet_cf_pools::Call::<Runtime>::set_range_order {
                    base_asset: Asset::Eth,
                    quote_asset: Asset::Usdc,
                    id: i as u64,
                    option_tick_range: Some(POSITION),
                    size: 
                    RangeOrderSize::AssetAmounts {
                        maximum: AssetAmounts { base: 1_000_000, quote: 1_000_000 },
                        minimum: AssetAmounts { base: 900_000, quote: 900_000 },
                    },
                });
                
                let res = apply_extrinsic_and_calculate_gas_fee(alice, call);
    
                println!("The result is {:?}", res);

            }
            
            for i in 6..8 {

                let call = RuntimeCall::LiquidityPools(pallet_cf_pools::Call::<Runtime>::update_range_order {
                    base_asset: Asset::Eth,
                    quote_asset: Asset::Usdc,
                    id: i as u64,
                    option_tick_range: Some(POSITION),
                    size_change: cf_traits::IncreaseOrDecrease::Increase(pallet_cf_pools::RangeOrderSize::Liquidity { liquidity: 100 * DECIMALS }),
                });
                
                let res = apply_extrinsic_and_calculate_gas_fee(alice, call);
    
                println!("The result is {:?}", res);

            }
		});
}

#[test]
fn other_call_can_not_scaling_fee() {
    let alice = AccountKeyring::Alice;
    let alice_id = alice.to_account_id();
	super::genesis::with_test_defaults()
		.with_additional_accounts(&[
			(alice_id.clone(), AccountRole::LiquidityProvider, 5 * FLIPPERINOS_PER_FLIP),
		])
		.build()
		.execute_with(|| {
			new_pool(Asset::Eth, 0, price_at_tick(0).unwrap());
			register_refund_addresses(&alice_id);

			// Use the same decimals amount for all assets.
			const DECIMALS: u128 = 10u128.pow(18);
			credit_account(&alice_id, Asset::Eth, 10_000_000 * DECIMALS);
			credit_account(&alice_id, Asset::Usdc, 10_000_000 * DECIMALS);

            let orders_to_delete: BoundedVec<CloseOrder, ConstU32<100>> = BoundedVec::new();
            for _ in 0..8 {

                let call = RuntimeCall::LiquidityPools(pallet_cf_pools::Call::<Runtime>::cancel_orders_batch {
                    orders: orders_to_delete.clone(),
                });
                
                let res = apply_extrinsic_and_calculate_gas_fee(alice, call);
    
                println!("The result is {:?}", res);

            }
            
		});
}