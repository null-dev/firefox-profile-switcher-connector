use std::collections::{HashMap, HashSet};
use crate::ipc::notify_update_profile_order;
use crate::native_req::{NativeMessageLaunchProfile, NativeMessageUpdateProfileOrder};
use crate::native_resp::{NativeResponse, NativeResponseData};
use crate::profiles::ProfilesIniState;
use crate::profiles_order::{native_notify_updated_profile_order, OrderData};
use crate::state::AppContext;

pub fn process_cmd_update_profiles_order(context: &AppContext,
                                         profiles: ProfilesIniState,
                                         msg: NativeMessageUpdateProfileOrder) -> NativeResponse {
    let new_order_data = OrderData { order: msg.order };
    let mut profile_map: HashSet<&str> = profiles.profile_entries.iter()
        .map(|x| x.id.as_str())
        .collect();
    for profile_id in &new_order_data.order {
        // Will also catch dupes if we remove them as we go
        if !profile_map.remove(profile_id.as_str()) {
            return NativeResponse::error("Attempted to re-arrange profile that does not exist!");
        }
    }
    if let Err(e) = new_order_data.write(&context.state.config_dir) {
        return NativeResponse::error_with_dbg_msg("Could not save profile order.", e);
    }

    notify_update_profile_order(context, &profiles);

    NativeResponse::success(NativeResponseData::ProfileOrderUpdated)
}
