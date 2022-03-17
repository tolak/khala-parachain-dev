#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use frame_support::{traits::OnRuntimeUpgrade, PalletId, weights::GetDispatchInfo};
use sp_runtime::{AccountId32, traits::{AccountIdConversion, BlakeTwo256, Hash}};
use hex_literal::hex;

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
#[cfg(feature = "try-runtime")]
const VOTER1: AccountId32 = AccountId32::new(hex!["c444f21d0057b8afe23049adf1fc902ca5c62f80dc76b03b76f582d6dfeb0d2e"]);	// hang
#[cfg(feature = "try-runtime")]
const VOTER2: AccountId32 = AccountId32::new(hex!["4ce421370cf0257d869618ec25c324ed4c6c7f65289297a3c134332c212e350b"]);	// marvin
#[cfg(feature = "try-runtime")]
const VOTER3: AccountId32 = AccountId32::new(hex!["948ca5f6416d8f39d0f445c3ca17c82002e8789a912d0cb33fe63ab051451f6c"]);	// jonas

#[cfg(feature = "try-runtime")]
type Call = <super::Runtime as frame_system::Config>::Call;
#[cfg(feature = "try-runtime")]
type Collective = pallet_collective::Pallet<super::Runtime, pallet_collective::Instance1>;
#[cfg(feature = "try-runtime")]
type System = frame_system::Pallet<super::Runtime>;

#[cfg(feature = "try-runtime")]
fn make_proposal() -> Call {
	let asset_owner: <super::Runtime as frame_system::Config>::AccountId = BRIDGE_ID.into_account();
	Call::AssetsWrapper(pallet_assets_wrapper::Call::force_register_asset {
		asset: MultiLocation::new(1, X2(Parachain(2001), GeneralKey([0x00, 0x01].to_vec()))).into(),
		asset_id: 2,
		properties: pallet_assets_wrapper::AssetProperties {
			name: b"Bifrost".to_vec(),
			symbol: b"BNC".to_vec(),
			decimals: 12,
		},
		owner: asset_owner.into(),
	})
}

pub struct AssetRegistryTest;

impl OnRuntimeUpgrade for AssetRegistryTest {
    /// Execute some pre-checks prior to a runtime upgrade.
	///
	/// This hook is never meant to be executed on-chain but is meant to be used by testing tools.
	#[cfg(feature = "try-runtime")]
	fn pre_upgrade() -> Result<(), &'static str> {
		// let origin: <super::Runtime as pallet_collective::Config<pallet_collective::Instance1>>::Origin =
		// 	RawOrigin::<<super::Runtime as frame_system::Config>::AccountId , pallet_collective::Instance1>::Members(3, 3).into();

		let proposal = make_proposal();
		let proposal_len: u32 = proposal.using_encoded(|p| p.len() as u32);
		let proposal_weight = proposal.get_dispatch_info().weight;
		let hash = BlakeTwo256::hash_of(&proposal);
		let proposal_index: u32 = 54;

		let proposal_result = Collective::propose(
			Origin::signed(VOTER1),
			3,	// member count
			Box::new(proposal.clone()),
			proposal_len
		);
        log::warn!("proposal_result: {:?}", proposal_result);

		let v1_result = Collective::vote(Origin::signed(VOTER1), hash, proposal_index, true);
		log::warn!("v1_result: {:?}", v1_result);

		let v2_result = Collective::vote(Origin::signed(VOTER2), hash, proposal_index, true);
		log::warn!("v2_result: {:?}", v2_result);

		let v3_result = Collective::vote(Origin::signed(VOTER3), hash, proposal_index, true);
		log::warn!("v3_result: {:?}", v3_result);

		// FIXME: Can't use in features:try-runtime
		// System::set_block_number(1377989);

		let close_result = Collective::close(Origin::signed(VOTER1), hash, proposal_index, proposal_weight, proposal_len);
		log::warn!("close_result: {:?}", close_result);

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