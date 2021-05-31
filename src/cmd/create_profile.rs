use crate::state::AppState;
use serde_json::Value;
use crate::profiles::{ProfilesIniState, ProfileEntry, calc_profile_id, write_profiles};
use crate::native_req::NativeMessageCreateProfile;
use crate::native_resp::{NativeResponse, NativeResponseProfileListProfileEntry, NativeResponseData};
use ulid::Ulid;
use std::collections::HashMap;
use std::fs;
use std::fs::OpenOptions;
use std::path::PathBuf;
use crate::ipc::notify_profile_changed;
use serde::Serialize;
use std::ops::Add;

fn find_extension_chunk<'a>(app_state: &AppState, json: &'a Value) -> Option<&'a serde_json::Map<String, Value>> {
    if let Some(our_extension_id) = &app_state.extension_id {
        if let Value::Object(json) = json {
            if let Some(Value::Array(addons)) = json.get("addons") {
                return addons.iter()
                    .find_map(|addon| {
                        if let Value::Object(addon) = addon {
                            if let Some(Value::String(addon_id)) = addon.get("id") {
                                if addon_id == our_extension_id {
                                    return Some(addon)
                                }
                            }
                        }

                        None
                    })
            }
        }
    }

    return None
}

#[derive(Serialize)]
struct ExtensionsJson {
    #[serde(rename = "schemaVersion")]
    schema_version: i64,
    addons: Vec<Value>
}

impl ExtensionsJson {
    fn from_extension_chunk(chunk: serde_json::Map<String, Value>) -> Self {
        ExtensionsJson {
            schema_version: 33,
            addons: [Value::Object(chunk)].to_vec()
        }
    }
}

pub fn process_cmd_create_profile(app_state: &AppState, profiles: &mut ProfilesIniState, msg: NativeMessageCreateProfile) -> NativeResponse {
    // TODO Inject extension into new profiles

    let new_trimmed_name = msg.name.trim();
    let name_conflict = profiles.profile_entries.iter().any(|p| p.name.trim().eq_ignore_ascii_case(new_trimmed_name));

    if name_conflict {
        return NativeResponse::error("A profile with this name already exists. Please choose another name.");
    }

    let new_profile_path = "profile-".to_owned() + &Ulid::new().to_string();

    let new_profile = ProfileEntry {
        id: calc_profile_id(&new_profile_path, true),
        name: new_trimmed_name.to_owned(),
        is_relative: true,
        path: new_profile_path,
        default: false,
        avatar: Some(msg.avatar),
        options: HashMap::new()
    };

    // Firefox will refuse to launch if we do not mkdirs the new profile folder
    let new_profile_full_path = new_profile.full_path(&app_state.config);
    if let Err(e) = fs::create_dir_all(&new_profile_full_path) {
        return NativeResponse::error_with_dbg_msg("Failed to folder for new profile!", e);
    }

    // Read current extensions JSON
    // TODO Extract this into function to fix this huge if-let chain
    {
        if let Some(our_profile) = profiles.profile_entries.iter().find(|p| Some(&p.id) == app_state.cur_profile_id.as_ref()) {
            let mut extensions_path = our_profile.full_path(&app_state.config);
            extensions_path.push("extensions.json");
            if let Ok(extensions_file) = OpenOptions::new()
                .read(true)
                .open(extensions_path) {
                if let Ok(extensions_json) = serde_json::from_reader(extensions_file) {
                    if let Some(mut extension_chunk) = find_extension_chunk(app_state, &extensions_json).cloned() {
                        let mut old_extension_path: Option<PathBuf> = None;
                        let mut new_extension_path: Option<PathBuf> = None;

                        // Rewrite extension path
                        if let serde_json::map::Entry::Occupied(mut path_entry) = extension_chunk.entry("path") {
                            if let Value::String(path) = path_entry.get() {
                                let extension_path = PathBuf::from(path);
                                if let Some(extension_filename) = extension_path.file_name() {
                                    let mut new_extension_path_builder = new_profile.full_path(&app_state.config);
                                    new_extension_path_builder.push("extensions");
                                    new_extension_path_builder.push(extension_filename);
                                    path_entry.insert(Value::String(new_extension_path_builder.to_string_lossy().to_string()));
                                    new_extension_path = Some(new_extension_path_builder);
                                }
                                old_extension_path = Some(extension_path);
                            }
                        }

                        if let Some(new_extension_path) = new_extension_path {
                            // Rewrite rootURI path
                            if let serde_json::map::Entry::Occupied(mut root_uri_entry) = extension_chunk.entry("rootURI") {
                                if let Value::String(_) = root_uri_entry.get() {
                                    let mut new_root_uri = url::Url::parse("file://").unwrap();
                                    new_root_uri.set_path(&new_extension_path.to_string_lossy());
                                    let mut new_root_uri = new_root_uri.into_string();
                                    new_root_uri.insert_str(0, "jar:");
                                    new_root_uri = new_root_uri.add("!/");
                                    root_uri_entry.insert(Value::String(new_root_uri));
                                }
                            }


                            // Now we have a valid extension chunk, let's create a new extensions.json with it
                            let extensions_json_content = ExtensionsJson::from_extension_chunk(extension_chunk);

                            // Write extension chunk
                            let mut extensions_json_path = new_profile_full_path.clone();
                            extensions_json_path.push("extensions.json");
                            match OpenOptions::new()
                                .create_new(true)
                                .write(true)
                                .open(extensions_json_path) {
                                Ok(file) => {
                                    if let Err(err) = serde_json::to_writer(file, &extensions_json_content) {
                                        log::error!("Failed to serialize new extensions JSON: {:?}", err);
                                    }
                                },
                                Err(err) => log::error!("Failed to open new extensions JSON: {:?}", err)
                            }

                            // Copy extension file
                            if let Some(extension_parent_dir) = new_extension_path.parent() {
                                fs::create_dir_all(extension_parent_dir);
                            }
                            if let Some(old_extension_path) = old_extension_path {
                                if let Err(err) = fs::copy(old_extension_path, new_extension_path) {
                                    log::error!("Failed to copy extension to new profile: {:?}", err);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let resp = NativeResponseProfileListProfileEntry {
        id: new_profile.id.clone(),
        name: new_profile.name.clone(),
        default: new_profile.default,
        avatar: new_profile.avatar.clone(),
        options: new_profile.options.clone()
    };
    profiles.profile_entries.push(new_profile);

    if let Err(e) = write_profiles(&app_state.config, &app_state.config_dir, profiles) {
        return NativeResponse::error_with_dbg_msg("Failed to save new changes!", e);
    }
    notify_profile_changed(app_state, profiles);

    return NativeResponse::success(NativeResponseData::ProfileCreated { profile: resp })
}
