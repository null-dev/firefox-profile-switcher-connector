use std::collections::HashMap;
use serde_json::Value;
use serde::{Deserialize, Serialize};
use crate::config::Config;
use std::path::{PathBuf, Path};
use ini::Ini;
use std::io;
use std::fs::OpenOptions;
use crate::storage::{avatar_data_path, options_data_path};
use ring::digest::{Context, SHA256};
use data_encoding::HEXUPPER;

// === PROFILE ===
pub struct ProfileEntry {
    pub id: String,
    pub name: String,
    pub is_relative: bool,
    pub path: String,
    pub default: bool,
    pub avatar: Option<String>,
    pub options: HashMap<String, Value>
}

impl ProfileEntry {
    pub fn full_path(&self, config: &Config) -> PathBuf {
        return if self.is_relative {
            let mut result = config.browser_profile_dir().clone();
            result.push(&self.path);
            result
        } else {
            PathBuf::from(&self.path)
        }
    }
}

pub struct ProfilesIniState {
    backing_ini: Ini,
    pub profile_entries: Vec<ProfileEntry>
}

#[derive(Serialize, Deserialize, Debug)]
struct AvatarData {
    avatars: HashMap<String, String>
}

#[derive(Serialize, Deserialize, Debug)]
struct OptionsData {
    options: HashMap<String, HashMap<String, Value>>
}

#[derive(Debug)]
pub enum ReadProfilesError {
    BadIniFormat,
    IniError(ini::Error),
    AvatarStoreError(io::Error),
    BadAvatarStoreFormat(serde_json::Error),
    OptionsStoreError(io::Error),
    BadOptionsStoreFormat(serde_json::Error)
}

pub fn read_profiles(config: &Config, config_dir: &Path) -> Result<ProfilesIniState, ReadProfilesError> {
    let profiles_conf = Ini::load_from_file(config.profiles_ini_path())
        .map_err(ReadProfilesError::IniError)?;

    let avatar_data: AvatarData = OpenOptions::new()
        .read(true)
        .open(avatar_data_path(config_dir))
        .map_err(ReadProfilesError::AvatarStoreError)
        .and_then(|f| serde_json::from_reader(f)
            .map_err(ReadProfilesError::BadAvatarStoreFormat))
        .unwrap_or_else(|e| {
            log::warn!("Failed to read avatar data: {:?}, falling back to defaults", e);
            AvatarData { avatars: HashMap::new() }
        });

    let options_data: OptionsData = OpenOptions::new()
        .read(true)
        .open(options_data_path(config_dir))
        .map_err(ReadProfilesError::OptionsStoreError)
        .and_then(|f| serde_json::from_reader(f)
            .map_err(ReadProfilesError::BadOptionsStoreFormat))
        .unwrap_or_else(|e| {
            log::warn!("Failed to read options data: {:?}, falling back to defaults", e);
            OptionsData { options: HashMap::new() }
        });

    let mut state = ProfilesIniState {
        backing_ini: Ini::new(),
        profile_entries: Vec::new()
    };

    for (sec, prop) in &profiles_conf {
        if sec.is_none() || !sec.unwrap().starts_with("Profile") {
            // Save non-profile keys in new INI file
            let mut section_setter = &mut state.backing_ini.with_section(sec);
            for (key, value) in prop.iter() {
                section_setter = section_setter.set(key, value);
            }
        } else {
            // Parse profile keys
            let mut profile_name = None::<String>;
            let mut profile_is_relative = None::<bool>;
            let mut profile_path = None::<String>;
            let mut profile_default = false;

            for (key, value) in prop.iter() {
                match key {
                    "Name" => profile_name = Some(value.to_owned()),
                    "IsRelative" => profile_is_relative = Some(value == "1"),
                    "Path" => profile_path = Some(value.to_owned()),
                    "Default" => profile_default = value == "1",
                    _ => {}
                }
            }

            if profile_name.is_none() || profile_path.is_none() || profile_is_relative.is_none() {
                return Err(ReadProfilesError::BadIniFormat)
            }

            let profile_path = profile_path.unwrap();
            let profile_is_relative = profile_is_relative.unwrap();
            let profile_id = calc_profile_id(&profile_path, profile_is_relative);
            let avatar = avatar_data.avatars.get(&profile_id).map(String::clone);
            let options = options_data.options
                .get(&profile_id)
                .map(HashMap::clone)
                .unwrap_or_else(HashMap::new);

            state.profile_entries.push(ProfileEntry {
                id: profile_id,
                name: profile_name.unwrap(),
                is_relative: profile_is_relative,
                path: profile_path,
                default: profile_default,
                avatar,
                options
            });
        }
    }

    Ok(state)
}

#[derive(Debug)]
pub enum WriteProfilesError {
    WriteIniError(io::Error),
    OpenAvatarFileError(io::Error),
    WriteAvatarFileError(serde_json::Error),
    OpenOptionsFileError(io::Error),
    WriteOptionsFileError(serde_json::Error)
}
pub fn write_profiles(config: &Config, config_dir: &Path, state: &ProfilesIniState) -> Result<(), WriteProfilesError> {
    // Build avatar data
    let mut avatar_data = AvatarData {
        avatars: HashMap::new()
    };
    let mut options_data = OptionsData {
        options: HashMap::new()
    };
    for profile in &state.profile_entries {
        if let Some(avatar) = &profile.avatar {
            avatar_data.avatars.insert(profile.id.clone(), avatar.clone());
        }
        options_data.options.insert(profile.id.clone(), profile.options.clone());
    }

    // Write avatar data
    let avatar_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(avatar_data_path(config_dir))
        .map_err(WriteProfilesError::OpenAvatarFileError)?;

    serde_json::to_writer(avatar_file, &avatar_data)
        .map_err(WriteProfilesError::WriteAvatarFileError)?;

    // Write options data
    let options_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(options_data_path(config_dir))
        .map_err(WriteProfilesError::OpenOptionsFileError)?;

    serde_json::to_writer(options_file, &options_data)
        .map_err(WriteProfilesError::WriteOptionsFileError)?;

    // Write profile data
    let mut new_ini = state.backing_ini.clone();

    let mut default_profile_path = None::<&str>;
    for (i, profile) in state.profile_entries.iter().enumerate() {
        let mut section = &mut new_ini.with_section(Some("Profile".to_owned() + &i.to_string()));
        section = section.set("Name", profile.name.as_str())
            .set("IsRelative", if profile.is_relative { "1" } else { "0" })
            .set("Path", profile.path.as_str());
        if profile.default {
            section.set("Default", "1");
            default_profile_path = Some(&profile.path);
        }
    }

    if let Some(default_profile_path) = default_profile_path {
        for (sec, prop) in &mut new_ini {
            if let Some(sec) = sec {
                if sec.starts_with("Install") && prop.contains_key("Default") {
                    prop.insert("Default", default_profile_path);
                    prop.insert("Locked", "0");
                }
            }
        }
    }

    if let Err(e) = new_ini.write_to_file(config.profiles_ini_path()) {
        return Err(WriteProfilesError::WriteIniError(e))
    }

    // Write install INI
    if let Some(default_profile_path) = default_profile_path {
        let installs_conf = Ini::load_from_file(config.installs_ini_path());
        if let Ok(mut installs_conf) = installs_conf {
            for (sec, prop) in &mut installs_conf {
                if let Some(sec) = sec {
                    if prop.contains_key("Default") {
                        prop.insert("Default", default_profile_path);
                        prop.insert("Locked", "0");
                    }
                }
            }
            if let Err(e) = installs_conf.write_to_file(config.installs_ini_path()) {
                log::warn!("Failed to write installs.ini: {:?}", e);
            }
        }
    }

    Ok(())
}

pub fn calc_profile_id(path: &str, is_relative: bool) -> String {
    let mut context = Context::new(&SHA256);
    context.update(&[is_relative as u8]);
    context.update(path.as_bytes());
    return HEXUPPER.encode(context.finish().as_ref());
}
