use crate::native_req::NativeMessageInitialize;
use crate::native_resp::{
    write_native_event, NativeResponse, NativeResponseData, NativeResponseEvent,
    NativeResponseProfileListProfileEntry,
};
use crate::options::native_notify_updated_options;
use crate::profiles::{write_profiles, ProfilesIniState};
use crate::state::AppState;
use std::fs;

pub fn process_cmd_initialize(
    app_state: &mut AppState,
    mut profiles: ProfilesIniState,
    msg: NativeMessageInitialize,
) -> NativeResponse {
    if let Some(profile_id) = &msg.profile_id {
        log::trace!("Profile ID was provided by extension: {}", profile_id);
        finish_init(app_state, &mut profiles, profile_id, msg.extension_id);
        return NativeResponse::success(NativeResponseData::Initialized { cached: true });
    }

    // Extension didn't tell us profile id so we have to determine it
    log::trace!(
        "Profile ID was not provided by extension, determining using ext id ({})",
        msg.extension_id
    );

    // Search every profile
    for profile in &profiles.profile_entries {
        let mut storage_path = profile.full_path(&app_state.config);
        storage_path.push("storage");
        storage_path.push("default");

        let ext_installed = match fs::read_dir(storage_path) {
            Ok(p) => p,
            Err(_) => continue, // Skip profiles that do not have valid storage dir
        }

        .filter_map(|it| match it {
            Ok(entry) => Some(entry),
            Err(_) => None,
        })
        .any(|it| {
            it.file_name()
                .to_string_lossy()
                .starts_with(&("moz-extension+++".to_owned() + &msg.extension_id))
        });

        if ext_installed {
            let profile_id = profile.id.clone();
            log::trace!("Profile ID determined: {}", profile_id);
            finish_init(app_state, &mut profiles, &profile_id, msg.extension_id);
            return NativeResponse::success(NativeResponseData::Initialized { cached: false });
        }
    }

    NativeResponse::error("Unable to detect current profile.")
}

fn finish_init(
    app_state: &mut AppState,
    profiles: &mut ProfilesIniState,
    profile_id: &str,
    internal_ext_id: String,
) {
    app_state.cur_profile_id = Some(profile_id.to_owned());
    app_state.internal_extension_id = Some(internal_ext_id);

    if app_state.first_run {
        app_state.first_run = false;
        log::trace!("First run!");

        match profiles
            .profile_entries
            .iter_mut()
            .find(|p| p.id == profile_id)
        {
            Some(profile) => {
                // Set first-run profile as default
                profile.default = true;
                for other_profile in profiles.profile_entries.iter_mut() {
                    if other_profile.id != profile_id {
                        other_profile.default = false
                    }
                }

                write_profiles(&app_state.config, &app_state.config_dir, profiles);
            }
            None => log::error!(
                "Failed to find first-run profile to set as default: {}",
                profile_id
            ),
        }
    }

    // Notify extension of new profile list
    write_native_event(NativeResponseEvent::ProfileList {
        current_profile_id: profile_id.to_owned(),
        profiles: profiles
            .profile_entries
            .iter()
            .map(NativeResponseProfileListProfileEntry::from_profile_entry)
            .collect(),
    });

    // Notify extension of current options
    native_notify_updated_options(app_state);
}
