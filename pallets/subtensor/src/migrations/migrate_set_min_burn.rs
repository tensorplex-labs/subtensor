use alloc::string::String;

use frame_support::IterableStorageMap;
use frame_support::{traits::Get, weights::Weight};

use super::*;

pub fn migrate_set_min_burn<T: Config>() -> Weight {
    let migration_name = b"migrate_set_min_burn_1".to_vec();

    // Initialize the weight with one read operation.
    let mut weight = T::DbWeight::get().reads(1);

    // Check if the migration has already run
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{:?}' has already run. Skipping.",
            migration_name
        );
        return weight;
    }
    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    let netuids: Vec<u16> = <NetworksAdded<T> as IterableStorageMap<u16, bool>>::iter()
        .map(|(netuid, _)| netuid)
        .collect();
    weight = weight.saturating_add(T::DbWeight::get().reads(netuids.len() as u64));

    for netuid in netuids.iter().clone() {
        if *netuid == 0 {
            continue;
        }
        // Set min burn to the newest initial min burn
        Pallet::<T>::set_min_burn(*netuid, T::InitialMinBurn::get());
        weight = weight.saturating_add(T::DbWeight::get().writes(1));
    }

    // Mark the migration as completed
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed.",
        String::from_utf8_lossy(&migration_name)
    );

    // Return the migration weight.
    weight
}
