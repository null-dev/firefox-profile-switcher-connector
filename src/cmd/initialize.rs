use crate::state::AppState;
use crate::profiles::{ProfilesIniState, write_profiles};
use crate::native_req::NativeMessageInitialize;
use crate::native_resp::{NativeResponse, NativeResponseData, NativeResponseEvent, NativeResponseProfileListProfileEntry, write_native_event};
use std::{thread, fs};
use libc::abort;
use crate::ipc::setup_ipc;
use crate::options::native_notify_updated_options;

pub fn process_cmd_initialize(app_state: &mut AppState,
                              profiles: &mut ProfilesIniState,
                              msg: NativeMessageInitialize) -> NativeResponse {
    /*if let Some(profile_id) = &msg.profile_id {
        log::trace!("Profile ID was provided by extension: {}", profile_id);
        finish_init(app_state, profiles, profile_id, msg.extension_id);
        return NativeResponse::success(NativeResponseData::Initialized { cached: true })
    }*/

    // Extension didn't tell us profile id so we have to determine it
    log::trace!("Determining profile ID using ext id ({})", msg.extension_id);

    // Search every profile
    for profile in &profiles.profile_entries {
        let mut storage_path = profile.full_path(&app_state.config);
        log::trace!("\tSearching profile: {} (is_relative: {}, path: {})", profile.name, profile.is_relative, profile.path);
        storage_path.push("storage");
        storage_path.push("default");
        log::trace!("\tListing contents of storage dir: {}", storage_path.display());

        let profile_dir = match fs::read_dir(storage_path) {
            Ok(p) => p,
            Err(err) => {
                log::trace!("\tFailed to list contents of storage dir: {:?}", err);
                continue
            }
        };
        let mut ext_installed = false;
        for profile in profile_dir {
            match profile {
                Ok(dir) => {
                    let folder_name_os = dir.file_name();
                    let folder_name = folder_name_os.to_string_lossy();
                    if folder_name.starts_with("moz") {
                        log::trace!("\t\t{}", folder_name);
                    }
                    if folder_name.starts_with(&("moz-extension+++".to_owned() + &msg.extension_id)) {
                        log::trace!("\t\t\t^This extension matches our extension ID!");
                        ext_installed = true;
                    }
                }
                Err(err) => {
                    log::trace!("\t\tERROR: {:?}", err);
                }
            }
        }

        if ext_installed {
            let profile_id = profile.id.clone();
            log::trace!("Profile ID determined: {}", profile_id);
            finish_init(app_state, profiles, &profile_id, msg.extension_id);
            log::trace!("Aborting as this is a debug build not meant for actual use!");
            panic!("Aborting as this is a debug build!");
            return NativeResponse::success(NativeResponseData::Initialized { cached: false })
        }
    }

    log::trace!("Aborting as this is a debug build not meant for actual use!");
    panic!("Aborting as this is a debug build!");
    return NativeResponse::error("Unable to detect current profile.")
}

fn finish_init(app_state: &mut AppState, profiles: &mut ProfilesIniState, profile_id: &str, internal_ext_id: String) {
    app_state.cur_profile_id = Some(profile_id.to_owned());
    app_state.internal_extension_id = Some(internal_ext_id);

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

