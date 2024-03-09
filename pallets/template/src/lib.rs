///! # The Lending pallet
///!
///! ## Overview
///!
///! The Lending pallet is responsible for managing the lending pools and the assets.
///!
///! The lending pallet adopts a protocol similar to Compound V2 for its lending operations,
///! leveraging a pool-based approach to aggregate assets from all users.
///!  
///! Interest rates adjust dynamically in response to the supply and demand conditions.
///! Additionally, for every lending positions a new token is minted, thus enabling the transfer of
///! ownership.
///!
///! Implemented Extrinsics:
///!
///! 1. supply
///! 2. withdraw
///! 3. borrow
///! 4. repay
///! 5. claim_rewards
///! 6. add_lending_pool
///! 7. remove_lending_pool
///! 8. activate_lending_pool
///! 9. deactivate_lending_pool
///! 10. update_pool_rate_model
///! 11. update_pool_kink
///!
///! Use case

#![cfg_attr(not(feature = "std"), no_std)]
use frame_support::{
	pallet_prelude::*,
	traits::{fungible, fungibles},
};
pub use pallet::*;

/// Account Type Definition
pub type AccountOf<T> = <T as frame_system::Config>::AccountId;

/// Asset Id
pub type AssetIdOf<T> = <<T as Config>::Fungibles as fungibles::Inspect<AccountOf<T>>>::AssetId;

/// Fungible Balance
pub type AssetBalanceOf<T> =
	<<T as Config>::Fungibles as fungibles::Inspect<AccountOf<T>>>::Balance;

/// Native Balance
pub type BalanceOf<T> = <<T as Config>::NativeBalance as fungible::Inspect<AccountOf<T>>>::Balance;

//pub type BalanceOf<T> = <T as currency::Config>::Balance;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub mod weights;
pub use weights::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{pallet_prelude::DispatchResult, PalletId};
	use frame_system::pallet_prelude::*;
	use frame_support::sp_runtime::traits::AccountIdConversion;
	use frame_support::{
		traits::{
			fungible::{self},
			fungibles::{self},
		}, DefaultNoBound
	};

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// The pallet's config trait.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		#[pallet::constant]
		type PalletId: Get<PalletId>;

		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// Type to access the Balances Pallet.
		type NativeBalance: fungible::Inspect<Self::AccountId>
			+ fungible::Mutate<Self::AccountId>
			+ fungible::hold::Inspect<Self::AccountId>
			+ fungible::hold::Mutate<Self::AccountId>
			+ fungible::freeze::Inspect<Self::AccountId>
			+ fungible::freeze::Mutate<Self::AccountId>;

		/// Type to access the Assets Pallet.
		type Fungibles: fungibles::Inspect<Self::AccountId, Balance = BalanceOf<Self>, AssetId = u32>
			+ fungibles::Mutate<Self::AccountId>
			+ fungibles::Create<Self::AccountId>;

		/// The origin which can add or remove LendingPools and update LendingPools (interest rate
		/// model, kink, activate, deactivate). TODO
		// type ManagerOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;
	}

	/// The AssetPool definition. Used as the Key in the lending pool storage
	#[derive(Clone, Encode, Decode, Eq, PartialEq, RuntimeDebug, MaxEncodedLen, TypeInfo, PartialOrd, DefaultNoBound)]
	#[scale_info(skip_type_params(T))]
	pub struct AssetPool<T: Config> {
		asset: AssetIdOf<T>,
	}

	/// Definition of the Lending Pool Reserve Entity
	/// A struct to hold the LendingPool and all its properties, 
	/// used as Value in the lending pool storage
	/// 
	#[derive(Clone, Encode, Decode, Eq, PartialEq, RuntimeDebug, MaxEncodedLen, TypeInfo, PartialOrd, DefaultNoBound)]
	#[scale_info(skip_type_params(T))]
	pub struct LendingPool<T: Config> {
		pub id: AssetIdOf<T>, // the lending pool id
		pub balance_free: AssetBalanceOf<T>, /* the not-yet-borrowed balance of the lending pool
		                       * minted tokens
		                       * rate model
		                       * kink
		                       *pub balance_locked: AssetBalanceOf<T>, */
	}
	impl<T: Config> LendingPool<T> {
		pub fn from(id: AssetIdOf<T>, balance_free: AssetBalanceOf<T>) -> Self {
			LendingPool { id, balance_free }
		}
	}

	/// PolyLend runtime storage items
	///
	/// Lending pools defined for the assets
	///
	/// StorageMap AssetPool { AssetId } => LendingPool { PoolId, Balance }
	///
	#[pallet::storage]
	#[pallet::getter(fn reserve_pools)]
	pub type ReservePools<T> =
		StorageMap<_, Blake2_128Concat, AssetPool<T>, LendingPool<T>, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		DepositSupplied { who: T::AccountId, balance: BalanceOf<T> },
		DepositWithdrawn { who: T::AccountId, balance: BalanceOf<T> },
		DepositBorrowed { who: T::AccountId, balance: BalanceOf<T> },
		DepositRepaid { who: T::AccountId, balance: BalanceOf<T> },
		RewardsClaimed { who: T::AccountId, balance: BalanceOf<T> },
		LendingPoolAdded { who: T::AccountId },
		LendingPoolRemoved { who: T::AccountId },
		LendingPoolActivated { who: T::AccountId, asset : AssetIdOf<T> },
		LendingPoolDeactivated { who: T::AccountId, asset : AssetIdOf<T> },
		LendingPoolRateModelUpdated { who: T::AccountId, asset : AssetIdOf<T> },
		LendingPoolKinkUpdated { who: T::AccountId, asset : AssetIdOf<T> },
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Lending Pool does not exist
		LendingPoolDoesNotExist,
		/// Lending Pool already exists
		LendingPoolAlreadyExists,
		/// Lending Pool already activated
		LendingPoolAlreadyActivated,
		/// Lending Pool already deactivated
		LendingPoolAlreadyDeactivated,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Create a new Lending pool and then supply some liquidity
		///
		/// The `create_lending_pool` function allows a user to add liquidity to a liquidity pool.
		/// Given two assets and their amounts, it either creates a new liquidity pool if
		/// it does not already exist for these two assets or adds the provided liquidity
		/// to an existing pool. The user will receive LP tokens in return.
		///
		/// # Arguments
		///
		/// * `origin` - The origin caller of this function. This should be signed by the user
		///   that creates the lending pool and add some liquidity.
		/// * `asset` - The identifier for the type of asset that the user wants to provide.
		/// * `asset_b` - The identifier for the second type of asset that the user wants to
		///   provide.
		/// * `amount_a` - The amount of `asset_a` that the user is providing.
		/// * `amount_b` - The amount of `asset_b` that the user is providing.
		///
		/// # Errors
		///
		/// This function will return an error in the following scenarios:
		///
		/// * If the origin is not signed (i.e., the function was not called by a user).
		/// * If the provided assets do not exist.
		/// * If `asset_a` and `asset_b` are the same.
		/// * If `amount_a` or `amount_b` is 0 or less.
		/// * If creating a new liquidity pool would exceed the maximum number of allowed assets
		///   (`AssetLimitReached`).
		/// * If adding liquidity to the pool fails for any reason due to arithmetic overflows or
		///   underflows
		///
		/// # Events
		///
		/// If the function succeeds, it triggers two events:
		///
		/// * `LiquidityPoolCreated(asset_a, asset_b)` if a new liquidity pool was created.
		/// * `LiquidityAdded(asset_a, asset_b, amount_a, amount_b)` after the liquidity has been
		///   successfully added.
		#[pallet::call_index(0)]
		#[pallet::weight(Weight::default())]
		pub fn create_lending_pool(origin: OriginFor<T>, balance: BalanceOf<T>) -> DispatchResult {
			let who = ensure_signed(origin)?;
			Self::do_create_lending_pool(balance)?;
			Self::deposit_event(Event::LendingPoolAdded { who : who.clone() });
			Self::deposit_event(Event::DepositSupplied { balance, who });
			Ok(())
		}

		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::do_something())]
		pub fn activate_lending_pool(
			origin: OriginFor<T>,
			asset : AssetIdOf<T>
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
			Self::deposit_event(Event::LendingPoolActivated { who, asset });

			Ok(())
		}

		#[pallet::call_index(2)]
		#[pallet::weight(T::WeightInfo::do_something())]
		pub fn supply(origin: OriginFor<T>, balance: BalanceOf<T>) -> DispatchResult {
			let who = ensure_signed(origin)?;
			Self::deposit_event(Event::DepositSupplied { balance, who });
			Ok(())
		}

		#[pallet::call_index(3)]
		#[pallet::weight(T::WeightInfo::do_something())]
		pub fn withdraw(origin: OriginFor<T>, balance: BalanceOf<T>) -> DispatchResult {
			let who = ensure_signed(origin)?;
			Self::deposit_event(Event::DepositWithdrawn { who, balance });
			Ok(())
		}

		#[pallet::call_index(4)]
		#[pallet::weight(T::WeightInfo::do_something())]
		pub fn borrow(origin: OriginFor<T>, balance: BalanceOf<T>) -> DispatchResult {
			let who = ensure_signed(origin)?;
			Self::deposit_event(Event::DepositBorrowed { who, balance });
			Ok(())
		}

		#[pallet::call_index(5)]
		#[pallet::weight(T::WeightInfo::do_something())]
		pub fn repay(origin: OriginFor<T>, balance: BalanceOf<T>) -> DispatchResult {
			let who = ensure_signed(origin)?;
			Self::deposit_event(Event::DepositRepaid { who, balance });
			Ok(())
		}

		#[pallet::call_index(6)]
		#[pallet::weight(T::WeightInfo::do_something())]
		pub fn claim_rewards(origin: OriginFor<T>, balance: BalanceOf<T>) -> DispatchResult {
			let who = ensure_signed(origin)?;
			Self::deposit_event(Event::RewardsClaimed { who, balance });
			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		fn do_create_lending_pool(balance: BalanceOf<T>) -> DispatchResult {
			Ok(())
		}

		/// The account ID of the Lending pot.
		///
		/// This actually does computation. If you need to keep using it, then make sure you cache
		/// the value and only call this once.
		pub fn account_id() -> T::AccountId {
			T::PalletId::get().into_account_truncating()
		}
	}
}
