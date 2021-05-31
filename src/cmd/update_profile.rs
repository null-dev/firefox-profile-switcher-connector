use crate::state::AppState;
use crate::profiles::{ProfilesIniState, write_profiles};
use crate::native_req::NativeMessageUpdateProfile;
use crate::native_resp::{NativeResponse, NativeResponseProfileListProfileEntry, NativeResponseData};
use crate::ipc::notify_profile_changed;

pub fn process_cmd_update_profile(app_state: &AppState, profiles: &mut ProfilesIniState, msg: NativeMessageUpdateProfile) -> NativeResponse {
    let new_trimmed_name = msg.name.trim();
    let name_conflict = profiles.profile_entries.iter()
        .filter(|p| p.id != msg.profile_id)
        .any(|p| p.name.trim().eq_ignore_ascii_case(new_trimmed_name));

    if name_conflict {
        return NativeResponse::error("A profile with this name already exists. Please choose another name.");
    }

    let profile = match profiles.profile_entries.iter_mut().find(|p| p.id == msg.profile_id) {
        Some(p) => p,
        None => return NativeResponse::error("No profile with the specified id could be found!")
    };

    profile.name = msg.name;
    profile.avatar = msg.avatar;
    profile.options = msg.options;

    if msg.default {
        profile.default = true
    }

    let resp = NativeResponseProfileListProfileEntry {
        id: msg.profile_id.clone(),
        name: profile.name.clone(),
        default: profile.default,
        avatar: profile.avatar.clone(),
        options: profile.options.clone()
    };

    if msg.default {
        for profile in profiles.profile_entries.iter_mut() {
            if profile.id != msg.profile_id {
                profile.default = false
            }
        }
    }

    if let Err(e) = write_profiles(&app_state.config, &app_state.config_dir, profiles) {
        return NativeResponse::error_with_dbg_msg("Failed to save new changes!", e);
    }
    notify_profile_changed(app_state, profiles);

    return NativeResponse::success(NativeResponseData::ProfileUpdated { profile: resp })
}

