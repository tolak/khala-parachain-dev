#![cfg(test)]

use frame_support::{
	ord_parameter_types, parameter_types,
	traits::{ConstU128, ConstU32, GenesisBuild},
	weights::Weight,
	PalletId,
};
use frame_system::{self as system};
use sp_core::H256;
use sp_runtime::{
	testing::Header,
	traits::{AccountIdConversion, BlakeTwo256, ConvertInto, IdentityLookup},
	AccountId32, Perbill,
};

use crate::bridge_transfer;
use crate::pallet_assets_wrapper;
use crate::pallet_bridge as bridge;
use crate::xcm_helper::NativeAssetFilter;
pub use pallet_balances as balances;
pub use xcm::latest::{prelude::*, AssetId, MultiAsset, MultiLocation};

type UncheckedExtrinsic = frame_system::mocking::MockUncheckedExtrinsic<Test>;
type Block = frame_system::mocking::MockBlock<Test>;

pub(crate) type Balance = u128;

frame_support::construct_runtime!(
	pub enum Test where
		Block = Block,
		NodeBlock = Block,
		UncheckedExtrinsic = UncheckedExtrinsic
	{
		System: frame_system::{Pallet, Call, Config, Storage, Event<T>},
		Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
		Bridge: bridge::{Pallet, Call, Storage, Event<T>},
		BridgeTransfer: bridge_transfer::{Pallet, Call, Storage, Event<T>},
		Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
		Assets: pallet_assets::{Pallet, Call, Storage, Event<T>},
		AssetsWrapper: pallet_assets_wrapper::{Pallet, Call, Storage, Event<T>},
		ParachainInfo: pallet_parachain_info::{Pallet, Storage, Config},
	}
);

parameter_types! {
	pub const BlockHashCount: u64 = 250;
	pub const MaximumBlockWeight: Weight = 1024;
	pub const MaximumBlockLength: u32 = 2 * 1024;
	pub const AvailableBlockRatio: Perbill = Perbill::one();
	pub const MaxLocks: u32 = 100;
	pub const MinimumPeriod: u64 = 1;
}

impl frame_system::Config for Test {
	type BaseCallFilter = frame_support::traits::Everything;
	type Origin = Origin;
	type Call = Call;
	type Index = u64;
	type BlockNumber = u64;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = AccountId32;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Header = Header;
	type Event = Event;
	type BlockHashCount = BlockHashCount;
	type DbWeight = ();
	type Version = ();
	type AccountData = pallet_balances::AccountData<Balance>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type PalletInfo = PalletInfo;
	type BlockWeights = ();
	type BlockLength = ();
	type SS58Prefix = ();
	type OnSetCode = ();
	type MaxConsumers = ConstU32<2>;
}

parameter_types! {
	pub const ExistentialDeposit: Balance = 1;
}

ord_parameter_types! {
	pub const One: u64 = 1;
}

impl pallet_balances::Config for Test {
	type Balance = Balance;
	type DustRemoval = ();
	type Event = Event;
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type WeightInfo = ();
	type MaxLocks = ();
	type MaxReserves = ();
	type ReserveIdentifier = [u8; 8];
}

parameter_types! {
	pub const TestChainId: u8 = 5;
	pub const ProposalLifetime: u64 = 100;

	// We define two test assets to simulate tranfer assets to reserve location and unreserve location,
	// we must defiend here because those need be configed as fee payment assets
	pub SoloChain0AssetLocation: MultiLocation = MultiLocation::new(
		1,
		X4(
			Parachain(2004),
			GeneralKey(pallet_assets_wrapper::CB_ASSET_KEY.to_vec()),
			GeneralIndex(0),
			GeneralKey(b"an asset".to_vec()),
		),
	);
	pub SoloChain2AssetLocation: MultiLocation = MultiLocation::new(
		1,
		X4(
			Parachain(2004),
			GeneralKey(pallet_assets_wrapper::CB_ASSET_KEY.to_vec()),
			GeneralIndex(2),
			GeneralKey(b"an asset".to_vec()),
		),
	);
	pub AssetId0: AssetId = SoloChain0AssetLocation::get().into();
	pub AssetId2: AssetId = SoloChain2AssetLocation::get().into();
	pub ExecutionPriceInAsset0: (AssetId, u128) = (
		AssetId0::get(),
		1
	);
	pub ExecutionPriceInAsset2: (AssetId, u128) = (
		AssetId2::get(),
		2
	);
	pub NativeExecutionPrice: u128 = 1;
	pub ExecutionPrices: Vec<(AssetId, u128)> = [
		ExecutionPriceInAsset0::get(),
		ExecutionPriceInAsset2::get(),
	].to_vec().into();
	pub TREASURY: AccountId32 = AccountId32::new([4u8; 32]);
}

impl bridge::Config for Test {
	type Event = Event;
	type BridgeCommitteeOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type Proposal = Call;
	type BridgeChainId = TestChainId;
	type ProposalLifetime = ProposalLifetime;
}

impl bridge_transfer::Config for Test {
	type Event = Event;
	type AssetsWrapper = AssetsWrapper;
	type BalanceConverter = pallet_assets::BalanceToAssetBalance<Balances, Test, ConvertInto>;
	type BridgeOrigin = bridge::EnsureBridge<Test>;
	type Currency = Balances;
	type XcmTransactor = ();
	type OnFeePay = ();
	type NativeChecker = NativeAssetFilter<ParachainInfo>;
	type NativeExecutionPrice = NativeExecutionPrice;
	type ExecutionPriceInfo = ExecutionPrices;
	type TreasuryAccount = TREASURY;
}

parameter_types! {
	pub const AssetDeposit: Balance = 1; // 1 Unit deposit to create asset
	pub const ApprovalDeposit: Balance = 1;
	pub const AssetsStringLimit: u32 = 50;
	pub const MetadataDepositBase: Balance = 1;
	pub const MetadataDepositPerByte: Balance = 1;
}

impl pallet_assets::Config for Test {
	type Event = Event;
	type Balance = Balance;
	type AssetId = u32;
	type Currency = Balances;
	type ForceOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type AssetDeposit = AssetDeposit;
	type AssetAccountDeposit = ConstU128<10>;
	type MetadataDepositBase = MetadataDepositBase;
	type MetadataDepositPerByte = MetadataDepositPerByte;
	type ApprovalDeposit = ApprovalDeposit;
	type StringLimit = AssetsStringLimit;
	type Freezer = ();
	type Extra = ();
	type WeightInfo = ();
}

impl pallet_assets_wrapper::Config for Test {
	type Event = Event;
	type AssetsCommitteeOrigin = frame_system::EnsureRoot<Self::AccountId>;
	type MinBalance = ExistentialDeposit;
}

impl pallet_timestamp::Config for Test {
	type Moment = u64;
	type OnTimestampSet = ();
	type MinimumPeriod = MinimumPeriod;
	type WeightInfo = ();
}

impl pallet_parachain_info::Config for Test {}

pub const ALICE: AccountId32 = AccountId32::new([0u8; 32]);
pub const RELAYER_A: AccountId32 = AccountId32::new([1u8; 32]);
pub const RELAYER_B: AccountId32 = AccountId32::new([2u8; 32]);
pub const RELAYER_C: AccountId32 = AccountId32::new([3u8; 32]);
pub const ENDOWED_BALANCE: Balance = 100_000_000;

pub fn new_test_ext() -> sp_io::TestExternalities {
	let bridge_account = PalletId(*b"phala/bg").into_account();
	let mut t = frame_system::GenesisConfig::default()
		.build_storage::<Test>()
		.unwrap();
	let parachain_info_config = pallet_parachain_info::GenesisConfig {
		parachain_id: 2004u32.into(),
	};
	<pallet_parachain_info::GenesisConfig as GenesisBuild<Test, _>>::assimilate_storage(
		&parachain_info_config,
		&mut t,
	)
	.unwrap();
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![
			(bridge_account, ENDOWED_BALANCE),
			(RELAYER_A, ENDOWED_BALANCE),
			(ALICE, ENDOWED_BALANCE),
		],
	}
	.assimilate_storage(&mut t)
	.unwrap();
	let mut ext = sp_io::TestExternalities::new(t);
	ext.execute_with(|| System::set_block_number(1));
	ext
}

fn last_event() -> Event {
	system::Pallet::<Test>::events()
		.pop()
		.map(|e| e.event)
		.expect("Event expected")
}

pub fn expect_event<E: Into<Event>>(e: E) {
	assert_eq!(last_event(), e.into());
}

// Checks events against the latest. A contiguous set of events must be provided. They must
// include the most recent event, but do not have to include every past event.
pub fn assert_events(mut expected: Vec<Event>) {
	let mut actual: Vec<Event> = system::Pallet::<Test>::events()
		.iter()
		.map(|e| e.event.clone())
		.collect();

	expected.reverse();

	for evt in expected {
		let next = actual.pop().expect("event expected");
		assert_eq!(next, evt.into(), "Events don't match");
	}
}
