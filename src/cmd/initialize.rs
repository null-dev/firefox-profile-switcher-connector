use crate::state::AppState;
use crate::profiles::{ProfilesIniState, write_profiles};
use crate::native_req::NativeMessageInitialize;
use crate::native_resp::{NativeResponse, NativeResponseData, write_native_response, NativeResponseWrapper, NATIVE_RESP_ID_EVENT, NativeResponseEvent, NativeResponseProfileListProfileEntry, write_native_event};
use std::{thread, fs};
use crate::ipc::setup_ipc;
use crate::options::native_notify_updated_options;

pub fn process_cmd_initialize(app_state: &mut AppState,
                              profiles: &mut ProfilesIniState,
                              msg: NativeMessageInitialize) -> NativeResponse {
    if let Some(profile_id) = &msg.profile_id {
        log::trace!("Profile ID was provided by extension: {}", profile_id);
        finish_init(app_state, profiles, profile_id);
        return NativeResponse::success(NativeResponseData::Initialized { cached: true })
    }

    // Extension didn't tell us profile id so we have to determine it
    log::trace!("Profile ID was not provided by extension, determining using ext id ({})", msg.extension_id);

    // Search every profile
    for profile in &profiles.profile_entries {
        let mut storage_path = profile.full_path(&app_state.config);
        storage_path.push("storage");
        storage_path.push("default");

        let ext_installed = match fs::read_dir(storage_path) {
            Ok(p) => p,
            Err(_) => continue // Skip profiles that do not have valid storage dir
        }.filter_map(|it| match it {
            Ok(entry) => Some(entry),
            Err(_) => None
        }).any(|it| it.file_name()
            .to_string_lossy()
            .starts_with(&("moz-extension+++".to_owned() + &msg.extension_id))
        );

        if ext_installed {
            let profile_id = profile.id.clone();
            log::trace!("Profile ID determined: {}", profile_id);
            finish_init(app_state, profiles, &profile_id);
            return NativeResponse::success(NativeResponseData::Initialized { cached: false })
        }
    }

    return NativeResponse::error("Unable to detect current profile.")
}

fn finish_init(app_state: &mut AppState, profiles: &mut ProfilesIniState, profile_id: &str) {
    app_state.cur_profile_id = Some(profile_id.to_owned());

    if app_state.first_run {
        app_state.first_run = false;
        log::trace!("First run!");

        match profiles.profile_entries.iter_mut().find(|p| p.id == profile_id) {
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
            None => log::error!("Failed to find first-run profile to set as default: {}", profile_id)
        }
    }

    // Notify extension of new profile list
    write_native_event(NativeResponseEvent::ProfileList {
        current_profile_id: profile_id.to_owned(),
        profiles: profiles.profile_entries.iter().map(NativeResponseProfileListProfileEntry::from_profile_entry).collect()
    });

    // Notify extension of current options
    native_notify_updated_options(app_state);

    // Begin IPC
    {
        let profile_id = profile_id.to_owned();
        let app_state = app_state.clone();
        thread::spawn(move || {
            if let Err(e) = setup_ipc(&profile_id, &app_state) {
                log::error!("Failed to setup IPC server: {:?}", e);
            }
        });
    }
}

