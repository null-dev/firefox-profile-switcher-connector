use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::Path;
use eyre::Context;
use serde::{Serialize, Deserialize};
use crate::ipc::notify_update_profile_order;
use crate::native_resp::{NativeResponseEvent, write_native_event};
use crate::profiles::ProfilesIniState;
use crate::state::{AppContext, AppState};
use crate::storage::order_data_path;

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct OrderData {
    pub order: Vec<String>
}

impl OrderData {
    /// Re-calculate the `profile_order` array, removing any profiles that are no longer present and
    /// adding any new profiles.
    pub fn recalculate(&mut self, profiles: &ProfilesIniState) {
        let mut profile_indicies: HashMap<&str, usize> = HashMap::new();
        for (idx, profile_id) in self.order.iter().enumerate() {
            profile_indicies.insert(profile_id, idx);
        }
        let profile_idx = |id: &str| profile_indicies.get(id)
            .copied()
            .unwrap_or(usize::MAX); // Profiles that we haven't defined an order for should be put near the end
        // This will also preserve creation order since the sort is stable
        let mut new_profile_order: Vec<String> = profiles.profile_entries.iter()
            .map(|p| p.id.clone())
            .collect();
        new_profile_order.sort_by_key(|id| profile_idx(id));
        self.order = new_profile_order;
    }

    /// Performs the following operations in sequence:
    /// - read
    /// - recalculate
    /// - write
    /// - ipc notify profile order updated
    /// Will log if the write fails.
    pub fn try_rewrite(context: &AppContext, profiles: &ProfilesIniState) {
        let mut order_data = Self::read(&context.state.config_dir);
        order_data.recalculate(profiles);
        if let Err(e) = order_data.write(&context.state.config_dir) {
            log::error!("Failed to update profiles order: {:?}", e);
        } else {
            notify_update_profile_order(context, profiles);
        }
    }

    pub fn read(config_dir: &Path) -> OrderData {
        OpenOptions::new()
            .read(true)
            .open(order_data_path(config_dir))
            .context("could not open profile order data file")
            .and_then(|f| serde_json::from_reader(f)
                .context("profile order data file is incorrectly formatted"))
            .unwrap_or_else(|e| {
                log::warn!("Failed to read options data: {:?}, falling back to defaults", e);
                OrderData::default()
            })
    }

    pub fn write(&self, config_dir: &Path) -> eyre::Result<()> {
        // Write order data
        let order_file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(order_data_path(config_dir))
            .context("failed to open profile order data file for writing")?;

        serde_json::to_writer(order_file, &self)
            .context("failed to write profile order data to file")
    }
}

pub fn native_notify_updated_profile_order(app_state: &AppState) {
    write_native_event(NativeResponseEvent::ProfileOrderUpdated {
        order: OrderData::read(&app_state.config_dir).order
    });
}
