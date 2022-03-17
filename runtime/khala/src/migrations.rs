#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use frame_support::{traits::OnRuntimeUpgrade, PalletId};
use sp_runtime::traits::AccountIdConversion;
use pallet_collective::RawOrigin;

// Note to "late-migration":
//
// All the migrations defined in this file are so called "late-migration". We should have done the
// pallet migrations as soon as we perform the runtime upgrade. However the runtime v1090 was done
// without applying the necessary migrations. Without the migrations, affected pallets can no
// longer access the state db properly.
//
// So here we need to redo the migrations afterward. An immediate problem is that, after the new
// pallets are upgraded, they may have already written some data under the new pallet storage
// prefixes. Most of the pre_upgrade logic checks there's no data under the new pallets as a safe
// guard. However for "late-migrations" this is not the case.
//
// The final decision is to just skip the pre_upgrade checks. We have carefully checked all the
// pre_upgrade checks and confirmed that only the prefix checks are skipped. All the other checks
// are still performed in an offline try-runtime test.

#[cfg(feature = "try-runtime")]
const BRIDGE_ID: PalletId = PalletId(*b"phala/bg");

pub struct AssetRegistryTest;

impl OnRuntimeUpgrade for AssetRegistryTest {
    /// Execute some pre-checks prior to a runtime upgrade.
	///
	/// This hook is never meant to be executed on-chain but is meant to be used by testing tools.
	#[cfg(feature = "try-runtime")]
	fn pre_upgrade() -> Result<(), &'static str> {
		let asset_owner: <super::Runtime as frame_system::Config>::AccountId = BRIDGE_ID.into_account();
		let origin: <super::Runtime as pallet_collective::Config<pallet_collective::Instance1>>::Origin =
			RawOrigin::<<super::Runtime as frame_system::Config>::AccountId , pallet_collective::Instance1>::Members(3, 3).into();

		log::warn!("AssetRegistryTest");
        let result = pallet_assets_wrapper::pallet::Pallet::<super::Runtime>::force_register_asset(
            origin,
            MultiLocation::new(1, X2(Parachain(2001), GeneralKey([0x00, 0x01].to_vec()))).into(),
            2,
            pallet_assets_wrapper::AssetProperties {
                name: b"Bifrost".to_vec(),
                symbol: b"BNC".to_vec(),
                decimals: 12,
            },
            asset_owner.into(),
        );

        log::warn!("Result: {:?}", result);

		Ok(())
	}

	/// Execute some post-checks after a runtime upgrade.
	///
	/// This hook is never meant to be executed on-chain but is meant to be used by testing tools.
	#[cfg(feature = "try-runtime")]
	fn post_upgrade() -> Result<(), &'static str> {
		Ok(())
	}
}