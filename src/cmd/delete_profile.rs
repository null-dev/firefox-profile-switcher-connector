use crate::profiles::{check_profile_active, ProfilesIniState, write_profiles};
use crate::native_req::NativeMessageDeleteProfile;
use crate::native_resp::{NativeResponse, NativeResponseData};
use crate::ipc::notify_profile_changed;
use std::fs;
use crate::AppContext;
use crate::profiles_order::OrderData;

pub fn process_cmd_delete_profile(context: &AppContext, mut profiles: ProfilesIniState, msg: NativeMessageDeleteProfile) -> NativeResponse {
    let profile_index = match profiles.profile_entries.iter().position(|p| p.id == msg.profile_id) {
        Some(p) => p,
        None => return NativeResponse::error("No profile with the specified id could be found!")
    };

    // Delete profile from profile list (but do not write new list yet)
    let profile = profiles.profile_entries.remove(profile_index);

    let profile_path = profile.full_path(&context.state.config);

    // Check that profile is closed
    if check_profile_active(&profile_path) {
        return NativeResponse::error(
            concat!(
            "This profile is in use and therefore cannot be deleted, close the profile and try again.\n\n",
            "Alternatively, your browser may have crashed the last time you used this profile and the profile was never properly shut down, ",
            "you can try opening and closing the profile to resolve this issue."
            )
        )
    }

    // Delete profile files
    fs::remove_dir_all(profile_path);

    // Make another profile the default
    if profile.default {
        if let Some(new_def_profile) = profiles.profile_entries.first_mut() {
            new_def_profile.default = true
        }
    }

    // Re-calculate profile order
    OrderData::try_rewrite(context, &profiles);

    // Write new profile list
    if let Err(e) = write_profiles(&context.state.config, &context.state.config_dir, &profiles) {
        return NativeResponse::error_with_dbg_msg("Failed to save new changes!", e);
    }
    notify_profile_changed(context, &profiles);

    return NativeResponse::success(NativeResponseData::ProfileDeleted)
}

