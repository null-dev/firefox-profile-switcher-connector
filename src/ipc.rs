use interprocess::local_socket::{ToLocalSocketName, LocalSocketStream, LocalSocketListener};
use std::{io, fs, thread};
use crate::state::AppState;
use byteorder::{WriteBytesExt, NetworkEndian, ReadBytesExt};
use std::io::Write;
use std::path::PathBuf;
use crate::native_resp::{write_native_response, NativeResponseWrapper, NATIVE_RESP_ID_EVENT, NativeResponse, NativeResponseEvent, NativeResponseProfileListProfileEntry};
use crate::profiles::{read_profiles, ProfilesIniState};
use crate::options::read_global_options;
use crate::storage::options_data_path;
use cfg_if::cfg_if;

// === IPC ===
pub const IPC_CMD_FOCUS_WINDOW: u32 = 1;
const IPC_CMD_UPDATE_PROFILE_LIST: u32 = 2;
pub const IPC_CMD_CLOSE_MANAGER: u32 = 3;
const IPC_CMD_UPDATE_OPTIONS: u32 = 4;
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

    loop {
        // Read command
        let command = match conn.read_u32::<NetworkEndian>() {
            Ok(c) => c,
            Err(e) => {
                match e.kind() {
                    io::ErrorKind::UnexpectedEof => {}
                    io::ErrorKind::ConnectionAborted => {}
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
        if let Err(e) = conn.write_i32::<NetworkEndian>(0).and_then(|_| conn.flush()) {
            match e.kind() {
                _ => log::error!("IPC error while writing command status: {:?}", e)
            }
            return
        }
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

fn handle_ipc_cmd(app_state: &AppState, cmd: u32) {
    log::trace!("Executing IPC command: {}", cmd);
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
                    if let Some(pid) = &app_state.cur_profile_id
                        .as_ref()
                        .map(|it| it.clone()) {
                        // Notify updated profile list
                        write_native_response(NativeResponseWrapper {
                            id: NATIVE_RESP_ID_EVENT,
                            resp: NativeResponse::event(NativeResponseEvent::ProfileList {
                                current_profile_id: pid.to_owned(),
                                profiles: profiles.profile_entries.iter().map(NativeResponseProfileListProfileEntry::from_profile_entry).collect()
                            })
                        });
                    }
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
        IPC_CMD_UPDATE_OPTIONS => {
            let new_options = read_global_options(
                &options_data_path(&app_state.config_dir));

            write_native_response(NativeResponseWrapper {
                id: NATIVE_RESP_ID_EVENT,
                resp: NativeResponse::event(NativeResponseEvent::OptionsUpdated {
                    options: new_options
                })
            })
        }
        _ => {
            log::error!("Unknown IPC command: {}", cmd);
        }
    }
}

#[derive(Debug)]
pub enum IpcError {
    NotRunning,
    BadStatus,
    IoError(io::Error)
}

pub fn send_ipc_cmd(app_state: &AppState, target_profile_id: &str, cmd: u32) -> std::result::Result<(), IpcError> {
    log::trace!("Sending IPC command {} to profile: {}", cmd, target_profile_id);
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
        conn.write_u32::<NetworkEndian>(cmd).and_then(|_| conn.flush()).map_err(|e| {IpcError::IoError(e)})?;
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

// Notify all other running instances to update their profile list
pub fn notify_profile_changed(app_state: &AppState, profiles: &ProfilesIniState) {
    for profile in &profiles.profile_entries {
        send_ipc_cmd(app_state, &profile.id, IPC_CMD_UPDATE_PROFILE_LIST);
    }
}

// Notify all other running instances to update their options list
pub fn notify_options_changed(app_state: &AppState, profiles: &ProfilesIniState) {
    for profile in &profiles.profile_entries {
        send_ipc_cmd(app_state, &profile.id, IPC_CMD_UPDATE_OPTIONS);
    }
}
