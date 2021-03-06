#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub use self::pallet::*;
#[frame_support::pallet]
pub mod pallet {
	use crate::pallet_assets_wrapper::{
		AccountId32Conversion, ExtractReserveLocation, GetAssetRegistryInfo, XTransferAsset,
		CB_ASSET_KEY,
	};
	use frame_support::{
		pallet_prelude::*,
		traits::{
			tokens::{
				fungibles::{Inspect, Mutate as FungibleMutate, Transfer as FungibleTransfer},
				BalanceConversion, WithdrawReasons,
			},
			Currency, ExistenceRequirement, OnUnbalanced, StorageVersion,
		},
		transactional,
	};

	use crate::bridge;
	use crate::bridge::pallet::BridgeTransact;
	use crate::xcm::xcm_transfer::pallet::XcmTransact;
	use crate::xcm_helper::NativeAssetChecker;
	use frame_system::pallet_prelude::*;
	use sp_arithmetic::traits::SaturatedConversion;
	use sp_core::U256;
	use sp_std::prelude::*;
	use xcm::latest::{
		prelude::*, AssetId as XcmAssetId, Fungibility::Fungible, MultiAsset, MultiLocation,
	};

	type ResourceId = bridge::ResourceId;

	type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
	type NegativeImbalanceOf<T> = <<T as Config>::Currency as Currency<
		<T as frame_system::Config>::AccountId,
	>>::NegativeImbalance;

	const LOG_TARGET: &str = "runtime::bridge-transfer";
	const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	#[pallet::storage_version(STORAGE_VERSION)]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config + bridge::Config + pallet_assets::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		/// Assets register wrapper
		type AssetsWrapper: GetAssetRegistryInfo<<Self as pallet_assets::Config>::AssetId>;

		/// Convert Balance of Currency to AssetId of pallet_assets
		type BalanceConverter: BalanceConversion<
			BalanceOf<Self>,
			<Self as pallet_assets::Config>::AssetId,
			<Self as pallet_assets::Config>::Balance,
		>;

		/// Specifies the origin check provided by the bridge for calls that can only be called by the bridge pallet
		type BridgeOrigin: EnsureOrigin<Self::Origin, Success = Self::AccountId>;

		/// Currency impl
		type Currency: Currency<Self::AccountId>;

		/// XCM transactor
		type XcmTransactor: XcmTransact<Self>;

		/// The handler to absorb the fee.
		type OnFeePay: OnUnbalanced<NegativeImbalanceOf<Self>>;

		/// Check whether an asset is PHA
		type NativeChecker: NativeAssetChecker;

		/// Execution price in PHA
		type NativeExecutionPrice: Get<u128>;

		/// Execution price information
		type ExecutionPriceInfo: Get<Vec<(XcmAssetId, u128)>>;

		/// Treasury account to receive assets fee
		type TreasuryAccount: Get<Self::AccountId>;
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		FeeUpdated {
			dest_id: bridge::BridgeChainId,
			min_fee: BalanceOf<T>,
			fee_scale: u32,
		},
		Deposited {
			asset: XTransferAsset,
			recipient: T::AccountId,
			amount: BalanceOf<T>,
		},
		Forwarded {
			asset: XTransferAsset,
			dest: MultiLocation,
			amount: BalanceOf<T>,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		AssetConversionFailed,
		AssetNotRegistered,
		FeeOptionsMissing,
		CannotPayAsFee,
		InvalidDestination,
		InvalidFeeOption,
		InsufficientBalance,
		BalanceConversionFailed,
		FailedToTransactAsset,
		DestUnrecognized,
		Unimplemented,
		CannotDetermineReservedLocation,
	}

	#[pallet::storage]
	#[pallet::getter(fn bridge_fee)]
	pub type BridgeFee<T: Config> =
		StorageMap<_, Twox64Concat, bridge::BridgeChainId, (BalanceOf<T>, u32), ValueQuery>;

	#[pallet::call]
	impl<T: Config> Pallet<T>
	where
		<T as frame_system::Config>::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
		BalanceOf<T>: Into<u128> + From<u128>,
	{
		/// Change extra bridge transfer fee that user should pay
		#[pallet::weight(195_000_000)]
		pub fn update_fee(
			origin: OriginFor<T>,
			min_fee: BalanceOf<T>,
			fee_scale: u32,
			dest_id: bridge::BridgeChainId,
		) -> DispatchResult {
			T::BridgeCommitteeOrigin::ensure_origin(origin)?;
			ensure!(fee_scale <= 1000u32, Error::<T>::InvalidFeeOption);
			BridgeFee::<T>::insert(dest_id, (min_fee, fee_scale));
			Self::deposit_event(Event::FeeUpdated {
				dest_id,
				min_fee,
				fee_scale,
			});
			Ok(())
		}

		/// Transfer some amount of specific asset to some recipient on a (whitelisted) distination chain.
		#[pallet::weight(195_000_000)]
		#[transactional]
		pub fn transfer_assets(
			origin: OriginFor<T>,
			asset: XTransferAsset,
			dest_id: bridge::BridgeChainId,
			recipient: Vec<u8>,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			let sender = ensure_signed(origin)?;
			let dest_reserve_location: MultiLocation = (
				0,
				X2(
					GeneralKey(CB_ASSET_KEY.to_vec()),
					GeneralIndex(dest_id as u128),
				),
			)
				.into();
			let asset_reserve_location = asset
				.reserve_location()
				.ok_or(Error::<T>::CannotDetermineReservedLocation)?;

			ensure!(
				<bridge::Pallet<T>>::chain_whitelisted(dest_id),
				Error::<T>::InvalidDestination
			);
			ensure!(
				BridgeFee::<T>::contains_key(&dest_id),
				Error::<T>::FeeOptionsMissing
			);

			let asset_id: <T as pallet_assets::Config>::AssetId =
				T::AssetsWrapper::id(&asset).ok_or(Error::<T>::AssetNotRegistered)?;

			let rid: bridge::ResourceId = asset.clone().into_rid(dest_id);
			// Ensure asset is setup for the solo chain
			ensure!(
				Self::rid_to_assetid(&rid).is_ok(),
				Error::<T>::AssetConversionFailed
			);

			let asset_amount = T::BalanceConverter::to_asset_balance(amount, asset_id)
				.map_err(|_| Error::<T>::BalanceConversionFailed)?;
			let reducible_balance = <pallet_assets::pallet::Pallet<T>>::reducible_balance(
				asset_id.into(),
				&sender,
				false,
			);
			ensure!(
				reducible_balance >= asset_amount,
				Error::<T>::InsufficientBalance
			);

			let fee = Self::get_fee(
				dest_id,
				&(Concrete(asset.clone().into()), Fungible(amount.into())).into(),
			)
			.ok_or(Error::<T>::CannotPayAsFee)?;
			// Check asset balance to cover fee
			ensure!(amount > fee.into(), Error::<T>::InsufficientBalance);

			// Transfer asset fee from sender to treasury account
			let fee_amount = T::BalanceConverter::to_asset_balance(fee.into(), asset_id)
				.map_err(|_| Error::<T>::BalanceConversionFailed)?;
			<pallet_assets::pallet::Pallet<T> as FungibleTransfer<T::AccountId>>::transfer(
				asset_id,
				&sender,
				&T::TreasuryAccount::get(),
				fee_amount,
				false,
			)
			.map_err(|_| Error::<T>::FailedToTransactAsset)?;

			let remain_asset = amount - fee.into();
			let asset_amount = T::BalanceConverter::to_asset_balance(remain_asset, asset_id)
				.map_err(|_| Error::<T>::BalanceConversionFailed)?;
			if asset_reserve_location == dest_reserve_location {
				// Burn if transfer back to its reserve location
				pallet_assets::pallet::Pallet::<T>::burn_from(asset_id, &sender, asset_amount)
					.map_err(|_| Error::<T>::FailedToTransactAsset)?;
			} else {
				// Transfer asset from sender to reserve account
				<pallet_assets::pallet::Pallet<T> as FungibleTransfer<T::AccountId>>::transfer(
					asset_id,
					&sender,
					&dest_reserve_location.into_account().into(),
					asset_amount,
					false,
				)
				.map_err(|_| Error::<T>::FailedToTransactAsset)?;
			}

			// Send message to evm chains
			<bridge::Pallet<T>>::transfer_fungible(
				dest_id,
				rid,
				recipient,
				U256::from((remain_asset).saturated_into::<u128>()),
			)
		}

		/// Transfers some amount of the native token to some recipient on a (whitelisted) destination chain.
		#[pallet::weight(195_000_000)]
		#[transactional]
		pub fn transfer_native(
			origin: OriginFor<T>,
			amount: BalanceOf<T>,
			recipient: Vec<u8>,
			dest_id: bridge::BridgeChainId,
		) -> DispatchResult {
			let sender = ensure_signed(origin)?;
			let reserve_id = <bridge::Pallet<T>>::account_id();
			ensure!(
				<bridge::Pallet<T>>::chain_whitelisted(dest_id),
				Error::<T>::InvalidDestination
			);
			ensure!(
				BridgeFee::<T>::contains_key(&dest_id),
				Error::<T>::FeeOptionsMissing
			);
			let fee = Self::estimate_fee_in_pha(dest_id, amount);
			let free_balance = <T as Config>::Currency::free_balance(&sender);
			ensure!(
				free_balance >= (amount + fee),
				Error::<T>::InsufficientBalance
			);

			let imbalance = <T as Config>::Currency::withdraw(
				&sender,
				fee,
				WithdrawReasons::FEE,
				ExistenceRequirement::AllowDeath,
			)?;
			T::OnFeePay::on_unbalanced(imbalance);
			<T as Config>::Currency::transfer(
				&sender,
				&reserve_id,
				amount,
				ExistenceRequirement::AllowDeath,
			)?;

			<bridge::Pallet<T>>::transfer_fungible(
				dest_id,
				Self::gen_pha_rid(dest_id),
				recipient,
				U256::from(amount.saturated_into::<u128>()),
			)
		}

		//
		// Executable calls. These can be triggered by a bridge transfer initiated on another chain
		//

		/// Executes a simple currency transfer using the bridge account as the source
		#[pallet::weight(195_000_000)]
		pub fn transfer(
			origin: OriginFor<T>,
			dest: Vec<u8>,
			amount: BalanceOf<T>,
			rid: ResourceId,
		) -> DispatchResult {
			let bridge_account = T::BridgeOrigin::ensure_origin(origin.clone())?;
			// For solo chain assets, we encode solo chain id as the first byte of resourceId
			let src_chainid: bridge::BridgeChainId = Self::get_chainid(&rid);
			let src_reserve_location: MultiLocation = (
				0,
				X2(
					GeneralKey(CB_ASSET_KEY.to_vec()),
					GeneralIndex(src_chainid as u128),
				),
			)
				.into();

			let dest_location: MultiLocation =
				Decode::decode(&mut dest.as_slice()).map_err(|_| Error::<T>::DestUnrecognized)?;

			let asset_location = Self::rid_to_location(&rid)?;
			let asset_reserve_location = asset_location
				.reserve_location()
				.ok_or(Error::<T>::CannotDetermineReservedLocation)?;

			log::trace!(
				target: LOG_TARGET,
				"Reserve location of assset ${:?}, reserve location of source: {:?}.",
				&asset_reserve_location,
				&src_reserve_location,
			);

			// We received asset send from non-reserve chain, which reserved
			// in the local our other parachains/relaychain. That means we had
			// reserved the asset in a reserve account while it was transfered
			// the the source chain, so here we need withdraw/burn from the reserve
			// account in advance.
			//
			// Note: If we received asset send from its reserve chain, we just need
			// mint the same amount of asset at local
			if asset_reserve_location != src_reserve_location {
				if rid == Self::gen_pha_rid(src_chainid) {
					// ERC20 PHA save reserved assets in bridge account
					let _imbalance = <T as Config>::Currency::withdraw(
						&bridge_account,
						amount,
						WithdrawReasons::TRANSFER,
						ExistenceRequirement::AllowDeath,
					)?;
				} else {
					let asset_id = Self::rid_to_assetid(&rid)?;
					let asset_amount = T::BalanceConverter::to_asset_balance(amount, asset_id)
						.map_err(|_| Error::<T>::BalanceConversionFailed)?;

					// burn from source reserve account
					pallet_assets::pallet::Pallet::<T>::burn_from(
						asset_id,
						&src_reserve_location.into_account().into(),
						asset_amount,
					)
					.map_err(|_| Error::<T>::FailedToTransactAsset)?;
				};
				log::trace!(
					target: LOG_TARGET,
					"Reserve of asset and src dismatch, burn asset form source reserve location.",
				);
			}

			// The asset already being "mint" or "withdrawn from reserve account", now settle to dest
			match (dest_location.parents, &dest_location.interior) {
				// To local account
				(0, &X1(AccountId32 { network: _, id })) => {
					if rid == Self::gen_pha_rid(src_chainid) {
						// ERC20 PHA transfer
						<T as Config>::Currency::deposit_creating(&id.into(), amount);
					} else {
						let asset_id = Self::rid_to_assetid(&rid)?;
						let asset_amount = T::BalanceConverter::to_asset_balance(amount, asset_id)
							.map_err(|_| Error::<T>::BalanceConversionFailed)?;

						// Mint asset into recipient
						pallet_assets::pallet::Pallet::<T>::mint_into(
							asset_id,
							&id.into(),
							asset_amount,
						)
						.map_err(|_| Error::<T>::FailedToTransactAsset)?;
					}
					Self::deposit_event(Event::Deposited {
						asset: asset_location.into(),
						recipient: id.into(),
						amount,
					});
				}
				// To relaychain or other parachain, forward it by xcm
				(1, X1(AccountId32 { .. })) | (1, X2(Parachain(_), AccountId32 { .. })) => {
					let temporary_account =
						MultiLocation::new(0, X1(GeneralKey(b"bridge_transfer".to_vec())))
							.into_account();
					log::trace!(
						target: LOG_TARGET,
						"Deposit withdrawn asset to a temporary account: {:?}",
						&temporary_account,
					);
					if rid == Self::gen_pha_rid(src_chainid) {
						<T as Config>::Currency::deposit_creating(
							&temporary_account.clone().into(),
							amount,
						);
					} else {
						let asset_id = Self::rid_to_assetid(&rid)?;
						let asset_amount = T::BalanceConverter::to_asset_balance(amount, asset_id)
							.map_err(|_| Error::<T>::BalanceConversionFailed)?;
						// Mint asset into dest temporary account
						pallet_assets::pallet::Pallet::<T>::mint_into(
							asset_id,
							&temporary_account.clone().into(),
							asset_amount,
						)
						.map_err(|_| Error::<T>::FailedToTransactAsset)?;
					}

					// After deposited asset into the temporary account, let xcm executor determine how to
					// handle the asset.
					T::XcmTransactor::transfer_fungible(
						Junction::AccountId32 {
							network: NetworkId::Any,
							id: temporary_account,
						}
						.into(),
						(asset_location.clone(), amount.into()).into(),
						dest_location.clone(),
						6000000000u64.into(),
					)?;
					Self::deposit_event(Event::Forwarded {
						asset: asset_location.into(),
						// dest_location already contains recipient account
						dest: dest_location,
						amount,
					});
				}
				// To other evm chains
				(
					0,
					X3(GeneralKey(_cb_key), GeneralIndex(_evm_chain_id), GeneralKey(_evm_account)),
				) => {
					// TODO
					return Err(Error::<T>::DestUnrecognized.into());
				}
				_ => return Err(Error::<T>::DestUnrecognized.into()),
			}
			Ok(())
		}
	}

	impl<T: Config> Pallet<T>
	where
		BalanceOf<T>: From<u128> + Into<u128>,
	{
		// TODO.wf: A more proper way to estimate fee
		pub fn estimate_fee_in_pha(
			dest_id: bridge::BridgeChainId,
			amount: BalanceOf<T>,
		) -> BalanceOf<T> {
			let (min_fee, fee_scale) = Self::bridge_fee(dest_id);
			let fee_estimated = amount * fee_scale.into() / 1000u32.into();
			if fee_estimated > min_fee {
				fee_estimated
			} else {
				min_fee
			}
		}

		pub fn to_e12(amount: u128, decimals: u8) -> u128 {
			if decimals > 12 {
				amount.saturating_div(10u128.saturating_pow(decimals as u32 - 12))
			} else {
				amount.saturating_mul(10u128.saturating_pow(12 - decimals as u32))
			}
		}

		pub fn from_e12(amount: u128, decimals: u8) -> u128 {
			if decimals > 12 {
				amount.saturating_mul(10u128.saturating_pow(decimals as u32 - 12))
			} else {
				amount.saturating_div(10u128.saturating_pow(12 - decimals as u32))
			}
		}

		pub fn convert_fee_from_pha(fee_in_pha: BalanceOf<T>, price: u128, decimals: u8) -> u128 {
			let fee_e12: u128 = fee_in_pha.into() * price / T::NativeExecutionPrice::get();
			Self::from_e12(fee_e12.into(), decimals)
		}

		pub fn rid_to_location(rid: &[u8; 32]) -> Result<MultiLocation, DispatchError> {
			let src_chainid: bridge::BridgeChainId = Self::get_chainid(rid);
			let asset_location: MultiLocation = if *rid == Self::gen_pha_rid(src_chainid) {
				MultiLocation::here()
			} else {
				let xtransfer_asset: XTransferAsset = T::AssetsWrapper::lookup_by_resource_id(&rid)
					.ok_or(Error::<T>::AssetConversionFailed)?;
				xtransfer_asset.into()
			};
			Ok(asset_location)
		}

		pub fn rid_to_assetid(
			rid: &[u8; 32],
		) -> Result<<T as pallet_assets::Config>::AssetId, DispatchError> {
			let src_chainid: bridge::BridgeChainId = Self::get_chainid(rid);
			// PHA based on pallet_balances, not pallet_assets
			if *rid == Self::gen_pha_rid(src_chainid) {
				return Err(Error::<T>::AssetNotRegistered.into());
			}
			let xtransfer_asset: XTransferAsset = T::AssetsWrapper::lookup_by_resource_id(&rid)
				.ok_or(Error::<T>::AssetConversionFailed)?;
			let asset_id: <T as pallet_assets::Config>::AssetId =
				T::AssetsWrapper::id(&xtransfer_asset).ok_or(Error::<T>::AssetNotRegistered)?;
			Ok(asset_id)
		}

		pub fn gen_pha_rid(chain_id: bridge::BridgeChainId) -> bridge::ResourceId {
			XTransferAsset(MultiLocation::here()).into_rid(chain_id)
		}

		pub fn get_chainid(rid: &bridge::ResourceId) -> bridge::BridgeChainId {
			rid[0]
		}
	}

	pub trait GetBridgeFee {
		fn get_fee(chain_id: bridge::BridgeChainId, asset: &MultiAsset) -> Option<u128>;
	}
	impl<T: Config> GetBridgeFee for Pallet<T>
	where
		BalanceOf<T>: From<u128> + Into<u128>,
	{
		fn get_fee(chain_id: bridge::BridgeChainId, asset: &MultiAsset) -> Option<u128> {
			match (&asset.id, &asset.fun) {
				(Concrete(location), Fungible(amount)) => {
					let id = T::AssetsWrapper::id(&XTransferAsset(location.clone()))?;
					let decimals = T::AssetsWrapper::decimals(&id).unwrap_or(12);
					let fee_in_pha = Self::estimate_fee_in_pha(
						chain_id,
						(Self::to_e12(*amount, decimals)).into(),
					);
					if T::NativeChecker::is_native_asset(asset) {
						Some(fee_in_pha.into())
					} else {
						let fee_prices = T::ExecutionPriceInfo::get();
						let fee_in_asset = fee_prices
							.iter()
							.position(|(fee_asset_id, _)| {
								fee_asset_id == &Concrete(location.clone())
							})
							.map(|idx| {
								Self::convert_fee_from_pha(fee_in_pha, fee_prices[idx].1, decimals)
							});
						fee_in_asset
					}
				}
				_ => None,
			}
		}
	}
}
