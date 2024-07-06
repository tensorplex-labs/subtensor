use super::*;
use sp_core::Get;
use frame_support::traits::fungible::Mutate;
use frame_support::traits::tokens::Preservation;
use frame_support::{storage::IterableStorageDoubleMap, weights::Weight};

impl<T: Config> Pallet<T> {
    /// Swaps the hotkey of a coldkey account.
    ///
    /// # Arguments
    ///
    /// * `origin` - The origin of the transaction, and also the coldkey account.
    /// * `old_hotkey` - The old hotkey to be swapped.
    /// * `new_hotkey` - The new hotkey to replace the old one.
    ///
    /// # Returns
    ///
    /// * `DispatchResultWithPostInfo` - The result of the dispatch.
    ///
    /// # Errors
    ///
    /// * `NonAssociatedColdKey` - If the coldkey does not own the old hotkey.
    /// * `HotKeySetTxRateLimitExceeded` - If the transaction rate limit is exceeded.
    /// * `NewHotKeyIsSameWithOld` - If the new hotkey is the same as the old hotkey.
    /// * `HotKeyAlreadyRegisteredInSubNet` - If the new hotkey is already registered in the subnet.
    /// * `NotEnoughBalanceToPaySwapHotKey` - If there is not enough balance to pay for the swap.
    pub fn do_swap_hotkey(
        origin: T::RuntimeOrigin,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
    ) -> DispatchResultWithPostInfo {
        let coldkey = ensure_signed(origin)?;
        ensure!(!Self::coldkey_in_arbitration(&coldkey), Error::<T>::ColdkeyIsInArbitration);

        let mut weight = T::DbWeight::get().reads(2);

        ensure!(old_hotkey != new_hotkey, Error::<T>::NewHotKeyIsSameWithOld);
        ensure!(
            !Self::is_hotkey_registered_on_any_network(new_hotkey),
            Error::<T>::HotKeyAlreadyRegisteredInSubNet
        );

        weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 0));
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, old_hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        let block: u64 = Self::get_current_block_as_u64();
        ensure!(
            !Self::exceeds_tx_rate_limit(Self::get_last_tx_block(&coldkey), block),
            Error::<T>::HotKeySetTxRateLimitExceeded
        );

        weight.saturating_accrue(
            T::DbWeight::get().reads((TotalNetworks::<T>::get().saturating_add(1u16)) as u64),
        );

        Self::swap_owner(old_hotkey, new_hotkey, &coldkey, &mut weight);
        Self::swap_total_hotkey_stake(old_hotkey, new_hotkey, &mut weight);
        Self::swap_delegates(old_hotkey, new_hotkey, &mut weight);
        Self::swap_stake(old_hotkey, new_hotkey, &mut weight);

        // Store the value of is_network_member for the old key
        let netuid_is_member: Vec<u16> = Self::get_netuid_is_member(old_hotkey, &mut weight);

        Self::swap_is_network_member(old_hotkey, new_hotkey, &netuid_is_member, &mut weight);
        Self::swap_axons(old_hotkey, new_hotkey, &netuid_is_member, &mut weight);
        Self::swap_keys(old_hotkey, new_hotkey, &netuid_is_member, &mut weight);
        Self::swap_loaded_emission(old_hotkey, new_hotkey, &netuid_is_member, &mut weight);
        Self::swap_uids(old_hotkey, new_hotkey, &netuid_is_member, &mut weight);
        Self::swap_prometheus(old_hotkey, new_hotkey, &netuid_is_member, &mut weight);

        Self::swap_total_hotkey_coldkey_stakes_this_interval(old_hotkey, new_hotkey, &mut weight);

        Self::set_last_tx_block(&coldkey, block);
        weight.saturating_accrue(T::DbWeight::get().writes(1));

        Self::deposit_event(Event::HotkeySwapped {
            coldkey,
            old_hotkey: old_hotkey.clone(),
            new_hotkey: new_hotkey.clone(),
        });

        Ok(Some(weight).into())
    }

    /// Swaps the coldkey associated with a set of hotkeys from an old coldkey to a new coldkey.
    ///
    /// # Arguments
    ///
    /// * `origin` - The origin of the call, which must be signed by the old coldkey.
    /// * `old_coldkey` - The account ID of the old coldkey.
    /// * `new_coldkey` - The account ID of the new coldkey.
    ///
    /// # Returns
    ///
    /// Returns a `DispatchResultWithPostInfo` indicating success or failure, along with the weight consumed.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - The caller is not the old coldkey.
    /// - The new coldkey is the same as the old coldkey.
    /// - The new coldkey is already associated with other hotkeys.
    /// - The transaction rate limit for coldkey swaps has been exceeded.
    /// - There's not enough balance to pay for the swap.
    ///
    /// # Events
    ///
    /// Emits a `ColdkeySwapped` event when successful.
    ///
    /// # Weight
    ///
    /// Weight is tracked and updated throughout the function execution.
    pub fn do_swap_coldkey(
        origin: T::RuntimeOrigin,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> DispatchResultWithPostInfo {
        let coldkey_performing_swap = ensure_signed(origin)?;
        ensure!(!Self::coldkey_in_arbitration(&coldkey_performing_swap), Error::<T>::ColdkeyIsInArbitration);

        let mut weight: Weight = T::DbWeight::get().reads(2);

        // Check that the coldkey is a new key (does not exist elsewhere.)
        ensure!(
            !Self::coldkey_has_associated_hotkeys(new_coldkey),
            Error::<T>::ColdKeyAlreadyAssociated
        );
        // Check that the new coldkey is not a hotkey.
        ensure!(
            !Self::hotkey_account_exists(new_coldkey),
            Error::<T>::ColdKeyAlreadyAssociated
        );

        // Actually do the swap.
        weight = weight.saturating_add(Self::perform_swap_coldkey(old_coldkey, new_coldkey, &mut weight).map_err(|_| Error::<T>::SwapError)?);

        Self::set_last_tx_block(new_coldkey, Self::get_current_block_as_u64() );
        weight.saturating_accrue(T::DbWeight::get().writes(1));

        Self::deposit_event(Event::ColdkeySwapped {
            old_coldkey: old_coldkey.clone(),
            new_coldkey: new_coldkey.clone(),
        });

        Ok(Some(weight).into())
    }

    /// Swaps the coldkey associated with a set of hotkeys from an old coldkey to a new coldkey over a delayed
    /// to faciliate arbitration.
    ///
    /// # Arguments
    ///
    /// * `origin` - The origin of the call, which must be signed by the old coldkey.
    /// * `new_coldkey` - The account ID of the new coldkey.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - The caller is the same key as the new key or if the new key is already in the arbitration keys.
    /// - There are already 2 keys in arbitration for this coldkey.
    ///   
    // The coldkey is in arbitration state.
    pub fn coldkey_in_arbitration( coldkey: &T::AccountId ) -> bool { ColdkeySwapDestinations::<T>::contains_key( coldkey ) } // Helper
    pub fn do_arbitrated_coldkey_swap(
        origin: T::RuntimeOrigin,
        new_coldkey: &T::AccountId,
    ) -> DispatchResult {

        // Attain the calling coldkey from the origin.
        let old_coldkey:T::AccountId = ensure_signed(origin)?;

        // Catch spurious swaps.
        ensure!(
            old_coldkey != *new_coldkey,
            Error::<T>::SameColdkey
        );

        // Get current destination coldkeys.
        let mut destination_coldkeys: Vec<T::AccountId> = ColdkeySwapDestinations::<T>::get( old_coldkey.clone() );

        // Check if the new coldkey is already in the swap wallets list
        ensure!(
            !destination_coldkeys.contains( new_coldkey ),
            Error::<T>::DuplicateColdkey
        );
    
        // Add the wallet to the swap wallets.
        let initial_destination_count = destination_coldkeys.len();

        // If the destinations keys are empty or have size 1 then we will add the new coldkey to the list.
        if initial_destination_count == 0 as usize || initial_destination_count == 1 as usize {
            // Extend the wallet to swap to.
            destination_coldkeys.push( new_coldkey.clone() );

            // Push the change to the storage.
            ColdkeySwapDestinations::<T>::insert( old_coldkey.clone(), destination_coldkeys.clone() );

        } else {

            // If the destinations len > 1 we dont add the new coldkey.
            return Err(Error::<T>::ColdkeyIsInArbitration.into());
        }

        // If this is the first time we have seen this key we will put the swap period to be in 7 days.
        if initial_destination_count == 0 as usize {

            // Set the arbitration period to be 7 days from now.
            let next_arbitration_period: u64 = Self::get_current_block_as_u64() + 7200 * 7;

            // First time seeing this key lets push the swap moment to 1 week in the future.
            let mut next_period_coldkeys_to_swap: Vec<T::AccountId> = ColdkeysToArbitrateAtBlock::<T>::get( next_arbitration_period );

            // Add the old coldkey to the next period keys to swap.
            // Sanity Check.
            if !next_period_coldkeys_to_swap.contains( old_coldkey.clone() ) {
                next_period_coldkeys_to_swap.push( old_coldkey.clone() );
            }

            // Set the new coldkeys to swap here.
            ColdkeysToArbitrateAtBlock::<T>::insert( next_arbitration_period, next_period_coldkeys_to_swap );
        }

        // Return true.
        Ok(())
    }

    /// Arbitrates coldkeys that are scheduled to be swapped on this block.
    ///
    /// This function retrieves the list of coldkeys scheduled to be swapped on the current block,
    /// and processes each coldkey by either extending the arbitration period or performing the swap
    /// to the new coldkey.
    ///
    /// # Returns
    ///
    /// * `Weight` - The total weight consumed by the operation.
    pub fn arbitrate_coldkeys_this_block() -> Result<Weight, &'static str> {
        let mut weight = frame_support::weights::Weight::from_parts(0, 0);

        // Get the block number
        let current_block: u64 = Self::get_current_block_as_u64();

        // Get the coldkeys to swap here and then remove them.
        let source_coldkeys: Vec<T::AccountId> = ColdkeysToArbitrateAtBlock::<T>::get( current_block );
        ColdkeysToArbitrateAtBlock::<T>::remove( current_block );
        weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));

        // Iterate over all keys in swap and call perform_swap_coldkey for each
        for coldkey_i in source_coldkeys.iter() {

            // Get the wallets to swap to for this coldkey.
            let destinations_coldkeys: Vec<T::AccountId> = ColdkeySwapDestinations::<T>::get( coldkey_i );
            ColdkeySwapDestinations::<T>::remove( &coldkey_i );
            weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));

            // If the wallets to swap is > 1, we extend the period.
            if destinations_coldkeys.len() > 1 {

                // Next arbitrage period
                let next_arbitrage_period: u64 = current_block + 7200 * 7;

                // Get the coldkeys to swap at the next arbitrage period.
                let mut next_period_coldkeys_to_swap: Vec<T::AccountId> = ColdkeysToArbitrateAtBlock::<T>::get( next_arbitrage_period );
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 0));

                // Add this new coldkey to these coldkeys
                // Sanity Check.
                if !next_period_coldkeys_to_swap.contains(coldkey_i) {
                    next_period_coldkeys_to_swap.push(coldkey_i.clone());
                }

                // Set the new coldkeys to swap here.
                ColdkeysToArbitrateAtBlock::<T>::insert( next_arbitrage_period, next_period_coldkeys_to_swap );
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(0, 1));

            } else if destinations_coldkeys.len() == 1 {
                // ONLY 1 wallet: Get the wallet to swap to.
                let new_coldkey = &destinations_coldkeys[0];

                // Perform the swap.
                if let Err(_) = Self::perform_swap_coldkey( &coldkey_i, new_coldkey ).map(|w| weight = weight.saturating_add(w)) {
                    return Err("Failed to perform coldkey swap");
                }
            }
        }

        Ok(weight)
    }


    pub fn perform_swap_coldkey( old_coldkey: &T::AccountId, new_coldkey: &T::AccountId ) -> Result<Weight, &'static str> {
        // Swap coldkey references in storage maps
        // NOTE The order of these calls is important
        Self::swap_total_coldkey_stake(old_coldkey, new_coldkey, &mut weight);
        Self::swap_stake_for_coldkey(old_coldkey, new_coldkey, &mut weight);
        Self::swap_owner_for_coldkey(old_coldkey, new_coldkey, &mut weight);
        Self::swap_total_hotkey_coldkey_stakes_this_interval_for_coldkey(
            old_coldkey,
            new_coldkey,
            &mut weight,
        );
        Self::swap_subnet_owner_for_coldkey(old_coldkey, new_coldkey, &mut weight);
        Self::swap_owned_for_coldkey(old_coldkey, new_coldkey, &mut weight);

        // Transfer any remaining balance from old_coldkey to new_coldkey
        let remaining_balance = Self::get_coldkey_balance(old_coldkey);
        if remaining_balance > 0 {
            if let Err(e) = Self::kill_coldkey_account(old_coldkey, remaining_balance) {
                return Err(e);
            }
            Self::add_balance_to_coldkey_account(new_coldkey, remaining_balance);
        }

        // Swap the coldkey.
        let total_balance:u64 = Self::get_coldkey_balance(&old_coldkey);
        if total_balance > 0 {
            // Attempt to transfer the entire total balance to coldkeyB.
            if let Err(e) = T::Currency::transfer(
                &old_coldkey,
                &new_coldkey,
                total_balance,
                Preservation::Expendable,
            ) {
                return Err(e);
            }
        }

        Ok(*weight)
    }

    /// Retrieves the network membership status for a given hotkey.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The hotkey to check for network membership.
    ///
    /// # Returns
    ///
    /// * `Vec<u16>` - A vector of network IDs where the hotkey is a member.
    pub fn get_netuid_is_member(old_hotkey: &T::AccountId, weight: &mut Weight) -> Vec<u16> {
        let netuid_is_member: Vec<u16> =
            <IsNetworkMember<T> as IterableStorageDoubleMap<_, _, _>>::iter_prefix(old_hotkey)
                .map(|(netuid, _)| netuid)
                .collect();
        weight.saturating_accrue(T::DbWeight::get().reads(netuid_is_member.len() as u64));
        netuid_is_member
    }

    /// Swaps the owner of the hotkey.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The old hotkey.
    /// * `new_hotkey` - The new hotkey.
    /// * `coldkey` - The coldkey owning the hotkey.
    /// * `weight` - The weight of the transaction.
    ///
    pub fn swap_owner(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        weight: &mut Weight,
    ) {
        Owner::<T>::remove(old_hotkey);
        Owner::<T>::insert(new_hotkey, coldkey.clone());

        // Update OwnedHotkeys map
        let mut hotkeys = OwnedHotkeys::<T>::get(coldkey);
        if !hotkeys.contains(new_hotkey) {
            hotkeys.push(new_hotkey.clone());
        }
        hotkeys.retain(|hk| *hk != *old_hotkey);
        OwnedHotkeys::<T>::insert(coldkey, hotkeys);

        weight.saturating_accrue(T::DbWeight::get().writes(2));
    }

    /// Swaps the total stake of the hotkey.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The old hotkey.
    /// * `new_hotkey` - The new hotkey.
    /// * `weight` - The weight of the transaction.
    ///
    /// # Weight Calculation
    ///
    /// * Reads: 1 if the old hotkey exists, otherwise 1 for the failed read.
    /// * Writes: 2 if the old hotkey exists (one for removal and one for insertion).
    pub fn swap_total_hotkey_stake(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        weight: &mut Weight,
    ) {
        if let Ok(total_hotkey_stake) = TotalHotkeyStake::<T>::try_get(old_hotkey) {
            TotalHotkeyStake::<T>::remove(old_hotkey);
            TotalHotkeyStake::<T>::insert(new_hotkey, total_hotkey_stake);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        } else {
            weight.saturating_accrue(T::DbWeight::get().reads(1));
        }
    }

    /// Swaps the delegates of the hotkey.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The old hotkey.
    /// * `new_hotkey` - The new hotkey.
    /// * `weight` - The weight of the transaction.
    ///
    /// # Weight Calculation
    ///
    /// * Reads: 1 if the old hotkey exists, otherwise 1 for the failed read.
    /// * Writes: 2 if the old hotkey exists (one for removal and one for insertion).
    pub fn swap_delegates(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        weight: &mut Weight,
    ) {
        if let Ok(delegate_take) = Delegates::<T>::try_get(old_hotkey) {
            Delegates::<T>::remove(old_hotkey);
            Delegates::<T>::insert(new_hotkey, delegate_take);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
        } else {
            weight.saturating_accrue(T::DbWeight::get().reads(1));
        }
    }

    /// Swaps the stake of the hotkey.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The old hotkey.
    /// * `new_hotkey` - The new hotkey.
    /// * `weight` - The weight of the transaction.
    pub fn swap_stake(old_hotkey: &T::AccountId, new_hotkey: &T::AccountId, weight: &mut Weight) {
        let mut writes: u64 = 0;
        let stakes: Vec<(T::AccountId, u64)> = Stake::<T>::iter_prefix(old_hotkey).collect();
        let stake_count = stakes.len() as u32;

        for (coldkey, stake_amount) in stakes {
            Stake::<T>::insert(new_hotkey, &coldkey, stake_amount);
            writes = writes.saturating_add(1u64); // One write for insert

            // Update StakingHotkeys map
            let mut staking_hotkeys = StakingHotkeys::<T>::get(&coldkey);
            if !staking_hotkeys.contains(new_hotkey) {
                staking_hotkeys.push(new_hotkey.clone());
                StakingHotkeys::<T>::insert(coldkey.clone(), staking_hotkeys);
                writes = writes.saturating_add(1u64); // One write for insert
            }
        }

        // Clear the prefix for the old hotkey after transferring all stakes
        let _ = Stake::<T>::clear_prefix(old_hotkey, stake_count, None);
        writes = writes.saturating_add(1); // One write for insert; // One write for clear_prefix

        // TODO: Remove all entries for old hotkey from StakingHotkeys map

        weight.saturating_accrue(T::DbWeight::get().writes(writes));
    }

    /// Swaps the network membership status of the hotkey.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The old hotkey.
    /// * `new_hotkey` - The new hotkey.
    /// * `netuid_is_member` - A vector of network IDs where the hotkey is a member.
    /// * `weight` - The weight of the transaction.
    pub fn swap_is_network_member(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid_is_member: &[u16],
        weight: &mut Weight,
    ) {
        let _ = IsNetworkMember::<T>::clear_prefix(old_hotkey, netuid_is_member.len() as u32, None);
        weight.saturating_accrue(T::DbWeight::get().writes(netuid_is_member.len() as u64));
        for netuid in netuid_is_member.iter() {
            IsNetworkMember::<T>::insert(new_hotkey, netuid, true);
            weight.saturating_accrue(T::DbWeight::get().writes(1));
        }
    }

    /// Swaps the axons of the hotkey.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The old hotkey.
    /// * `new_hotkey` - The new hotkey.
    /// * `netuid_is_member` - A vector of network IDs where the hotkey is a member.
    /// * `weight` - The weight of the transaction.
    ///
    /// # Weight Calculation
    ///
    /// * Reads: 1 for each network ID if the old hotkey exists in that network.
    /// * Writes: 2 for each network ID if the old hotkey exists in that network (one for removal and one for insertion).
    pub fn swap_axons(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid_is_member: &[u16],
        weight: &mut Weight,
    ) {
        for netuid in netuid_is_member.iter() {
            if let Ok(axon_info) = Axons::<T>::try_get(netuid, old_hotkey) {
                Axons::<T>::remove(netuid, old_hotkey);
                Axons::<T>::insert(netuid, new_hotkey, axon_info);
                weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
            } else {
                weight.saturating_accrue(T::DbWeight::get().reads(1));
            }
        }
    }

    /// Swaps the references in the keys storage map of the hotkey.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The old hotkey.
    /// * `new_hotkey` - The new hotkey.
    /// * `netuid_is_member` - A vector of network IDs where the hotkey is a member.
    /// * `weight` - The weight of the transaction.
    pub fn swap_keys(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid_is_member: &[u16],
        weight: &mut Weight,
    ) {
        let mut writes: u64 = 0;
        for netuid in netuid_is_member {
            let keys: Vec<(u16, T::AccountId)> = Keys::<T>::iter_prefix(netuid).collect();
            for (uid, key) in keys {
                if key == *old_hotkey {
                    log::info!("old hotkey found: {:?}", old_hotkey);
                    Keys::<T>::insert(netuid, uid, new_hotkey.clone());
                }
                writes = writes.saturating_add(2u64);
            }
        }
        log::info!("writes: {:?}", writes);
        weight.saturating_accrue(T::DbWeight::get().writes(writes));
    }

    /// Swaps the loaded emission of the hotkey.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The old hotkey.
    /// * `new_hotkey` - The new hotkey.
    /// * `netuid_is_member` - A vector of network IDs where the hotkey is a member.
    /// * `weight` - The weight of the transaction.
    ///
    pub fn swap_loaded_emission(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid_is_member: &[u16],
        weight: &mut Weight,
    ) {
        for netuid in netuid_is_member {
            if let Some(mut emissions) = LoadedEmission::<T>::get(netuid) {
                for emission in emissions.iter_mut() {
                    if emission.0 == *old_hotkey {
                        emission.0 = new_hotkey.clone();
                    }
                }
                LoadedEmission::<T>::insert(netuid, emissions);
            }
        }
        weight.saturating_accrue(T::DbWeight::get().writes(netuid_is_member.len() as u64));
    }

    /// Swaps the UIDs of the hotkey.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The old hotkey.
    /// * `new_hotkey` - The new hotkey.
    /// * `netuid_is_member` - A vector of network IDs where the hotkey is a member.
    /// * `weight` - The weight of the transaction.
    ///
    pub fn swap_uids(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid_is_member: &[u16],
        weight: &mut Weight,
    ) {
        for netuid in netuid_is_member.iter() {
            if let Ok(uid) = Uids::<T>::try_get(netuid, old_hotkey) {
                Uids::<T>::remove(netuid, old_hotkey);
                Uids::<T>::insert(netuid, new_hotkey, uid);
                weight.saturating_accrue(T::DbWeight::get().writes(2));
            }
        }
    }

    /// Swaps the Prometheus data of the hotkey.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The old hotkey.
    /// * `new_hotkey` - The new hotkey.
    /// * `netuid_is_member` - A vector of network IDs where the hotkey is a member.
    /// * `weight` - The weight of the transaction.
    ///
    /// # Weight Calculation
    ///
    /// * Reads: 1 for each network ID if the old hotkey exists in that network.
    /// * Writes: 2 for each network ID if the old hotkey exists in that network (one for removal and one for insertion).
    pub fn swap_prometheus(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid_is_member: &[u16],
        weight: &mut Weight,
    ) {
        for netuid in netuid_is_member.iter() {
            if let Ok(prometheus_info) = Prometheus::<T>::try_get(netuid, old_hotkey) {
                Prometheus::<T>::remove(netuid, old_hotkey);
                Prometheus::<T>::insert(netuid, new_hotkey, prometheus_info);
                weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
            } else {
                weight.saturating_accrue(T::DbWeight::get().reads(1));
            }
        }
    }

    /// Swaps the total hotkey-coldkey stakes for the current interval.
    ///
    /// # Arguments
    ///
    /// * `old_hotkey` - The old hotkey.
    /// * `new_hotkey` - The new hotkey.
    /// * `weight` - The weight of the transaction.
    ///
    pub fn swap_total_hotkey_coldkey_stakes_this_interval(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        weight: &mut Weight,
    ) {
        let stakes: Vec<(T::AccountId, (u64, u64))> =
            TotalHotkeyColdkeyStakesThisInterval::<T>::iter_prefix(old_hotkey).collect();
        log::info!("Stakes to swap: {:?}", stakes);
        for (coldkey, stake) in stakes {
            log::info!(
                "Swapping stake for coldkey: {:?}, stake: {:?}",
                coldkey,
                stake
            );
            TotalHotkeyColdkeyStakesThisInterval::<T>::insert(new_hotkey, &coldkey, stake);
            TotalHotkeyColdkeyStakesThisInterval::<T>::remove(old_hotkey, &coldkey);
            weight.saturating_accrue(T::DbWeight::get().writes(2)); // One write for insert and one for remove
        }
    }

    /// Swaps the total stake associated with a coldkey from the old coldkey to the new coldkey.
    ///
    /// # Arguments
    ///
    /// * `old_coldkey` - The AccountId of the old coldkey.
    /// * `new_coldkey` - The AccountId of the new coldkey.
    /// * `weight` - Mutable reference to the weight of the transaction.
    ///
    /// # Effects
    ///
    /// * Removes the total stake from the old coldkey.
    /// * Inserts the total stake for the new coldkey.
    /// * Updates the transaction weight.
    pub fn swap_total_coldkey_stake(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
        weight: &mut Weight,
    ) {
        let stake = TotalColdkeyStake::<T>::get(old_coldkey);
        TotalColdkeyStake::<T>::remove(old_coldkey);
        TotalColdkeyStake::<T>::insert(new_coldkey, stake);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 2));
    }

    /// Swaps all stakes associated with a coldkey from the old coldkey to the new coldkey.
    ///
    /// # Arguments
    ///
    /// * `old_coldkey` - The AccountId of the old coldkey.
    /// * `new_coldkey` - The AccountId of the new coldkey.
    /// * `weight` - Mutable reference to the weight of the transaction.
    ///
    /// # Effects
    ///
    /// * Removes all stakes associated with the old coldkey.
    /// * Inserts all stakes for the new coldkey.
    /// * Updates the transaction weight.
    pub fn swap_stake_for_coldkey(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
        weight: &mut Weight,
    ) {
        // Find all hotkeys for this coldkey
        let hotkeys = OwnedHotkeys::<T>::get(old_coldkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 0));
        for hotkey in hotkeys.iter() {
            let stake = Stake::<T>::get(&hotkey, old_coldkey);
            Stake::<T>::remove(&hotkey, old_coldkey);
            Stake::<T>::insert(&hotkey, new_coldkey, stake);

            // Update StakingHotkeys map
            let staking_hotkeys = StakingHotkeys::<T>::get(old_coldkey);
            StakingHotkeys::<T>::insert(new_coldkey.clone(), staking_hotkeys);

            weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 3));
        }
    }

    /// Swaps the owner of all hotkeys from the old coldkey to the new coldkey.
    ///
    /// # Arguments
    ///
    /// * `old_coldkey` - The AccountId of the old coldkey.
    /// * `new_coldkey` - The AccountId of the new coldkey.
    /// * `weight` - Mutable reference to the weight of the transaction.
    ///
    /// # Effects
    ///
    /// * Updates the owner of all hotkeys associated with the old coldkey to the new coldkey.
    /// * Updates the transaction weight.
    pub fn swap_owner_for_coldkey(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
        weight: &mut Weight,
    ) {
        let hotkeys = OwnedHotkeys::<T>::get(old_coldkey);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 0));
        for hotkey in hotkeys.iter() {
            Owner::<T>::insert(&hotkey, new_coldkey);
            weight.saturating_accrue(T::DbWeight::get().reads_writes(0, 1));
        }
    }

    /// Swaps the total hotkey-coldkey stakes for the current interval from the old coldkey to the new coldkey.
    ///
    /// # Arguments
    ///
    /// * `old_coldkey` - The AccountId of the old coldkey.
    /// * `new_coldkey` - The AccountId of the new coldkey.
    /// * `weight` - Mutable reference to the weight of the transaction.
    ///
    /// # Effects
    ///
    /// * Removes all total hotkey-coldkey stakes for the current interval associated with the old coldkey.
    /// * Inserts all total hotkey-coldkey stakes for the current interval for the new coldkey.
    /// * Updates the transaction weight.
    pub fn swap_total_hotkey_coldkey_stakes_this_interval_for_coldkey(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
        weight: &mut Weight,
    ) {
        weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 0));
        for hotkey in OwnedHotkeys::<T>::get(old_coldkey).iter() {
            let (stake, block) =
                TotalHotkeyColdkeyStakesThisInterval::<T>::get(&hotkey, old_coldkey);
            TotalHotkeyColdkeyStakesThisInterval::<T>::remove(&hotkey, old_coldkey);
            TotalHotkeyColdkeyStakesThisInterval::<T>::insert(&hotkey, new_coldkey, (stake, block));
            weight.saturating_accrue(T::DbWeight::get().reads_writes(2, 2));
        }
    }

    /// Checks if a coldkey has any associated hotkeys.
    ///
    /// # Arguments
    ///
    /// * `coldkey` - The AccountId of the coldkey to check.
    ///
    /// # Returns
    ///
    /// * `bool` - True if the coldkey has any associated hotkeys, false otherwise.
    pub fn coldkey_has_associated_hotkeys(coldkey: &T::AccountId) -> bool {
        Owner::<T>::iter().any(|(_, owner)| owner == *coldkey)
    }

    /// Swaps the subnet owner from the old coldkey to the new coldkey for all networks where the old coldkey is the owner.
    ///
    /// # Arguments
    ///
    /// * `old_coldkey` - The AccountId of the old coldkey.
    /// * `new_coldkey` - The AccountId of the new coldkey.
    /// * `weight` - Mutable reference to the weight of the transaction.
    ///
    /// # Effects
    ///
    /// * Updates the subnet owner to the new coldkey for all networks where the old coldkey was the owner.
    /// * Updates the transaction weight.
    pub fn swap_subnet_owner_for_coldkey(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
        weight: &mut Weight,
    ) {
        for netuid in 0..=TotalNetworks::<T>::get() {
            let subnet_owner = SubnetOwner::<T>::get(netuid);
            if subnet_owner == *old_coldkey {
                SubnetOwner::<T>::insert(netuid, new_coldkey.clone());
                weight.saturating_accrue(T::DbWeight::get().writes(1));
            }
        }
        weight.saturating_accrue(T::DbWeight::get().reads(TotalNetworks::<T>::get() as u64));
    }

    /// Swaps the owned hotkeys for the coldkey
    ///
    /// # Arguments
    ///
    /// * `old_coldkey` - The AccountId of the old coldkey.
    /// * `new_coldkey` - The AccountId of the new coldkey.
    /// * `weight` - Mutable reference to the weight of the transaction.
    ///
    /// # Effects
    ///
    /// * Updates the subnet owner to the new coldkey for all networks where the old coldkey was the owner.
    /// * Updates the transaction weight.
    pub fn swap_owned_for_coldkey(
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
        weight: &mut Weight,
    ) {
        // Update OwnedHotkeys map with new coldkey
        let hotkeys = OwnedHotkeys::<T>::get(old_coldkey);
        OwnedHotkeys::<T>::remove(old_coldkey);
        OwnedHotkeys::<T>::insert(new_coldkey, hotkeys);
        weight.saturating_accrue(T::DbWeight::get().reads_writes(0, 2));
    }
}
