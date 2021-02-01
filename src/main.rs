extern crate ini;
extern crate serde;
extern crate serde_json;
extern crate byteorder;
extern crate directories;
extern crate fs2;
extern crate cfg_if;
extern crate ring;
extern crate data_encoding;
extern crate ulid;
extern crate libc;
extern crate interprocess;
extern crate fern;
extern crate log;
extern crate url;

use std::path::{PathBuf, Path};
use ini::Ini;
use std::{io, env, process, thread};
use std::io::{Read, Error, Write};
use byteorder::{ReadBytesExt, NativeEndian, WriteBytesExt, NetworkEndian};
use serde::{Deserialize, Serialize};
use std::fs;
use directories::ProjectDirs;
use std::fs::{OpenOptions, File};
use fs2::FileExt;
use std::fmt::Debug;
use cfg_if::cfg_if;
use ring::digest::{Context, SHA256};
use data_encoding::{HEXUPPER};
use std::process::{Command, Child, exit, Stdio};
use ulid::Ulid;
use interprocess::local_socket::{LocalSocketListener, ToLocalSocketName, LocalSocketStream};
use std::sync::Mutex;
use std::iter::Map;
use std::collections::HashMap;
use serde_json::Value;
use std::ops::Add;
use crate::GetParentProcError::NoCrashReporterEnvVar;
use std::env::VarError;

cfg_if! {
    if #[cfg(target_family = "unix")] {
        extern crate nix;

        use nix::unistd::ForkResult;
        use nix::sys::wait::waitpid;
    } else if #[cfg(target_family = "windows")] {
    } else {
        compile_error!("Unknown OS!");
    }
}

const APP_VERSION: &'static str = env!("CARGO_PKG_VERSION");

// === NATIVE REQUEST ===
#[derive(Serialize, Deserialize)]
struct NativeMessageCreateProfile {
    name: String,
    avatar: String
}

#[derive(Serialize, Deserialize)]
struct NativeMessageLaunchProfile {
    profile_id: String
}

#[derive(Serialize, Deserialize)]
struct NativeMessageDeleteProfile {
    profile_id: String
}

#[derive(Serialize, Deserialize)]
struct NativeMessageUpdateProfile {
    profile_id: String,
    name: String,
    avatar: String
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "command")]
enum NativeMessage {
    LaunchProfile(NativeMessageLaunchProfile),
    CreateProfile(NativeMessageCreateProfile),
    DeleteProfile(NativeMessageDeleteProfile),
    UpdateProfile(NativeMessageUpdateProfile),
    CloseManager
}

#[derive(Serialize, Deserialize)]
struct NativeMessageWrapper {
    id: i64,
    msg: NativeMessage
}
fn read_incoming_message(input: &mut impl Read) -> NativeMessageWrapper {
    // Read size of incoming message
    let size = input.read_u32::<NativeEndian>()
        .expect("Failed to read native message size!");

    // Read and deserialize
    let mut conf_buffer = vec![0u8; size as usize];
    input.read_exact(&mut conf_buffer)
        .expect("Failed to read native message!");
    serde_json::from_slice(&conf_buffer)
        .expect("Failed to deserialize native message!")
}

// === NATIVE RESPONSE ===

#[derive(Serialize)]
#[serde(untagged)]
enum NativeResponse {
    Error {
        success: bool,
        error: String,
        debug_msg: Option<String>
    },
    Success {
        success: bool,
        #[serde(flatten)]
        data: NativeResponseData
    },
    Event(NativeResponseEvent)
}

const NATIVE_RESP_ID_EVENT: i64 = -1;

#[derive(Serialize)]
struct NativeResponseWrapper {
    id: i64,
    resp: NativeResponse
}

impl NativeResponse {
    pub fn error(msg: &str) -> NativeResponse {
        NativeResponse::Error {
            success: false,
            error: String::from(msg),
            debug_msg: None
        }
    }
    pub fn error_with_dbg_msg(msg: &str, err: impl Debug) -> NativeResponse {
        NativeResponse::Error {
            success: false,
            error: String::from(msg),
            debug_msg: Some(format!("{:?}", err))
        }
    }
    pub fn error_with_dbg_str(msg: &str, err: String) -> NativeResponse {
        NativeResponse::Error {
            success: false,
            error: String::from(msg),
            debug_msg: Some(err)
        }
    }
    pub fn success(data: NativeResponseData) -> NativeResponse {
        NativeResponse::Success {
            success: true,
            data
        }
    }
    pub fn event(event: NativeResponseEvent) -> NativeResponse {
        NativeResponse::Event(event)
    }
}

#[derive(Serialize)]
struct NativeResponseProfileListProfileEntry {
    id: String,
    name: String,
    default: bool,
    avatar: Option<String>
}

impl NativeResponseProfileListProfileEntry {
    fn from_profile_entry(entry: &ProfileEntry) -> NativeResponseProfileListProfileEntry {
        NativeResponseProfileListProfileEntry {
            id: entry.id.clone(),
            name: entry.name.clone(),
            default: entry.default,
            avatar: entry.avatar.clone()
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum NativeResponseData {
    ProfileLaunched,
    ProfileCreated {
        profile: NativeResponseProfileListProfileEntry
    },
    ProfileUpdated {
        profile: NativeResponseProfileListProfileEntry
    },
    ProfileDeleted,
    ManagerClosed
}

#[derive(Serialize)]
#[serde(tag = "event")]
enum NativeResponseEvent {
    ProfileList { current_profile_id: String, profiles: Vec<NativeResponseProfileListProfileEntry> },
    FocusWindow,
    CloseManager,
    ConnectorInformation { version: String }
}

fn write_native_response(resp: NativeResponseWrapper) {
    let serialized = serde_json::to_vec(&resp).unwrap();
    // TODO Handle error
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    handle.write_u32::<NativeEndian>(serialized.len() as u32);
    handle.write_all(&serialized);
    handle.flush();
}

// === CONFIG ===

#[derive(Serialize, Deserialize, Clone)]
struct Config {
    browser_profile_dir: PathBuf
}

impl Config {
    fn profiles_ini_path(&self) -> PathBuf {
        let mut profiles_ini = self.browser_profile_dir.clone();
        profiles_ini.push("profiles.ini");
        return profiles_ini;
    }
}

fn get_default_browser_profile_folder() -> PathBuf {
    let user_dirs = directories::UserDirs::new()
        .expect("Unable to determine user folder!");

    let mut result = user_dirs.home_dir().to_path_buf();
    cfg_if! {
        if #[cfg(target_os = "linux")] {
            result.push(".mozilla");
            result.push("firefox");
        } else if #[cfg(target_os = "macos")] {
            result.push("Library");
            result.push("Application Support");
            result.push("Firefox");
        } else if #[cfg(target_os = "windows")] {
            result.push("AppData");
            result.push("Roaming");
            result.push("Mozilla");
            result.push("Firefox");
        } else {
            compile_error!("Unknown OS!");
        }
    }
    return result;
}

impl Default for Config {
    fn default() -> Self {
        Config {
            browser_profile_dir: get_default_browser_profile_folder()
        }
    }
}

// This is the application state, it will be immutable through the life of the application
#[derive(Clone)]
struct AppState {
    config: Config,
    cur_profile_id: String,
    extension_id: Option<String>,
    config_dir: PathBuf,
    data_dir: PathBuf
}

fn read_configuration(path: &PathBuf) -> Config {
    if let Ok(file) = OpenOptions::new().read(true).open(path) {
        if let Ok(config) = serde_json::from_reader(file) {
            return config;
        }
    }

    // Config doesn't exist or is invalid, load default config
    Config::default()
}

// === PROFILE ===
struct ProfileEntry {
    id: String,
    name: String,
    is_relative: bool,
    path: String,
    default: bool,
    avatar: Option<String>
}

impl ProfileEntry {
    fn full_path(&self, config: &Config) -> PathBuf {
        return if self.is_relative {
            let mut result = config.browser_profile_dir.clone();
            result.push(&self.path);
            result
        } else {
            PathBuf::from(&self.path)
        }
    }
}

struct ProfilesIniState {
    backing_ini: Ini,
    profile_entries: Vec<ProfileEntry>
}

#[derive(Serialize, Deserialize)]
struct AvatarData {
    avatars: HashMap<String, String>
}

#[derive(Debug)]
enum ReadProfilesError {
    BadIniFormat,
    IniError(ini::Error),
    AvatarStoreError(io::Error),
    BadAvatarStoreFormat(serde_json::Error)
}

fn avatar_data_path(config_dir: &Path) -> PathBuf {
    config_dir.join("avatars.json")
}

fn read_profiles(config: &Config, config_dir: &Path) -> Result<ProfilesIniState, ReadProfilesError> {
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

            state.profile_entries.push(ProfileEntry {
                id: profile_id,
                name: profile_name.unwrap(),
                is_relative: profile_is_relative,
                path: profile_path,
                default: profile_default,
                avatar
            });
        }
    }

    Ok(state)
}

#[derive(Debug)]
enum WriteProfilesError {
    WriteIniError(Error),
    OpenAvatarFileError(io::Error),
    WriteAvatarFileError(serde_json::Error)
}
fn write_profiles(config: &Config, config_dir: &Path, state: &ProfilesIniState) -> Result<(), WriteProfilesError> {
    // Build avatar data
    let mut avatar_data = AvatarData {
        avatars: HashMap::new()
    };
    for profile in &state.profile_entries {
        if let Some(avatar) = &profile.avatar {
            avatar_data.avatars.insert(profile.id.clone(), avatar.clone());
        }
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

    // Write profile data
    let mut new_ini = state.backing_ini.clone();

    for (i, profile) in state.profile_entries.iter().enumerate() {
        let mut section = &mut new_ini.with_section(Some("Profile".to_owned() + &i.to_string()));
        section = section.set("Name", profile.name.as_str())
            .set("IsRelative", if profile.is_relative { "1" } else { "0" })
            .set("Path", profile.path.as_str());
        if profile.default {
            section.set("Default", "1");
        }
    }

    if let Err(e) = new_ini.write_to_file(config.profiles_ini_path()) {
        return Err(WriteProfilesError::WriteIniError(e))
    }

    Ok(())
}

fn calc_profile_id(path: &str, is_relative: bool) -> String {
    let mut context = Context::new(&SHA256);
    context.update(&[is_relative as u8]);
    context.update(path.as_bytes());
    return HEXUPPER.encode(context.finish().as_ref());
}

// === IPC ===
const IPC_CMD_FOCUS_WINDOW: u32 = 1;
const IPC_CMD_UPDATE_PROFILE_LIST: u32 = 2;
const IPC_CMD_CLOSE_MANAGER: u32 = 3;
fn get_ipc_socket_name(profile_id: &str, reset: bool) -> io::Result<impl ToLocalSocketName<'static>> {
    cfg_if! {
        if #[cfg(target_family = "unix")] {
            // TODO Somehow delete unix socket afterwards? IDK, could break everything if new instance starts before we delete socket
            let path: PathBuf = ["/tmp", ("fps-profile_".to_owned() + profile_id).as_str()].iter().collect();
            if reset {
                fs::remove_file(&path); // Delete old socket
            }
            return Ok(path);
        } else if #[cfg(target_family = "windows")] {
            return Ok("@fps-profile_".to_owned() + profile_id);
        } else {
            compile_error!("Unknown OS!");
        }
    }
}

fn handle_conn(app_state: &AppState, mut conn: LocalSocketStream) {
    // Write version
    if let Err(e) = conn.write_u8(0) {
        match e.kind() {
            io::ErrorKind::WriteZero => {}
            _ => log::error!("IPC error while writing version: {:?}", e)
        }
        return
    }

    loop {
        // Read command
        let command = match conn.read_u32::<NetworkEndian>() {
            Ok(c) => c,
            Err(e) => {
                match e.kind() {
                    io::ErrorKind::UnexpectedEof => {}
                    _ => log::error!("IPC error while reading command: {:?}", e)
                }
                return
            }
        };

        // Read command length
        /*let len = match conn.read_u64() {
            Ok(c) => c,
            Err(e) => {
                match e.kind() {
                    io::ErrorKind::UnexpectedEof => {}
                    _ => log::error!("IPC error while reading command length: {:?}", e)
                }
                return
            }
        };*/

        handle_ipc_cmd(app_state, command);

        // TODO Write different status if command failed
        // Write command status
        if let Err(e) = conn.write_i32::<NetworkEndian>(0) {
            match e.kind() {
                io::ErrorKind::WriteZero => {}
                _ => log::error!("IPC error while writing command status: {:?}", e)
            }
            return
        }
    }
}

fn setup_ipc(app_state: &AppState) -> std::result::Result<(), io::Error> {
    let socket_name = get_ipc_socket_name(&app_state.cur_profile_id, true)?;

    let listener = LocalSocketListener::bind(socket_name)?;
    for mut conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let app_state = app_state.clone();
                thread::spawn(move || handle_conn(&app_state, stream));
            }
            Err(e) => {
                log::error!("Incoming IPC connection failure: {:?}", e);
            }
        }
    }

    return Ok(());
}

fn handle_ipc_cmd(app_state: &AppState, cmd: u32) {
    match cmd {
        IPC_CMD_FOCUS_WINDOW => {
            // Focus window
            write_native_response(NativeResponseWrapper {
                id: NATIVE_RESP_ID_EVENT,
                resp: NativeResponse::event(NativeResponseEvent::FocusWindow)
            });
        }
        IPC_CMD_UPDATE_PROFILE_LIST => {
            match read_profiles(&app_state.config, &app_state.config_dir) {
                Ok(profiles) => {
                    // Notify updated profile list
                    write_native_response(NativeResponseWrapper {
                        id: NATIVE_RESP_ID_EVENT,
                        resp: NativeResponse::event(NativeResponseEvent::ProfileList {
                            current_profile_id: app_state.cur_profile_id.clone(),
                            profiles: profiles.profile_entries.iter().map(NativeResponseProfileListProfileEntry::from_profile_entry).collect()
                        })
                    });
                },
                Err(e) => {
                    log::error!("Failed to update profile list: {:?}", e);
                }
            };
        }
        IPC_CMD_CLOSE_MANAGER => {
            write_native_response(NativeResponseWrapper {
                id: NATIVE_RESP_ID_EVENT,
                resp: NativeResponse::event(NativeResponseEvent::CloseManager)
            });
        }
        _ => {
            log::error!("Unknown IPC command: {}", cmd);
        }
    }
}

#[derive(Debug)]
enum IpcError {
    NotRunning,
    IoError(io::Error)
}

fn send_ipc_cmd(app_state: &AppState, target_profile_id: &str, cmd: u32) -> std::result::Result<(), IpcError> {
    if app_state.cur_profile_id == target_profile_id {
        handle_ipc_cmd(app_state, cmd);
    } else {
        let socket_name = get_ipc_socket_name(target_profile_id, false)
            .map_err(|e| {IpcError::IoError(e)})?;

        let mut conn = LocalSocketStream::connect(socket_name).map_err(|e| {IpcError::IoError(e)})?;
        conn.write_u32::<NetworkEndian>(cmd).map_err(|e| {IpcError::IoError(e)})?;
    }
    Ok(())
}

// Notify all other running instances to update their profile list
fn notify_profile_changed(app_state: &AppState, state: &ProfilesIniState) {
    for profile in &state.profile_entries {
        send_ipc_cmd(app_state, &profile.id, IPC_CMD_UPDATE_PROFILE_LIST);
    }
}

// === MAIN ===

const DEBUG_ENV_VAR_WRITE_INPUT: &str = "FPS_DEBUG_WRITE_INPUT";
const DEBUG_ENV_VAR_INPUT_FILE: &str = "FPS_DEBUG_INPUT";
const DEBUG_ENV_VAR_PARENT_EXE: &str = "FPS_DEBUG_PARENT_EXE";

fn main() {
    // TODO DEBUG STUFF
    if env::var(DEBUG_ENV_VAR_WRITE_INPUT).is_ok() {
        if let Ok(debug_input_file) = env::var(DEBUG_ENV_VAR_INPUT_FILE) {
            eprintln!("Writing debug input file: {}", debug_input_file);

            // let input = NativeMessage::DeleteProfile(NativeMessageDeleteProfile { profile_id: "BA04B9F739719E40D10F4209D0D47CABE9E6FC2590F3F85144002EB8A31F8D36".to_owned() });
            let input =
                NativeMessage::LaunchProfile(NativeMessageLaunchProfile { profile_id: "BA04B9F739719E40D10F4209D0D47CABE9E6FC2590F3F85144002EB8A31F8D36".to_owned() });
            // let input = NativeMessage::ListProfiles;
            // let input = NativeMessage::CreateProfile(NativeMessageCreateProfile { name: "cheese".to_owned() });

            let serialized = serde_json::to_vec(&NativeMessageWrapper { id: 0, msg: input})
                .expect("Failed to serialize input to write to debug input file!");

            let mut debug_input_out = OpenOptions::new()
                .create(true)
                .write(true)
                .open(debug_input_file)
                .expect("Failed to open debug input file!");

            debug_input_out
                .write_u32::<NativeEndian>(serialized.len() as u32)
                .expect("Failed to write input size to debug input file!");

            debug_input_out.write_all(&serialized)
                .expect("Failed to write input to debug input file!");
        }
    }

    // Notify extension of our version
    write_native_response(NativeResponseWrapper {
        id: NATIVE_RESP_ID_EVENT,
        resp: NativeResponse::event(NativeResponseEvent::ConnectorInformation {
            version: APP_VERSION.to_string()
        })
    });

    // Calculate storage dirs
    let project_dirs = ProjectDirs::from("ax.nd",
                                        "nulldev",
                                        "FirefoxProfileSwitcher")
        .expect("Could not initialize configuration (failed to find storage dir)!");
    let pref_dir = project_dirs.preference_dir();
    let data_dir = project_dirs.data_local_dir();

    // mkdirs
    fs::create_dir_all(pref_dir);
    fs::create_dir_all(data_dir);

    // Setup logging
    fern::Dispatch::new()
        .level(log::LevelFilter::Trace)
        .chain(fern::log_file(data_dir.join("log.txt"))
            .expect("Unable to open logfile!"))
        .apply()
        .expect("Failed to setup logging!");

    // Find extension ID
    let args: Vec<String> = env::args().collect();
    let extension_id = args.get(2);
    if extension_id.is_none() {
        log::warn!("Could not determine extension ID!");
    }

    // Read configuration
    let config_path = pref_dir.join("config.json");
    let config = read_configuration(&config_path);

    // Calculate current profile location
    let cur_profile_id = {
        const CRASHREPORTER_ENV_VAR: &'static str = "MOZ_CRASHREPORTER_EVENTS_DIRECTORY";
        let crash_reporter_var = match env::var(CRASHREPORTER_ENV_VAR) {
            Ok(v) => v,
            Err(e) => {
                log::error!("Unable to read profile location (no env var: {}): {:?}", CRASHREPORTER_ENV_VAR, e);
                panic!("Unable to read profile location!");
            }
        };
        let profiles = match read_profiles(&config, pref_dir) {
            Ok(p) => p,
            Err(e) => {
                log::error!("Unable to read profiles list: {:?}", e);
                panic!("Unable to read profiles list!");
            }
        };
        let path = Path::new(&crash_reporter_var).parent().and_then(|v| v.parent());
        let path_unwrapped = match path {
            None => {
                log::error!("Could not traverse profile path {:?}!", path);
                panic!("Could not traverse profile path!");
            }
            Some(p) => p
        };

        let cur_profile_id = match profiles.profile_entries.iter()
            .find(|p| p.full_path(&config) == path_unwrapped)
            .map(|p| calc_profile_id(&p.path, p.is_relative)) {
            None => {
                log::error!("Could not find current profile!");
                panic!("Could not find current profile!");
            }
            Some(id) => id
        };

        write_native_response(NativeResponseWrapper {
            id: NATIVE_RESP_ID_EVENT,
            resp: NativeResponse::event(NativeResponseEvent::ProfileList {
                current_profile_id: cur_profile_id.clone(),
                profiles: profiles.profile_entries.iter().map(NativeResponseProfileListProfileEntry::from_profile_entry).collect()
            })
        });

        cur_profile_id
    };

    let app_state = AppState {
        config,
        cur_profile_id,
        extension_id: extension_id.cloned(),
        config_dir: pref_dir.to_path_buf(),
        data_dir: data_dir.to_path_buf()
    };

    // Begin IPC
    {
        let app_state = app_state.clone();
        thread::spawn(move || {
            if let Err(e) = setup_ipc(&app_state) {
                log::error!("Failed to setup IPC server: {:?}", e);
            }
        });
    }

    let mut using_debug = false; // TODO DEBUG STUFF
    loop {
        let message = match env::var(DEBUG_ENV_VAR_INPUT_FILE) {
            Ok(debug_input_file) => { // TODO DEBUG STUFF
                eprintln!("Using debug input file: {}", debug_input_file);
                using_debug = true;
                read_incoming_message(&mut OpenOptions::new()
                    .read(true)
                    .open(debug_input_file)
                    .expect("Unable to open debug input file!"))
            }
            Err(_) => read_incoming_message(&mut io::stdin())
        };

        // TODO Lock SI when updating profile list over IPC
        // SI lock
        let lock_path = data_dir.join("si.lock");
        // Create/open SI lockfile
        let lock_file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&lock_path)
            .expect("Failed to open single-instance lock!");
        // Lock the lockfile
        lock_file.lock_exclusive()
            .expect("Failed to grab single-instance lock!");

        let response = process_message(&app_state, message.msg);

        write_native_response(NativeResponseWrapper {
            id: message.id,
            resp: response
        });

        if using_debug { break } // TODO DEBUG STUFF
    }
}

fn process_message(app_state: &AppState, msg: NativeMessage) -> NativeResponse {
    let mut profiles = match read_profiles(&app_state.config, &app_state.config_dir) {
        Ok(p) => p,
        Err(e) => {
            return NativeResponse::error_with_dbg_msg("Failed to load profile list.", e);
        }
    };

    match msg {
        NativeMessage::LaunchProfile(msg) => process_cmd_launch_profile(app_state, &profiles, msg),
        NativeMessage::CreateProfile(msg) => process_cmd_create_profile(app_state, &mut profiles, msg),
        NativeMessage::DeleteProfile(msg) => process_cmd_delete_profile(app_state, &mut profiles, msg),
        NativeMessage::UpdateProfile(msg) => process_cmd_update_profile(app_state, &mut profiles, msg),
        NativeMessage::CloseManager => process_cmd_close_manager(app_state, &profiles)
    }
}

// === COMMANDS ===

fn process_cmd_launch_profile(app_state: &AppState,
                              profiles: &ProfilesIniState,
                              msg: NativeMessageLaunchProfile) -> NativeResponse {
    // Match ID with profile
    let profile = match profiles.profile_entries.iter().find(|p| p.id == msg.profile_id) {
        Some(p) => p,
        None => return NativeResponse::error("No profile with the specified id could be found!")
    };

    match send_ipc_cmd(app_state, &msg.profile_id, IPC_CMD_FOCUS_WINDOW) {
        Ok(_) => { return NativeResponse::success(NativeResponseData::ProfileLaunched); }
        Err(e) => { log::warn!("Failed to focus current browser window, launching new window: {:?}", e); }
    }

    let parent_proc = match get_parent_proc_path() {
        Ok(v) => v,
        Err(e) => return NativeResponse::error_with_dbg_msg("Unable to find browser binary!", e)
    };

    if !parent_proc.exists() {
        return NativeResponse::error_with_dbg_str(concat!(
        "The version of your browser that is currently running can no longer be found. ",
        "This is usually because your browser has updated but you haven't restarted your browser recently to apply the update. ",
        "Please restart your browser to resolve this issue."
        ), "Browser path: ".to_owned() + parent_proc.to_str().unwrap_or("UNKNOWN"))
    }

    /*let launch_browser = || match spawn_browser_proc(parent_proc, &profile.name) {
        Ok(_) => NativeResponse::success(NativeResponseData::ProfileLaunched),
        Err(e) => NativeResponse::error_with_dbg_msg("Failed to launch browser with new profile!", e)
    };*/

    cfg_if! {
        if #[cfg(target_family = "unix")] {
            match unsafe { nix::unistd::fork() } {
                Ok(ForkResult::Parent {child}) => {
                    match waitpid(child, None) {
                        Ok(nix::sys::wait::WaitStatus::Exited(child, 0)) => NativeResponse::success(NativeResponseData::ProfileLaunched),
                        e => NativeResponse::error_with_dbg_msg("Failed to launch browser with new profile (bad exit code)!", e)
                    }
                },
                Ok(ForkResult::Child) => exit(match nix::unistd::setsid() {
                    Ok(_) => {
                        // Close stdout, stderr and stdin
                        /*unsafe {
                            libc::close(0);
                            libc::close(1);
                            libc::close(2);
                        }*/
                        match spawn_browser_proc(&parent_proc, &profile.name) {
                            Ok(_) => 0,
                            Err(e) => 1
                        }
                    },
                    Err(_) => 2
                }),
                Err(e) => NativeResponse::error_with_dbg_msg("Failed to launch browser with new profile (fork error)!", e)
            }
        } else if #[cfg(target_family = "windows")] {
            match spawn_browser_proc(&parent_proc, &profile.name) {
                Ok(_) => NativeResponse::success(NativeResponseData::ProfileLaunched),
                Err(e) => NativeResponse::error_with_dbg_msg("Failed to launch browser with new profile!", e)
            }
        } else {
            compile_error!("Unknown OS!");
        }
    }
}

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

fn process_cmd_create_profile(app_state: &AppState, profiles: &mut ProfilesIniState, msg: NativeMessageCreateProfile) -> NativeResponse {
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
        avatar: Some(msg.avatar)
    };

    // Firefox will refuse to launch if we do not mkdirs the new profile folder
    let new_profile_full_path = new_profile.full_path(&app_state.config);
    if let Err(e) = fs::create_dir_all(&new_profile_full_path) {
        return NativeResponse::error_with_dbg_msg("Failed to folder for new profile!", e);
    }

    // Read current extensions JSON
    // TODO Extract this into function to fix this huge if-let chain
    {
        if let Some(our_profile) = profiles.profile_entries.iter().find(|p| p.id == app_state.cur_profile_id) {
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
        avatar: new_profile.avatar.clone()
    };
    profiles.profile_entries.push(new_profile);

    if let Err(e) = write_profiles(&app_state.config, &app_state.config_dir, profiles) {
        return NativeResponse::error_with_dbg_msg("Failed to save new changes!", e);
    }
    notify_profile_changed(app_state, profiles);

    return NativeResponse::success(NativeResponseData::ProfileCreated { profile: resp })
}

fn process_cmd_delete_profile(app_state: &AppState, profiles: &mut ProfilesIniState, msg: NativeMessageDeleteProfile) -> NativeResponse {
    let profile_index = match profiles.profile_entries.iter().position(|p| p.id == msg.profile_id) {
        Some(p) => p,
        None => return NativeResponse::error("No profile with the specified id could be found!")
    };

    // Delete profile from profile list (but do not write new list yet)
    let profile = profiles.profile_entries.remove(profile_index);

    let profile_path = profile.full_path(&app_state.config);

    // Check that profile is closed
    if [
        profile_path.join("cookies.sqlite-wal"),
        profile_path.join("webappsstore.sqlite-wal"),
        profile_path.join("places.sqlite-wal")
    ].iter().any(|file| file.exists()) {
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

    // Write new profile list
    if let Err(e) = write_profiles(&app_state.config, &app_state.config_dir, profiles) {
        return NativeResponse::error_with_dbg_msg("Failed to save new changes!", e);
    }
    notify_profile_changed(app_state, profiles);

    return NativeResponse::success(NativeResponseData::ProfileDeleted)
}

fn process_cmd_update_profile(app_state: &AppState, profiles: &mut ProfilesIniState, msg: NativeMessageUpdateProfile) -> NativeResponse {
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
    profile.avatar = Some(msg.avatar);

    let resp = NativeResponseProfileListProfileEntry {
        id: msg.profile_id,
        name: profile.name.clone(),
        default: profile.default,
        avatar: profile.avatar.clone()
    };

    if let Err(e) = write_profiles(&app_state.config, &app_state.config_dir, profiles) {
        return NativeResponse::error_with_dbg_msg("Failed to save new changes!", e);
    }
    notify_profile_changed(app_state, profiles);

    return NativeResponse::success(NativeResponseData::ProfileUpdated { profile: resp })
}

fn process_cmd_close_manager(app_state: &AppState, profiles: &ProfilesIniState) -> NativeResponse {
    for profile in &profiles.profile_entries {
        if profile.id != app_state.cur_profile_id {
            send_ipc_cmd(app_state, &profile.id, IPC_CMD_CLOSE_MANAGER);
        }
    }

    return NativeResponse::success(NativeResponseData::ManagerClosed)
}

// === PROCESS UTILS ===

fn spawn_browser_proc(bin_path: &PathBuf, profile_name: &str) -> io::Result<Child> {
    return Command::new(bin_path)
        .arg("-P")
        .arg(profile_name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[derive(Debug)]
enum GetParentProcError {
    NoCrashReporterEnvVar(VarError),
    LinuxOpenCurProcFailed(io::Error),
    LinuxFailedToParsePidString(String),
    LinuxCouldNotFindPPid,
    LinuxResolveParentExeFailed(io::Error)
}

fn get_parent_proc_path() -> Result<PathBuf, GetParentProcError> {
    // let cur_pid = process::id();
    let parent_binary: PathBuf;

    // New method gets browser binary by reading crash-reporter env var
    parent_binary = match env::var("MOZ_CRASHREPORTER_RESTART_ARG_0") {
        Ok(v) => PathBuf::from(v),
        Err(e) => return Err(NoCrashReporterEnvVar(e))
    };

    // Old method gets browser binary by getting parent proc
    /*cfg_if! {
        if #[cfg(target_os = "linux")] {
            // Get PID of parent
            let mut proc_status_file = match OpenOptions::new()
                    .read(true)
                    .open(["/proc", cur_pid.to_string().as_str(), "status"].iter().collect::<PathBuf>()) {
                Ok(f) => f,
                Err(e) => return Err(GetParentProcError::LinuxOpenCurProcFailed(e))
            };
            let mut proc_status = String::new();
            if let Err(e) = proc_status_file.read_to_string(&mut proc_status) {
                return Err(GetParentProcError::LinuxOpenCurProcFailed(e))
            }
            let mut parent_pid = None::<u32>;
            for line in proc_status.lines() {
                if let Some(split_loc) = line.find(":") {
                    let (left, right) = line.split_at(split_loc);
                    if left == "PPid" {
                        let right_trimmed = right[1..].trim();
                        parent_pid = Some(match right_trimmed.parse() {
                            Ok(v) => v,
                            Err(_) => return Err(GetParentProcError::LinuxFailedToParsePidString(right_trimmed.to_owned()))
                        })
                    }
                }
            }

            match parent_pid {
                Some(parent_pid) => {
                    // Get binary of parent
                    let parent_binary_symlink: PathBuf = ["/proc", parent_pid.to_string().as_str(), "exe"].iter().collect();
                    match fs::read_link(parent_binary_symlink) {
                        Ok(v) => parent_binary = v,
                        Err(e) => return Err(GetParentProcError::LinuxResolveParentExeFailed(e))
                    }
                }
                None => return Err(GetParentProcError::LinuxCouldNotFindPPid)
            }
        } else if #[cfg(target_os = "macos")] {
            compile_error!("Unimplemented!");
        } else if #[cfg(target_os = "windows")] {
            compile_error!("Unimplemented!");
        } else {
            compile_error!("Unknown OS!");
        }
    }*/

    // TODO DEBUG STUFF
    if let Ok(new_parent_binary) = env::var(DEBUG_ENV_VAR_PARENT_EXE) {
        return Ok(PathBuf::from(new_parent_binary));
    }

    Ok(parent_binary)
}