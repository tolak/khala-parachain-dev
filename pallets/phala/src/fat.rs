/// Public key registry for workers and contracts.
pub use self::pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use codec::Encode;
	use frame_support::{dispatch::DispatchResult, pallet_prelude::*, traits::StorageVersion};
	use frame_system::pallet_prelude::*;
	use sp_core::H256;
	use sp_runtime::traits::Hash;
	use sp_std::prelude::*;

	use crate::{mq::MessageOriginInfo, registry};
	// Re-export
	pub use crate::attestation::{Attestation, IasValidator};

	use phala_types::{
		contract::messaging::{ContractEvent, ContractOperation},
		contract::{CodeIndex, ContractClusterId, ContractId, ContractInfo, DeployTarget},
		messaging::{bind_topic, DecodedMessage, MessageOrigin, WorkerContractReport},
		ContractPublicKey, WorkerIdentity, WorkerPublicKey,
	};

	bind_topic!(ContractRegistryEvent, b"^phala/registry/contract");
	#[derive(Encode, Decode, Clone, Debug)]
	pub enum ContractRegistryEvent {
		PubkeyAvailable {
			contract: ContractId,
			pubkey: ContractPublicKey,
		},
	}

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
	}

	const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	#[pallet::storage_version(STORAGE_VERSION)]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(_);

	/// Mapping from an original code hash to the original code, untouched by instrumentation
	#[pallet::storage]
	pub type Code<T: Config> = StorageMap<_, Twox64Concat, CodeHash<T>, Vec<u8>>;

	/// The contract cluster counter, it always equals to the latest cluster id.
	#[pallet::storage]
	pub type ClusterCounter<T> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub type Clusters<T> =
		StorageMap<_, Twox64Concat, ContractClusterId, Vec<ContractId>, ValueQuery>;

	#[pallet::storage]
	pub type Contracts<T: Config> =
		StorageMap<_, Twox64Concat, ContractId, ContractInfo<CodeHash<T>, T::AccountId>>;

	#[pallet::storage]
	pub type ClusterWorkers<T> =
		StorageMap<_, Twox64Concat, ContractClusterId, Vec<WorkerPublicKey>>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		CodeUploaded {
			hash: CodeHash<T>,
		},
		PubkeyAvailable {
			contract: ContractId,
			pubkey: ContractPublicKey,
		},
		Instantiating {
			contract: ContractId,
			cluster: ContractClusterId,
			deployer: T::AccountId,
		},
		Instantiated {
			contract: ContractId,
			cluster: ContractClusterId,
			deployer: H256,
		},
		InstantiationFailed {
			contract: ContractId,
			cluster: ContractClusterId,
			deployer: H256,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		CodeNotFound,
		ContractClusterNotFound,
		DuplicatedContract,
		DuplicatedDeployment,
		NoWorkerSpecified,
		InvalidSender,
		WorkerNotFound,
	}

	type CodeHash<T> = <T as frame_system::Config>::Hash;

	#[pallet::call]
	impl<T: Config> Pallet<T>
	where
		T: crate::mq::Config + crate::registry::Config,
		T::AccountId: AsRef<[u8]>,
	{
		#[pallet::weight(0)]
		pub fn upload_code(origin: OriginFor<T>, code: Vec<u8>) -> DispatchResult {
			ensure_signed(origin)?;
			let hash = T::Hashing::hash(&code);
			Code::<T>::insert(&hash, &code);
			Self::deposit_event(Event::CodeUploaded { hash });
			Ok(())
		}

		#[pallet::weight(0)]
		pub fn upload_code_to_cluster(
			origin: OriginFor<T>,
			code: Vec<u8>,
			cluster_id: ContractClusterId,
		) -> DispatchResult {
			let origin: T::AccountId = ensure_signed(origin)?;
			// TODO.shelven: check permission?
			Self::push_message(ContractOperation::UploadCodeToCluster {
				origin,
				code,
				cluster_id,
			});
			Ok(())
		}

		#[pallet::weight(0)]
		pub fn instantiate_contract(
			origin: OriginFor<T>,
			code_index: CodeIndex<CodeHash<T>>,
			data: Vec<u8>,
			salt: Vec<u8>,
			deploy_to: DeployTarget,
		) -> DispatchResult {
			let deployer = ensure_signed(origin)?;

			match code_index {
				CodeIndex::NativeCode(_) => {}
				CodeIndex::WasmCode(code_hash) => {
					ensure!(Code::<T>::contains_key(code_hash), Error::<T>::CodeNotFound);
				}
			}

			let mut new_cluster = false;
			let (cluster_id, deploy_workers) = match deploy_to {
				DeployTarget::Cluster(cluster_id) => {
					let workers = ClusterWorkers::<T>::get(cluster_id)
						.ok_or(Error::<T>::ContractClusterNotFound)?;
					(cluster_id, workers)
				}
				DeployTarget::NewGroup(deploy_workers) => {
					ensure!(deploy_workers.len() > 0, Error::<T>::NoWorkerSpecified);

					let counter = ClusterCounter::<T>::mutate(|counter| {
						*counter += 1;
						*counter
					});
					let cluster_id = ContractClusterId::from_low_u64_be(counter);
					new_cluster = true;
					(cluster_id, deploy_workers)
				}
			};

			let mut workers = Vec::new();
			for worker in &deploy_workers {
				let worker_info =
					registry::Workers::<T>::try_get(worker).or(Err(Error::<T>::WorkerNotFound))?;
				workers.push(WorkerIdentity {
					pubkey: worker_info.pubkey,
					ecdh_pubkey: worker_info.ecdh_pubkey,
				});
			}
			if new_cluster {
				ClusterWorkers::<T>::insert(&cluster_id, deploy_workers);
			}

			// We send code index instead of raw code here to reduce message size
			let contract_info = ContractInfo {
				deployer,
				code_index,
				salt,
				cluster_id,
				instantiate_data: data,
			};
			let contract_id = contract_info.contract_id(Box::new(crate::hashing::blake2_256));
			ensure!(
				!Contracts::<T>::contains_key(contract_id),
				Error::<T>::DuplicatedContract
			);
			Contracts::<T>::insert(&contract_id, &contract_info);
			Clusters::<T>::append(cluster_id, contract_id);

			Self::push_message(ContractEvent::instantiate_code(
				contract_info.clone(),
				workers,
			));
			Self::deposit_event(Event::Instantiating {
				contract: contract_id,
				cluster: contract_info.cluster_id,
				deployer: contract_info.deployer,
			});

			Ok(())
		}
	}

	impl<T: Config> Pallet<T>
	where
		T: crate::mq::Config + crate::registry::Config,
	{
		pub fn on_contract_message_received(
			message: DecodedMessage<ContractRegistryEvent>,
		) -> DispatchResult {
			ensure!(
				message.sender == MessageOrigin::Gatekeeper,
				Error::<T>::InvalidSender
			);
			match message.payload {
				ContractRegistryEvent::PubkeyAvailable { contract, pubkey } => {
					registry::ContractKeys::<T>::insert(contract, pubkey);
					Self::deposit_event(Event::PubkeyAvailable { contract, pubkey });
				}
			}
			Ok(())
		}

		pub fn on_worker_contract_message_received(
			message: DecodedMessage<WorkerContractReport>,
		) -> DispatchResult {
			let _worker_pubkey = match &message.sender {
				MessageOrigin::Worker(worker_pubkey) => worker_pubkey,
				_ => return Err(Error::<T>::InvalidSender.into()),
			};
			match message.payload {
				WorkerContractReport::ContractInstantiated {
					id,
					cluster_id,
					deployer,
					pubkey: _,
				} => {
					Self::deposit_event(Event::Instantiated {
						contract: id,
						cluster: cluster_id,
						deployer,
					});
				}
				WorkerContractReport::ContractInstantiationFailed {
					id,
					cluster_id,
					deployer,
				} => {
					Self::deposit_event(Event::InstantiationFailed {
						contract: id,
						cluster: cluster_id,
						deployer,
					});
					// TODO.shelven: some cleanup?
				}
			}
			Ok(())
		}
	}

	impl<T: Config + crate::mq::Config> MessageOriginInfo for Pallet<T> {
		type Config = T;
	}
}
