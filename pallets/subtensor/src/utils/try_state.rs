use frame_support::traits::fungible::Inspect;

use super::*;

impl<T: Config> Pallet<T> {
    /// Checks [`TotalIssuance`] equals the sum of currency issuance, total stake, and total subnet
    /// locked.
    pub(crate) fn check_total_issuance() -> Result<(), sp_runtime::TryRuntimeError> {
        // Get the total subnet locked amount
        let total_subnet_locked = Self::get_total_subnet_locked();

        // Get the total currency issuance
        let currency_issuance = T::Currency::total_issuance();

        // Calculate the expected total issuance
        let expected_total_issuance = currency_issuance
            .saturating_add(TotalStake::<T>::get())
            .saturating_add(total_subnet_locked);

        // Verify the diff between calculated TI and actual TI is less than delta
        //
        // These values can be off slightly due to float rounding errors.
        // They are corrected every runtime upgrade.
        let delta = 1000;
        let total_issuance = TotalIssuance::<T>::get();

        let diff = if total_issuance > expected_total_issuance {
            total_issuance.checked_sub(expected_total_issuance)
        } else {
            expected_total_issuance.checked_sub(total_issuance)
        }
        .expect("LHS > RHS");

        ensure!(
            diff <= delta,
            "TotalIssuance diff greater than allowable delta",
        );

        Ok(())
    }

    /// Checks the sum of all stakes matches the [`TotalStake`].
    #[allow(dead_code)]
    pub(crate) fn check_total_stake() -> Result<(), sp_runtime::TryRuntimeError> {
        // Calculate the total staked amount
        let total_staked = SubnetTAO::<T>::iter().fold(0u64, |acc, (netuid, stake)| {
            let acc = acc.saturating_add(stake);

            if netuid == Self::get_root_netuid() {
                // root network doesn't have initial pool TAO
                acc
            } else {
                acc.saturating_sub(Self::get_network_min_lock())
            }
        });

        log::warn!(
            "total_staked: {}, TotalStake: {}",
            total_staked,
            TotalStake::<T>::get()
        );

        // Verify that the calculated total stake matches the stored TotalStake
        ensure!(
            total_staked == TotalStake::<T>::get(),
            "TotalStake does not match total staked",
        );

        Ok(())
    }
}
