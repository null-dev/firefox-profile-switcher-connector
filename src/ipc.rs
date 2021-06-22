use interprocess::local_socket::{ToLocalSocketName, LocalSocketStream, LocalSocketListener};
use std::{io, fs, thread};
use crate::state::AppState;
use byteorder::{WriteBytesExt, NetworkEndian, ReadBytesExt};
use std::io::{Write, Read, BufReader, BufWriter};
use std::path::PathBuf;
use crate::native_resp::{write_native_response, NativeResponseWrapper, NATIVE_RESP_ID_EVENT, NativeResponse, NativeResponseEvent, NativeResponseProfileListProfileEntry, write_native_event};
use crate::profiles::{read_profiles, ProfilesIniState};
use crate::options::{read_global_options, native_notify_updated_options};
use crate::storage::{options_data_path, global_options_data_path};
use cfg_if::cfg_if;
use serde::{Serialize, Deserialize};
use serde_json::value::Serializer;
use crate::process::fork_browser_proc;

// === IPC ===
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t", content = "c")]
enum IPCCommand {
    FocusWindow(FocusWindowCommand),
    UpdateProfileList,
    CloseManager,
    UpdateOptions
}
#[derive(Serialize, Deserialize, Debug)]
struct FocusWindowCommand {
    url: Option<String>
}
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
    if let Err(e) = conn.write_u8(0).and_then(|_| conn.flush()) {
        match e.kind() {
            _ => log::error!("IPC error while writing version: {:?}", e)
        }
        return
    }

    // We no longer attempt to read multiple messages on a connection, blame Windows...

    // Read command
    let mut deserializer = serde_cbor::Deserializer::from_reader(&mut conn);
    match IPCCommand::deserialize(&mut deserializer) {
        Ok(command) => handle_ipc_cmd(app_state, command),
        Err(e) => {
            log::error!("Failed to read command from IPC: {:?}", e);
            return
        }
    }

    // TODO Write different status if command failed
    // Write command status
    if let Err(e) = conn.write_i32::<NetworkEndian>(0).and_then(|_| conn.flush()) {
        match e.kind() {
            _ => log::error!("IPC error while writing command status: {:?}", e)
        }
        return
    }
}

pub fn setup_ipc(cur_profile_id: &str, app_state: &AppState) -> std::result::Result<(), io::Error> {
    log::trace!("Starting IPC server...");
    let socket_name = get_ipc_socket_name(cur_profile_id, true)?;

    let listener = LocalSocketListener::bind(socket_name)?;
    for mut conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                log::trace!("Incoming IPC connection.");
                let app_state = app_state.clone();

                // Windows seems to have trouble with multiple threads and named pipes :(
                cfg_if! {
                    if #[cfg(target_family = "windows")] {
                        handle_conn(&app_state, stream);
                    } else {
                        thread::spawn(move || handle_conn(&app_state, stream));
                    }
                }
            }
            Err(e) => {
                log::error!("Incoming IPC connection failure: {:?}", e);
            }
        }
    }

    return Ok(());
}

fn handle_ipc_cmd(app_state: &AppState, cmd: IPCCommand) {
    log::trace!("Executing IPC command: {:?}", cmd);

    match cmd {
        IPCCommand::FocusWindow(options) => handle_ipc_cmd_focus_window(app_state, options),
        IPCCommand::UpdateProfileList => {
            match read_profiles(&app_state.config, &app_state.config_dir) {
                Ok(profiles) => {
                    if let Some(pid) = &app_state.cur_profile_id
                        .as_ref()
                        .map(|it| it.clone()) {
                        // Notify updated profile list
                        write_native_event(NativeResponseEvent::ProfileList {
                            current_profile_id: pid.to_owned(),
                            profiles: profiles.profile_entries.iter().map(NativeResponseProfileListProfileEntry::from_profile_entry).collect()
                        });
                    }
                },
                Err(e) => {
                    log::error!("Failed to update profile list: {:?}", e);
                }
            };
        }
        IPCCommand::CloseManager => {
            write_native_event(NativeResponseEvent::CloseManager);
        }
        IPCCommand::UpdateOptions => {
            native_notify_updated_options(app_state);
        }
    }
}

fn handle_ipc_cmd_focus_window(app_state: &AppState, cmd: FocusWindowCommand) {
    if let Some(extension_id) = app_state.internal_extension_id.as_ref() {
        if let Some(cur_profile_id) = app_state.cur_profile_id.as_ref() {
            let global_options = read_global_options(&global_options_data_path(&app_state.config_dir));
            if global_options["windowFocusWorkaround"] == serde_json::Value::Bool(true) {
                if let Ok(profiles) = read_profiles(&app_state.config, &app_state.config_dir) {
                    if let Some(cur_profile) = profiles.profile_entries
                        .iter()
                        .find(|e| &e.id == cur_profile_id) {
                        let url = match cmd.url {
                            Some(url) => url,
                            None => format!("moz-extension://{}/js/winfocus/winfocus.html", extension_id)
                        };
                        fork_browser_proc(app_state, cur_profile, Some(url));
                        return;
                    }
                }
            }
        }
    }
    // Focus window
    write_native_event(NativeResponseEvent::FocusWindow {
        url: cmd.url
    });
}

#[derive(Debug)]
pub enum IpcError {
    NotRunning,
    BadStatus,
    SerializationError(serde_cbor::Error),
    IoError(io::Error)
}

fn send_ipc_cmd(app_state: &AppState, target_profile_id: &str, cmd: IPCCommand) -> std::result::Result<(), IpcError> {
    log::trace!("Sending IPC command {:?} to profile: {}", cmd, target_profile_id);
    let cur_profile_id = &app_state.cur_profile_id;
    if cur_profile_id.is_some() && cur_profile_id.as_ref().unwrap() == target_profile_id {
        log::trace!("Fast-pathing IPC command...");
        handle_ipc_cmd(app_state, cmd);
        Ok(())
    } else {
        let socket_name = get_ipc_socket_name(target_profile_id, false)
            .map_err(|e| {IpcError::IoError(e)})?;

        let mut conn = LocalSocketStream::connect(socket_name).map_err(|e| {IpcError::IoError(e)})?;
        log::trace!("Connected to IPC target, reading remote version...");
        let remote_version = conn.read_u8().map_err(|e| {IpcError::IoError(e)})?;
        log::trace!("Remote version is: {}, Writing IPC command...", remote_version);
        serde_cbor::to_writer(&mut conn, &cmd)
            .map_err(IpcError::SerializationError)
            .and_then(|_| conn.flush()
                .map_err(IpcError::IoError))?;
        log::trace!("IPC command written, reading status...");
        let status = conn.read_i32::<NetworkEndian>().map_err(|e| {IpcError::IoError(e)})?;
        log::trace!("IPC command status is: {}", status);
        if status == 0 {
            Ok(())
        } else {
            Err(IpcError::BadStatus)
        }
    }
}

// Notify another instance to focus it's window
pub fn notify_focus_window(app_state: &AppState, target_profile_id: &String, url: Option<String>) -> Result<(), IpcError> {
    send_ipc_cmd(app_state, target_profile_id, IPCCommand::FocusWindow(FocusWindowCommand {
        url
    }))
}

// Notify all other running instances to update their profile list
pub fn notify_profile_changed(app_state: &AppState, profiles: &ProfilesIniState) {
    for profile in &profiles.profile_entries {
        send_ipc_cmd(app_state, &profile.id, IPCCommand::UpdateProfileList);
    }
}

// Notify all other running instances to update their options
pub fn notify_options_changed(app_state: &AppState, profiles: &ProfilesIniState) {
    for profile in &profiles.profile_entries {
        send_ipc_cmd(app_state, &profile.id, IPCCommand::UpdateOptions);
    }
}

// Notify all other running instances to close their managers
pub fn notify_close_manager(app_state: &AppState, profiles: &ProfilesIniState) {
    for profile in &profiles.profile_entries {
        if Some(&profile.id) != app_state.cur_profile_id.as_ref() {
            send_ipc_cmd(app_state, &profile.id, IPCCommand::CloseManager);
        }
    }
}
