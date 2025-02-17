use super::*;
use frame_support::pallet_prelude::{Decode, Encode};
use frame_support::storage::IterableStorageMap;
use frame_support::IterableStorageDoubleMap;
use safe_math::*;
use substrate_fixed::types::U64F64;
extern crate alloc;
use codec::Compact;

#[freeze_struct("66105c2cfec0608d")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct DelegateInfo<AccountId: TypeInfo + Encode + Decode> {
    delegate_ss58: AccountId,
    take: Compact<u16>,
    nominators: Vec<(AccountId, Compact<u64>)>, // map of nominator_ss58 to stake amount
    owner_ss58: AccountId,
    registrations: Vec<Compact<u16>>, // Vec of netuid this delegate is registered on
    validator_permits: Vec<Compact<u16>>, // Vec of netuid this delegate has validator permit on
    return_per_1000: Compact<u64>, // Delegators current daily return per 1000 TAO staked minus take fee
    total_daily_return: Compact<u64>, // Delegators current daily return
}

impl<T: Config> Pallet<T> {
    fn return_per_1000_tao(
        take: Compact<u16>,
        total_stake: U64F64,
        emissions_per_day: U64F64,
    ) -> U64F64 {
        // Get the take as a percentage and subtract it from 1 for remainder.
        let without_take: U64F64 = U64F64::saturating_from_num(1)
            .saturating_sub(U64F64::saturating_from_num(take.0).safe_div(u16::MAX.into()));

        if total_stake > U64F64::saturating_from_num(0) {
            emissions_per_day
                .saturating_mul(without_take)
                // Divide by 1000 TAO for return per 1k
                .safe_div(total_stake.safe_div(U64F64::saturating_from_num(1000.0 * 1e9)))
        } else {
            U64F64::saturating_from_num(0)
        }
    }

    #[cfg(test)]
    pub fn return_per_1000_tao_test(
        take: Compact<u16>,
        total_stake: U64F64,
        emissions_per_day: U64F64,
    ) -> U64F64 {
        Self::return_per_1000_tao(take, total_stake, emissions_per_day)
    }

    fn get_delegate_by_existing_account(delegate: AccountIdOf<T>) -> DelegateInfo<T::AccountId> {
        let mut nominators = Vec::<(T::AccountId, Compact<u64>)>::new();

        for (nominator, stake) in
            <Stake<T> as IterableStorageDoubleMap<T::AccountId, T::AccountId, u64>>::iter_prefix(
                delegate.clone(),
            )
        {
            if stake == 0 {
                continue;
            }
            // Only add nominators with stake
            nominators.push((nominator.clone(), stake.into()));
        }

        let registrations = Self::get_registered_networks_for_hotkey(&delegate.clone());
        let mut validator_permits = Vec::<Compact<u16>>::new();
        let mut emissions_per_day: U64F64 = U64F64::saturating_from_num(0);

        for netuid in registrations.iter() {
            if let Ok(uid) = Self::get_uid_for_net_and_hotkey(*netuid, &delegate.clone()) {
                let validator_permit = Self::get_validator_permit_for_uid(*netuid, uid);
                if validator_permit {
                    validator_permits.push((*netuid).into());
                }

                let emission: U64F64 = Self::get_emission_for_uid(*netuid, uid).into();
                let tempo: U64F64 = Self::get_tempo(*netuid).into();
                if tempo > U64F64::saturating_from_num(0) {
                    let epochs_per_day: U64F64 = U64F64::saturating_from_num(7200).safe_div(tempo);
                    emissions_per_day =
                        emissions_per_day.saturating_add(emission.saturating_mul(epochs_per_day));
                }
            }
        }

        let owner = Self::get_owning_coldkey_for_hotkey(&delegate.clone());
        let take: Compact<u16> = <Delegates<T>>::get(delegate.clone()).into();

        let total_stake: U64F64 =
            Self::get_stake_for_hotkey_on_subnet(&delegate.clone(), Self::get_root_netuid()).into();

        let return_per_1000: U64F64 =
            Self::return_per_1000_tao(take, total_stake, emissions_per_day);

        DelegateInfo {
            delegate_ss58: delegate.clone(),
            take,
            nominators,
            owner_ss58: owner.clone(),
            registrations: registrations.iter().map(|x| x.into()).collect(),
            validator_permits,
            return_per_1000: return_per_1000.saturating_to_num::<u64>().into(),
            total_daily_return: emissions_per_day.saturating_to_num::<u64>().into(),
        }
    }

    pub fn get_delegate(delegate: T::AccountId) -> Option<DelegateInfo<T::AccountId>> {
        // Check delegate exists
        if !<Delegates<T>>::contains_key(delegate.clone()) {
            return None;
        }

        let delegate_info = Self::get_delegate_by_existing_account(delegate.clone());
        Some(delegate_info)
    }

    /// get all delegates info from storage
    ///
    pub fn get_delegates() -> Vec<DelegateInfo<T::AccountId>> {
        let mut delegates = Vec::<DelegateInfo<T::AccountId>>::new();
        for delegate in <Delegates<T> as IterableStorageMap<T::AccountId, u16>>::iter_keys() {
            let delegate_info = Self::get_delegate_by_existing_account(delegate.clone());
            delegates.push(delegate_info);
        }

        delegates
    }

    /// get all delegate info and staked token amount for a given delegatee account
    ///
    pub fn get_delegated(
        delegatee: T::AccountId,
    ) -> Vec<(DelegateInfo<T::AccountId>, Compact<u64>)> {
        let mut delegates: Vec<(DelegateInfo<T::AccountId>, Compact<u64>)> = Vec::new();
        for delegate in <Delegates<T> as IterableStorageMap<T::AccountId, u16>>::iter_keys() {
            // Staked to this delegate, so add to list
            let delegate_info = Self::get_delegate_by_existing_account(delegate.clone());
            delegates.push((
                delegate_info,
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &delegatee,
                    &delegate,
                    Self::get_root_netuid(),
                )
                .into(),
            ));
        }

        delegates
    }

    // Helper function to get the coldkey associated with a hotkey
    pub fn get_coldkey_for_hotkey(hotkey: &T::AccountId) -> T::AccountId {
        Owner::<T>::get(hotkey)
    }
}
