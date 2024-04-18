use super::*;
use frame_support::{
    storage::IterableStorageDoubleMap,
    traits::{
        tokens::{
            fungible::{Balanced as _, Inspect as _, Mutate as _},
            Fortitude, Precision, Preservation,
        },
        Imbalance,
    },
};
use sp_core::Get;
use substrate_fixed::types::{I64F64};

impl<T: Config> Pallet<T> {
    // ---- The implementation for the extrinsic become_delegate: signals that this hotkey allows delegated stake.
    //
    // # Args:
    // 	* 'origin': (<T as frame_system::Config>RuntimeOrigin):
    // 		- The signature of the caller's coldkey.
    //
    // 	* 'hotkey' (T::AccountId):
    // 		- The hotkey we are delegating (must be owned by the coldkey.)
    //
    // 	* 'take' (u16):
    // 		- The stake proportion that this hotkey takes from delegations for subnet ID.
    //
    // # Event:
    // 	* DelegateAdded;
    // 		- On successfully setting a hotkey as a delegate.
    //
    // # Raises:
    // 	* 'NotRegistered':
    // 		- The hotkey we are delegating is not registered on the network.
    //
    // 	* 'NonAssociatedColdKey':
    // 		- The hotkey we are delegating is not owned by the calling coldket.
    //
    // 	* 'TxRateLimitExceeded':
    // 		- Thrown if key has hit transaction rate limit
    //
    pub fn do_become_delegate(
        origin: T::RuntimeOrigin,
        hotkey: T::AccountId,
        take: u16,
    ) -> dispatch::DispatchResult {
        // --- 1. We check the coldkey signature.
        let coldkey = ensure_signed(origin)?;
        log::info!(
            "do_become_delegate( origin:{:?} hotkey:{:?}, take:{:?} )",
            coldkey,
            hotkey,
            take
        );

        // --- 2. Ensure we are delegating a known key.
        // --- 3. Ensure that the coldkey is the owner.
        Self::do_take_checks(&coldkey, &hotkey)?;

        // --- 4. Ensure take is within the 0 ..= InitialDefaultTake (18%) range
        let max_take = T::InitialDefaultTake::get();
        ensure!(
            take <= max_take,
            Error::<T>::InvalidTake
        );

        // --- 5. Ensure we are not already a delegate (dont allow changing delegate take here.)
        ensure!(
            !Self::hotkey_is_delegate(&hotkey),
            Error::<T>::AlreadyDelegate
        );

        // --- 6. Ensure we don't exceed tx rate limit
        let block: u64 = Self::get_current_block_as_u64();
        ensure!(
            !Self::exceeds_tx_rate_limit(Self::get_last_tx_block(&coldkey), block),
            Error::<T>::TxRateLimitExceeded
        );

        // --- 7. Delegate the key.
        Self::delegate_hotkey(&hotkey, take);

        // Set last block for rate limiting
        Self::set_last_tx_block(&coldkey, block);

        // Also, set last block for take increase rate limiting
        Self::set_last_tx_block_delegate_take(&coldkey, block);

        // --- 8. Emit the staking event.
        log::info!(
            "DelegateAdded( coldkey:{:?}, hotkey:{:?}, take:{:?} )",
            coldkey,
            hotkey,
            take
        );
        Self::deposit_event(Event::DelegateAdded(coldkey, hotkey, take));

        // --- 9. Ok and return.
        Ok(())
    }

    // ---- The implementation for the extrinsic decrease_take
    //
    // # Args:
    // 	* 'origin': (<T as frame_system::Config>::RuntimeOrigin):
    // 		- The signature of the caller's coldkey.
    //
    // 	* 'hotkey' (T::AccountId):
    // 		- The hotkey we are delegating (must be owned by the coldkey.)
    //
    // 	* 'netuid' (u16):
    // 		- Subnet ID to decrease take for
    //
    // 	* 'take' (u16):
    // 		- The stake proportion that this hotkey takes from delegations for subnet ID.
    //
    // # Event:
    // 	* TakeDecreased;
    // 		- On successfully setting a decreased take for this hotkey.
    //
    // # Raises:
    // 	* 'NotRegistered':
    // 		- The hotkey we are delegating is not registered on the network.
    //
    // 	* 'NonAssociatedColdKey':
    // 		- The hotkey we are delegating is not owned by the calling coldket.
    //
    pub fn do_decrease_take(
        origin: T::RuntimeOrigin,
        hotkey: T::AccountId,
        netuid: u16,
        take: u16,
    ) -> dispatch::DispatchResult {
        // --- 1. We check the coldkey signature.
        let coldkey = ensure_signed(origin)?;
        log::info!(
            "do_decrease_take( origin:{:?} hotkey:{:?}, take:{:?} )",
            coldkey,
            hotkey,
            take
        );

        // --- 2. Ensure we are delegating a known key.
        //        Ensure that the coldkey is the owner.
        Self::do_take_checks(&coldkey, &hotkey)?;

        // --- 3. Ensure we are always strictly decreasing, never increasing take
        if let Ok(current_take) = DelegatesTake::<T>::try_get(&hotkey, netuid) {
            ensure!(
                take < current_take,
                Error::<T>::InvalidTake
            );
        }

        // --- 4. Set the new take value.
        DelegatesTake::<T>::insert(hotkey.clone(), netuid, take);

        // --- 5. Emit the take value.
        log::info!(
            "TakeDecreased( coldkey:{:?}, hotkey:{:?}, take:{:?} )",
            coldkey,
            hotkey,
            take
        );
        Self::deposit_event(Event::TakeDecreased(coldkey, hotkey, take));

        // --- 6. Ok and return.
        Ok(())
    }

    // ---- The implementation for the extrinsic increase_take
    //
    // # Args:
    // 	* 'origin': (<T as frame_system::Config>::RuntimeOrigin):
    // 		- The signature of the caller's coldkey.
    //
    // 	* 'hotkey' (T::AccountId):
    // 		- The hotkey we are delegating (must be owned by the coldkey.)
    //
    // 	* 'netuid' (u16):
    // 		- Subnet ID to increase take for
    //
    // 	* 'take' (u16):
    // 		- The stake proportion that this hotkey takes from delegations for subnet ID.
    //
    // # Event:
    // 	* TakeDecreased;
    // 		- On successfully setting a decreased take for this hotkey.
    //
    // # Raises:
    // 	* 'NotRegistered':
    // 		- The hotkey we are delegating is not registered on the network.
    //
    // 	* 'NonAssociatedColdKey':
    // 		- The hotkey we are delegating is not owned by the calling coldket.
    //
    // 	* 'TxRateLimitExceeded':
    // 		- Thrown if key has hit transaction rate limit
    //
    pub fn do_increase_take(
        origin: T::RuntimeOrigin,
        hotkey: T::AccountId,
        netuid: u16,
        take: u16,
    ) -> dispatch::DispatchResult {
        // --- 1. We check the coldkey signature.
        let coldkey = ensure_signed(origin)?;
        log::info!(
            "do_increase_take( origin:{:?} hotkey:{:?}, take:{:?} )",
            coldkey,
            hotkey,
            take
        );

        // --- 2. Ensure we are delegating a known key.
        //        Ensure that the coldkey is the owner.
        Self::do_take_checks(&coldkey, &hotkey)?;

        // --- 3. Ensure we are strinctly increasing take
        if let Ok(current_take) = DelegatesTake::<T>::try_get(&hotkey, netuid) {
            ensure!(
                take > current_take,
                Error::<T>::InvalidTake
            );
        }

        // --- 4. Ensure take is within the 0 ..= InitialDefaultTake (18%) range
        let max_take = T::InitialDefaultTake::get();
        ensure!(
            take <= max_take,
            Error::<T>::InvalidTake
        );

        // --- 5. Enforce the rate limit (independently on do_add_stake rate limits)
        let block: u64 = Self::get_current_block_as_u64();
        ensure!(
            !Self::exceeds_tx_delegate_take_rate_limit(Self::get_last_tx_block_delegate_take(&coldkey), block),
            Error::<T>::TxRateLimitExceeded
        );

        // Set last block for rate limiting
        Self::set_last_tx_block_delegate_take(&coldkey, block);

        // --- 6. Set the new take value.
        DelegatesTake::<T>::insert(hotkey.clone(), netuid, take);

        // --- 7. Emit the take value.
        log::info!(
            "TakeIncreased( coldkey:{:?}, hotkey:{:?}, take:{:?} )",
            coldkey,
            hotkey,
            take
        );
        Self::deposit_event(Event::TakeIncreased(coldkey, hotkey, take));

        // --- 8. Ok and return.
        Ok(())
    }


    /// Adds or redistributes weighted stake across specified subnets for a given hotkey.
    ///
    /// This function allows a coldkey to allocate or reallocate stake across different subnets
    /// based on provided weights. It first unstakes from all specified subnets, then redistributes
    /// the stake according to the new weights. If there's any remainder from rounding errors or
    /// unallocated stake, it is staked into the root network.
    //
    // # Args:
    // 	* 'origin': (<T as frame_system::Config>RuntimeOrigin):
    // 		- The signature of the caller's coldkey.
    //
    // 	* 'hotkey' (T::AccountId):
    // 		- The associated hotkey account.
    //    
    // 	* 'netuids' ( Vec<u16> ):
    // 		- The netuids of the weights to be set on the chain.
    //
    // 	* 'values' ( Vec<u16> ):
    // 		- The values of the weights to set on the chain. u16 normalized.
    //
    // 	* 'stake_to_be_added' (u64):
    // 		- The amount of stake to be added to the hotkey staking account.
    //
    // # Event:
    // 	* StakeAdded;
    // 		- On the successfully adding stake to a global account.
    //
    // # Raises:
    // 	* 'CouldNotConvertToBalance':
    // 		- Unable to convert the passed stake value to a balance.
    //
    // 	* 'NotEnoughBalanceToStake':
    // 		- Not enough balance on the coldkey to add onto the global account.
    //
    // 	* 'NonAssociatedColdKey':
    // 		- The calling coldkey is not associated with this hotkey.
    //
    // 	* 'BalanceWithdrawalError':
    // 		- Errors stemming from transaction pallet.
    //
    // 	* 'TxRateLimitExceeded':
    // 		- Thrown if key has hit transaction rate limit
    //
    // TODO(greg) test this.
    pub fn do_add_weighted_stake(
        origin: T::RuntimeOrigin,
        hotkey: T::AccountId,
        netuids: Vec<u16>,
        values: Vec<u16>,
    ) -> dispatch::DispatchResult {
        // --- 1. We check that the transaction is signed by the caller and retrieve the T::AccountId coldkey information.
        let coldkey = ensure_signed(origin)?;
        log::info!(
            "do_add_weighted_stake( origin:{:?} hotkey:{:?}, netuids:{:?}, values:{:?} )",
            coldkey,
            hotkey,
            netuids,
            values
        );

        // --- 2. Ensure that the hotkey account exists.
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::NotRegistered
        );

        // --- 3. We are either moving nominated stake or we own the hotkey.
        ensure!(
            Self::hotkey_is_delegate(&hotkey) || Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        // --- 4. Check weights rate limit.
        let block: u64 = Self::get_current_block_as_u64();
        ensure!(
            !Self::exceeds_tx_rate_limit(Self::get_last_tx_block(&coldkey), block),
            Error::<T>::TxRateLimitExceeded
        );

        // --- 5. Check that the length of netuid list and value list are equal for this network.
        ensure!(
            Self::uids_match_values(&netuids, &values),
            Error::<T>::WeightVecNotEqualSize
        );

        // --- 6. Ensure the passed netuids contain no duplicates.
        ensure!(!Self::has_duplicate_uids(&netuids), Error::<T>::DuplicateUids);

        // --- 7. Ensure that the netuids are valid.
        for netuid in netuids.iter() {
            ensure!(
                Self::if_subnet_exist(*netuid),
                Error::<T>::NetworkDoesNotExist
            );
        }

        // --- 8. Unstake from all subnets here.
        let mut total_removed: u64 = 0;
        let all_netuids: Vec<u16> = Self::get_all_subnet_netuids();
        for netuid_i in all_netuids.iter() {
            
            // --- 8.a Get the stake on all of the subnets.
            let netuid_stake_for_coldkey_i: u64 = Self::get_subnet_stake_for_coldkey_and_hotkey( &coldkey, &hotkey, *netuid_i );

            // --- 8.b Remove this stake from this network.
            Self::decrease_stake_on_coldkey_hotkey_account(
                &coldkey,
                &hotkey,
                *netuid_i,
                netuid_stake_for_coldkey_i,
            );

            // --- 8.c  Increment total removed.
            total_removed += netuid_stake_for_coldkey_i
        }

        // --- 9. Get sum of stake weights being set.
        let value_sum: u64 = values.iter().map(|&val| val as u64).sum();
        let weights_sum: I64F64 = I64F64::from_num(value_sum);

        // -- 10. Iterate over netuid value and stake to individual subnets proportional to weights.
        let mut total_stake_allocated: u64 = 0;
        let mut amounts_staked: Vec<u64> = vec![];
        for (netuid_i, weight_i) in netuids.iter().zip(values.iter()) {

            // 10.a -- Normalize the weight.
            let normalized_weight:I64F64 = I64F64::from_num( *weight_i ) / weights_sum;
            // 10.b -- Calculate effective stake based on the total removed in the previous step.
            let stake_to_be_added_netuid: u64 = (normalized_weight * I64F64::from_num( total_removed )).to_num::<u64>();
            // 10.c -- Set stake on subnet the effective stake.
            Self::increase_stake_on_coldkey_hotkey_account(
                &coldkey,
                &hotkey,
                *netuid_i,
                stake_to_be_added_netuid,
            );

            // 10.d -- Sum amounts for accounting remainder
            amounts_staked.push( stake_to_be_added_netuid ); 
            total_stake_allocated += stake_to_be_added_netuid;
        }

        // --- 11. Stake remainder to root network for accounting purposes.
        let remainder_stake: u64 = total_removed - total_stake_allocated;
        if remainder_stake > 0 {
            Self::increase_stake_on_coldkey_hotkey_account(
                &coldkey,
                &hotkey,
                Self::get_root_netuid(),
                remainder_stake,
            );
        }

        // -- 16. Set last block for rate limiting
        Self::set_last_tx_block(&coldkey, block);

        // --- 17. Emit the staking event.
        log::info!(
            "StakeWeightAdded( hotkey:{:?}, netuids:{:?}, values:{:?}, stakes:{:?} )",
            hotkey,
            netuids,
            values,
            amounts_staked
        );
        Self::deposit_event(Event::StakeAdded(hotkey, 0, total_removed)); // Restaking the total_removed amount.

        // --- 18. Ok and return.
        Ok(())
    }

    // ---- The implementation for the extrinsic add_stake: Adds stake to a hotkey account.
    //
    // # Args:
    // 	* 'origin': (<T as frame_system::Config>RuntimeOrigin):
    // 		- The signature of the caller's coldkey.
    //
    // 	* 'hotkey' (T::AccountId):
    // 		- The associated hotkey account.
    //
    // 	* 'netuid' (u16):
    // 		- The netuid to stake into.
    //
    // 	* 'stake_to_be_added' (u64):
    // 		- The amount of stake to be added to the hotkey staking account.
    //
    // # Event:
    // 	* StakeAdded;
    // 		- On the successfully adding stake to a global account.
    //
    // # Raises:
    // 	* 'CouldNotConvertToBalance':
    // 		- Unable to convert the passed stake value to a balance.
    //
    // 	* 'NotEnoughBalanceToStake':
    // 		- Not enough balance on the coldkey to add onto the global account.
    //
    // 	* 'NonAssociatedColdKey':
    // 		- The calling coldkey is not associated with this hotkey.
    //
    // 	* 'BalanceWithdrawalError':
    // 		- Errors stemming from transaction pallet.
    //
    // 	* 'TxRateLimitExceeded':
    // 		- Thrown if key has hit transaction rate limit
    //
    pub fn do_add_stake(
        origin: T::RuntimeOrigin,
        hotkey: T::AccountId,
        netuid: u16,
        stake_to_be_added: u64,
    ) -> dispatch::DispatchResult {
        // --- 1. We check that the transaction is signed by the caller and retrieve the T::AccountId coldkey information.
        let coldkey = ensure_signed(origin)?;
        log::info!(
            "do_add_stake( origin:{:?} hotkey:{:?}, netuid:{:?}, stake_to_be_added:{:?} )",
            coldkey,
            hotkey,
            netuid,
            stake_to_be_added
        );

        // --- 2. Ensure that the netuid exists.
        ensure!(
            Self::if_subnet_exist(netuid),
            Error::<T>::NetworkDoesNotExist
        );

        // --- 3. We convert the stake u64 into a balance.
        let stake_as_balance = Self::u64_to_balance(stake_to_be_added);
        ensure!(
            stake_as_balance.is_some(),
            Error::<T>::CouldNotConvertToBalance
        );

        // --- 4. Ensure the callers coldkey has enough stake to perform the transaction.
        ensure!(
            Self::can_remove_balance_from_coldkey_account(&coldkey, stake_as_balance.unwrap()),
            Error::<T>::NotEnoughBalanceToStake
        );

        // --- 5. Ensure that the hotkey account exists this is only possible through registration.
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::NotRegistered
        );

        // --- 6. Ensure that the hotkey allows delegation or that the hotkey is owned by the calling coldkey.
        ensure!(
            Self::hotkey_is_delegate(&hotkey) || Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        // --- 7. Enforce the nominator limit
        // let nominator_count: u32 = 0; // TODO: get the number of nominators
        // ensure!(
        //     nominator_count < DelegateLimit::<T>::get(),
        //     Error::<T>::TooManyNominations
        // );

        // --- 8. Ensure we don't exceed tx rate limit
        let block: u64 = Self::get_current_block_as_u64();
        ensure!(
            !Self::exceeds_tx_rate_limit(Self::get_last_tx_block(&coldkey), block),
            Error::<T>::TxRateLimitExceeded
        );

        // --- 9. Ensure we don't exceed stake rate limit
        let stakes_this_interval = Self::get_stakes_this_interval_for_hotkey(&hotkey);
        ensure!(
            stakes_this_interval < Self::get_target_stakes_per_interval(),
            Error::<T>::StakeRateLimitExceeded
        );

        // --- 10. Ensure the remove operation from the coldkey is a success.
        let actual_amount_to_stake =
            Self::remove_balance_from_coldkey_account(&coldkey, stake_as_balance.unwrap())?;

        // --- 11. If we reach here, add the balance to the hotkey.
        Self::increase_stake_on_coldkey_hotkey_account(
            &coldkey,
            &hotkey,
            netuid,
            actual_amount_to_stake,
        );

        // -- 12. Set last block for rate limiting
        Self::set_last_tx_block(&coldkey, block);

        // --- 13. Emit the staking event.
        Self::set_stakes_this_interval_for_hotkey(&hotkey, stakes_this_interval + 1, block);
        log::info!(
            "StakeAdded( hotkey:{:?}, netuid:{:?}, stake_to_be_added:{:?} )",
            hotkey,
            netuid,
            stake_to_be_added
        );
        Self::deposit_event(Event::StakeAdded(hotkey, netuid, stake_to_be_added));

        // --- 14. Ok and return.
        Ok(())
    }

    // ---- The implementation for the extrinsic remove_stake: Removes stake from a hotkey account and adds it onto a coldkey.
    //
    // # Args:
    // 	* 'origin': (<T as frame_system::Config>RuntimeOrigin):
    // 		- The signature of the caller's coldkey.
    //
    // 	* 'hotkey' (T::AccountId):
    // 		- The associated hotkey account.
    //
    // 	* 'netuid' (u16):
    // 		- The netuid to remove stake from.
    //
    // 	* 'stake_to_be_added' (u64):
    // 		- The amount of stake to be added to the hotkey staking account.
    //
    // # Event:
    // 	* StakeRemoved;
    // 		- On the successfully removing stake from the hotkey account.
    //
    // # Raises:
    //
    //  * 'NetworkDoesNotExist':
    //      - Thrown if the subnet we are attempting to stake into does not exist.
    //
    // 	* 'NotRegistered':
    // 		- Thrown if the account we are attempting to unstake from is non existent.
    //
    // 	* 'NonAssociatedColdKey':
    // 		- Thrown if the coldkey does not own the hotkey we are unstaking from.
    //
    // 	* 'NotEnoughStaketoWithdraw':
    // 		- Thrown if there is not enough stake on the hotkey to withdwraw this amount.
    //
    // 	* 'CouldNotConvertToBalance':
    // 		- Thrown if we could not convert this amount to a balance.
    //
    // 	* 'TxRateLimitExceeded':
    // 		- Thrown if key has hit transaction rate limit
    //
    //
    pub fn do_remove_stake(
        origin: T::RuntimeOrigin,
        hotkey: T::AccountId,
        netuid: u16,
        stake_to_be_removed: u64,
    ) -> dispatch::DispatchResult {
        // --- 1. We check the transaction is signed by the caller and retrieve the T::AccountId coldkey information.
        let coldkey = ensure_signed(origin)?;
        log::info!(
            "do_remove_stake( origin:{:?} netuid:{:?}, hotkey:{:?}, stake_to_be_removed:{:?} )",
            coldkey,
            hotkey,
            netuid,
            stake_to_be_removed
        );

        // --- 2. Ensure that the netuid exists.
        ensure!(
            Self::if_subnet_exist(netuid),
            Error::<T>::NetworkDoesNotExist
        );

        // --- 3. Ensure that the hotkey account exists this is only possible through registration.
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::NotRegistered
        );

        // --- 4. Ensure that the hotkey allows delegation or that the hotkey is owned by the calling coldkey.
        ensure!(
            Self::hotkey_is_delegate(&hotkey) || Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        // --- 5. Ensure that the stake amount to be removed is above zero.
        ensure!(
            stake_to_be_removed > 0,
            Error::<T>::NotEnoughStaketoWithdraw
        );

        // --- 6. Ensure that the hotkey has enough stake to withdraw.
        ensure!(
            Self::has_enough_stake(&coldkey, &hotkey, netuid, stake_to_be_removed),
            Error::<T>::NotEnoughStaketoWithdraw
        );

        // --- 7. Ensure that we can conver this u64 to a balance.
        let stake_to_be_added_as_currency = Self::u64_to_balance(stake_to_be_removed);
        ensure!(
            stake_to_be_added_as_currency.is_some(),
            Error::<T>::CouldNotConvertToBalance
        );

        // --- 8. Ensure we don't exceed tx rate limit
        let block: u64 = Self::get_current_block_as_u64();
        ensure!(
            !Self::exceeds_tx_rate_limit(Self::get_last_tx_block(&coldkey), block),
            Error::<T>::TxRateLimitExceeded
        );

        // --- 7. Ensure we don't exceed stake rate limit
        let unstakes_this_interval = Self::get_stakes_this_interval_for_hotkey(&hotkey);
        ensure!(
            unstakes_this_interval < Self::get_target_stakes_per_interval(),
            Error::<T>::UnstakeRateLimitExceeded
        );

        // --- 8. We remove the balance from the hotkey.
        Self::decrease_stake_on_coldkey_hotkey_account(
            &coldkey,
            &hotkey,
            netuid,
            stake_to_be_removed,
        );

        // --- 9. We add the balancer to the coldkey.  If the above fails we will not credit this coldkey.
        Self::add_balance_to_coldkey_account(&coldkey, stake_to_be_added_as_currency.unwrap());

        // Set last block for rate limiting
        Self::set_last_tx_block(&coldkey, block);

        // --- 10. Emit the unstaking event.
        Self::set_stakes_this_interval_for_hotkey(&hotkey, unstakes_this_interval + 1, block);
        log::info!(
            "StakeRemoved( hotkey:{:?}, stake_to_be_removed:{:?} )",
            hotkey,
            stake_to_be_removed
        );
        Self::deposit_event(Event::StakeRemoved(hotkey, netuid, stake_to_be_removed));

        // --- 11. Done and ok.
        Ok(())
    }

    // Returns true if the passed hotkey allow delegative staking.
    //
    pub fn hotkey_is_delegate(hotkey: &T::AccountId) -> bool {
        Delegates::<T>::contains_key(hotkey)
    }

    // Sets the hotkey as a delegate with take.
    //
    pub fn delegate_hotkey(hotkey: &T::AccountId, take: u16) {
        Delegates::<T>::insert(hotkey, take);
    }

    // Returns the total amount of stake in the staking table.
    //
    pub fn get_total_stake() -> u64 {
        return TotalStake::<T>::get();
    }

    // Returns the total amount of stake under a subnet (delegative or otherwise)
    pub fn get_total_stake_for_subnet(target_subnet: u16) -> u64 {
        SubStake::<T>::iter()
            .filter(|((_, _, subnet), _)| *subnet == target_subnet)
            .fold(0, |acc, (_, stake)| acc.saturating_add(stake))
    }

    // Increases the total amount of stake by the passed amount.
    //
    pub fn increase_total_stake(increment: u64) {
        TotalStake::<T>::put(Self::get_total_stake().saturating_add(increment));
    }

    // Decreases the total amount of stake by the passed amount.
    //
    pub fn decrease_total_stake(decrement: u64) {
        TotalStake::<T>::put(Self::get_total_stake().saturating_sub(decrement));
    }

    // Returns the total amount of stake under a hotkey (delegative or otherwise)
    //
    pub fn get_total_stake_for_hotkey(hotkey: &T::AccountId) -> u64 {
        return TotalHotkeyStake::<T>::get(hotkey);
    }

    // Returns the total amount of stake under a hotkey for a subnet (delegative or otherwise)
    //
    pub fn get_total_stake_for_hotkey_and_subnet(hotkey: &T::AccountId, netuid: u16) -> u64 {
        return TotalHotkeySubStake::<T>::get(hotkey, netuid);
    }

    // Returns the total amount of stake held by the coldkey (delegative or otherwise)
    //
    pub fn get_total_stake_for_coldkey(coldkey: &T::AccountId) -> u64 {
        return TotalColdkeyStake::<T>::get(coldkey);
    }

    // Retrieves the total stakes for a given hotkey (account ID) for the current staking interval.
    pub fn get_stakes_this_interval_for_hotkey(hotkey: &T::AccountId) -> u64 {
        // Retrieve the configured stake interval duration from storage.
        let stake_interval = StakeInterval::<T>::get();

        // Obtain the current block number as an unsigned 64-bit integer.
        let current_block = Self::get_current_block_as_u64();

        // Fetch the total stakes and the last block number when stakes were made for the hotkey.
        let (stakes, block_last_staked_at) = TotalHotkeyStakesThisInterval::<T>::get(hotkey);

        // Calculate the block number after which the stakes for the hotkey should be reset.
        let block_to_reset_after = block_last_staked_at + stake_interval;

        // If the current block number is beyond the reset point,
        // it indicates the end of the staking interval for the hotkey.
        if block_to_reset_after <= current_block {
            // Reset the stakes for this hotkey for the current interval.
            Self::set_stakes_this_interval_for_hotkey(hotkey, 0, block_last_staked_at);
            // Return 0 as the stake amount since we've just reset the stakes.
            return 0;
        }

        // If the staking interval has not yet ended, return the current stake amount.
        stakes
    }

    pub fn get_target_stakes_per_interval() -> u64 {
        return TargetStakesPerInterval::<T>::get();
    }

    // Creates a cold - hot pairing account if the hotkey is not already an active account.
    //
    pub fn create_account_if_non_existent(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: u16,
    ) {
        if !Self::hotkey_account_exists(hotkey) {
            Stake::<T>::insert(hotkey, coldkey, 0);
            SubStake::<T>::insert((hotkey, coldkey, netuid), 0);
            Owner::<T>::insert(hotkey, coldkey);
        }
    }

    // Returns the coldkey owning this hotkey. This function should only be called for active accounts.
    //
    pub fn get_owning_coldkey_for_hotkey(hotkey: &T::AccountId) -> T::AccountId {
        return Owner::<T>::get(hotkey);
    }

    // Returns the hotkey take
    //
    pub fn get_delegate_take(hotkey: &T::AccountId, netuid: u16) -> u16 {
        DelegatesTake::<T>::get(hotkey, netuid)
    }

    // Returns true if the hotkey account has been created.
    //
    pub fn hotkey_account_exists(hotkey: &T::AccountId) -> bool {
        return Owner::<T>::contains_key(hotkey);
    }

    // Return true if the passed coldkey owns the hotkey.
    //
    pub fn coldkey_owns_hotkey(coldkey: &T::AccountId, hotkey: &T::AccountId) -> bool {
        if Self::hotkey_account_exists(hotkey) {
            return Owner::<T>::get(hotkey) == *coldkey;
        } else {
            return false;
        }
    }

    // Returns true if the cold-hot staking account has enough balance to fufil the decrement.
    //
    pub fn has_enough_stake(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: u16,
        decrement: u64,
    ) -> bool {
        return Self::get_subnet_stake_for_coldkey_and_hotkey(coldkey, hotkey, netuid) >= decrement;
    }

    // Increases the stake on the hotkey account under its owning coldkey.
    //
    pub fn increase_stake_on_hotkey_account(hotkey: &T::AccountId, netuid: u16, increment: u64) {
        Self::increase_stake_on_coldkey_hotkey_account(
            &Self::get_owning_coldkey_for_hotkey(hotkey),
            hotkey,
            netuid,
            increment,
        );
    }

    // Decreases the stake on the hotkey account under its owning coldkey.
    //
    pub fn decrease_stake_on_hotkey_account(hotkey: &T::AccountId, netuid: u16, decrement: u64) {
        Self::decrease_stake_on_coldkey_hotkey_account(
            &Self::get_owning_coldkey_for_hotkey(hotkey),
            hotkey,
            netuid,
            decrement,
        );
    }

    // Returns the subent stake under the cold - hot pairing in the staking table.
    //
    pub fn get_subnet_stake_for_coldkey_and_hotkey(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: u16,
    ) -> u64 {
        SubStake::<T>::try_get((hotkey, coldkey, netuid)).unwrap_or(0)
    }

    // Returns the stake under the cold - hot pairing in the staking table.
    //
    pub fn get_total_stake_for_hotkey_and_coldkey(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> u64 {
        Stake::<T>::try_get(hotkey, coldkey).unwrap_or(0)
    }

    // Increases the stake on the cold - hot pairing by increment while also incrementing other counters.
    // This function should be called rather than set_stake under account.
    //
    pub fn increase_stake_on_coldkey_hotkey_account(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: u16,
        increment: u64,
    ) {
        if increment == 0 {
            return;
        }
        TotalColdkeyStake::<T>::mutate(coldkey, |stake| {
            *stake = stake.saturating_add(increment);
        });
        TotalHotkeyStake::<T>::mutate(hotkey, |stake| {
            *stake = stake.saturating_add(increment);
        });
        TotalHotkeySubStake::<T>::mutate(hotkey,netuid, |stake| {
            *stake = stake.saturating_add(increment);
        });
        Stake::<T>::mutate(hotkey, coldkey, |stake| {
            *stake = stake.saturating_add(increment);
        });
        SubStake::<T>::insert((hotkey, coldkey, netuid),
            SubStake::<T>::try_get((hotkey, coldkey, netuid))
                .unwrap_or(0)
                .saturating_add(increment),
        );
        TotalStake::<T>::mutate(|stake| {
            *stake = stake.saturating_add(increment);
        });
    }

    // Decreases the stake on the cold - hot pairing by the decrement while decreasing other counters.
    //
    pub fn decrease_stake_on_coldkey_hotkey_account(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: u16,
        decrement: u64,
    ) {
        if decrement == 0 {
            return;
        }
        TotalColdkeyStake::<T>::mutate(coldkey, |stake| {
            *stake = stake.saturating_sub(decrement);
        });
        TotalHotkeyStake::<T>::mutate(hotkey, |stake| {
            *stake = stake.saturating_sub(decrement);
        });
        TotalHotkeySubStake::<T>::mutate(hotkey,netuid, |stake| {
            *stake = stake.saturating_sub(decrement);
        });
        Stake::<T>::mutate(hotkey,coldkey, |stake| {
            *stake = stake.saturating_sub(decrement);
        });
        SubStake::<T>::insert((hotkey, coldkey, netuid),
            SubStake::<T>::try_get((hotkey, coldkey, netuid))
                .unwrap_or(0)
                .saturating_sub(decrement),
        );
        TotalStake::<T>::mutate(|stake| {
            *stake = stake.saturating_sub(decrement);
        });
    }

    pub fn u64_to_balance(
        input: u64,
    ) -> Option<
        <<T as Config>::Currency as fungible::Inspect<<T as frame_system::Config>::AccountId>>::Balance,
    >{
        input.try_into().ok()
    }

    pub fn add_balance_to_coldkey_account(
        coldkey: &T::AccountId,
        amount: <<T as Config>::Currency as fungible::Inspect<<T as system::Config>::AccountId>>::Balance,
    ) {
        // infallible
        let _ = T::Currency::deposit(&coldkey, amount, Precision::BestEffort);
    }

    pub fn set_balance_on_coldkey_account(
        coldkey: &T::AccountId,
        amount: <<T as Config>::Currency as fungible::Inspect<<T as system::Config>::AccountId>>::Balance,
    ) {
        T::Currency::set_balance(&coldkey, amount);
    }

    pub fn can_remove_balance_from_coldkey_account(
        coldkey: &T::AccountId,
        amount: <<T as Config>::Currency as fungible::Inspect<<T as system::Config>::AccountId>>::Balance,
    ) -> bool {
        let current_balance = Self::get_coldkey_balance(coldkey);
        if amount > current_balance {
            return false;
        }

        // This bit is currently untested. @todo
        let can_withdraw = T::Currency::can_withdraw(&coldkey, amount)
            .into_result(false)
            .is_ok();
        can_withdraw
    }

    pub fn get_coldkey_balance(
        coldkey: &T::AccountId,
    ) -> <<T as Config>::Currency as fungible::Inspect<<T as system::Config>::AccountId>>::Balance
    {
        return T::Currency::reducible_balance(
            &coldkey,
            Preservation::Expendable,
            Fortitude::Polite,
        );
    }

    #[must_use = "Balance must be used to preserve total issuance of token"]
    pub fn remove_balance_from_coldkey_account(
        coldkey: &T::AccountId,
        amount: <<T as Config>::Currency as fungible::Inspect<<T as system::Config>::AccountId>>::Balance,
    ) -> Result<u64, DispatchError> {
        let amount_u64: u64 = amount
            .try_into()
            .map_err(|_| Error::<T>::CouldNotConvertToU64)?;

        if amount_u64 == 0 {
            return Ok(0);
        }

        let credit = T::Currency::withdraw(
            &coldkey,
            amount,
            Precision::BestEffort,
            Preservation::Preserve,
            Fortitude::Polite,
        )
        .map_err(|_| Error::<T>::BalanceWithdrawalError)?
        .peek();

        let credit_u64: u64 = credit
            .try_into()
            .map_err(|_| Error::<T>::CouldNotConvertToU64)?;

        if credit_u64 == 0 {
            return Err(Error::<T>::BalanceWithdrawalError.into());
        }

        Ok(credit_u64)
    }

    pub fn unstake_all_coldkeys_from_hotkey_account(hotkey: &T::AccountId) {
        // Iterate through all coldkeys that have a stake on this hotkey account.
        let all_netuids: Vec<u16> = Self::get_all_subnet_netuids();
        for (coldkey_i, _) in
            <Stake<T> as IterableStorageDoubleMap<T::AccountId, T::AccountId, u64>>::iter_prefix(
                hotkey,
            )
        {
            for netuid_i in all_netuids.iter() {
                // Get the stake on this uid.
                let stake_i =
                    Self::get_subnet_stake_for_coldkey_and_hotkey(&coldkey_i, hotkey, *netuid_i);

                // Convert to balance and add to the coldkey account.
                let stake_i_as_balance = Self::u64_to_balance(stake_i);
                if stake_i_as_balance.is_none() {
                    continue; // Don't unstake if we can't convert to balance.
                } else {
                    // Stake is successfully converted to balance.

                    // Remove the stake from the coldkey - hotkey pairing.
                    Self::decrease_stake_on_coldkey_hotkey_account(
                        &coldkey_i, hotkey, *netuid_i, stake_i,
                    );

                    // Add the balance to the coldkey account.
                    Self::add_balance_to_coldkey_account(&coldkey_i, stake_i_as_balance.unwrap());
                }
            }
        }
    }
}
