use std::{io, thread};
use crate::state::AppState;
use std::time::Duration;
use crate::native_resp::{NativeResponseEvent, NativeResponseProfileListProfileEntry, write_native_event};
use crate::profiles::{read_profiles, ProfilesIniState};
use crate::options::{read_global_options, native_notify_updated_options};
use crate::storage::{global_options_data_path};
use cfg_if::cfg_if;
use eyre::ContextCompat;
use nng::{Message, Protocol, Socket};
use nng::options::{Options, RecvTimeout, SendTimeout};
use serde::{Serialize, Deserialize};
use crate::AppContext;
use crate::avatars::{update_and_native_notify_avatars};
use crate::process::fork_browser_proc;
use crate::profiles_order::{native_notify_updated_profile_order, OrderData};

// === IPC ===
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t", content = "c")]
enum IPCCommand {
    FocusWindow(FocusWindowCommand),
    UpdateProfileList,
    CloseManager,
    UpdateOptions,
    UpdateAvatars,
    UpdateProfileOrder,
}
#[derive(Serialize, Deserialize, Debug)]
struct FocusWindowCommand {
    url: Option<String>
}
fn get_ipc_socket_name(profile_id: &str, reset: bool) -> io::Result<String> {
    cfg_if! {
        if #[cfg(target_family = "unix")] {
            // TODO Somehow delete unix socket afterwards? IDK, could break everything if new instance starts before we delete socket
            let url = format!("ipc:///tmp/fps-profile_{}", profile_id);
            log::trace!("IPC socket for profile {:?} resolved to: {:?}", profile_id, url);
            return Ok(url);
        } else if #[cfg(target_family = "windows")] {
            let name = format!("ipc://fps-profile_{}", profile_id);
            log::trace!("IPC pipe for profile {:?} resolved to: {:?}", profile_id, name);
            return Ok(name);
        } else {
            compile_error!("Unknown OS!");
        }
    }
}

fn handle_conn(context: &AppContext, server: &Socket, msg: Message) {
    // Read command
    let mut deserializer = serde_cbor::Deserializer::from_slice(msg.as_slice());
    match IPCCommand::deserialize(&mut deserializer) {
        Ok(command) => {
            let context_clone = context.clone();
            // Windows doesn't seem to like it if we block when reading from a named pipe
            //   So instead handle the command in a new thread to avoid doing expensive stuff
            //   in the IPC thread.
            thread::spawn(move || handle_ipc_cmd(&context_clone, command));
        }
        Err(e) => {
            log::error!("Failed to read command from IPC: {:?}", e);
            return
        }
    }

    // TODO Write different status if command failed
    // Write command status
    if let Err(e) = server.send(Message::from([0])) {
        log::error!("IPC error while writing command status: {:?}", e);
        return
    }
}

pub fn setup_ipc(context: &AppContext) -> eyre::Result<()> {
    log::trace!("Starting IPC server...");
    let socket_name = get_ipc_socket_name(context.state
                                              .cur_profile_id
                                              .as_ref()
                                              .context("Missing profile ID!")?, true)?;

    let server = Socket::new(Protocol::Rep0)?;
    server.listen(&socket_name)?;
    loop {
        let msg = server.recv()?;

        handle_conn(context, &server, msg);
    }
}

fn handle_ipc_cmd(context: &AppContext, cmd: IPCCommand) {
    log::trace!("Executing IPC command: {:?}", cmd);

    match cmd {
        IPCCommand::FocusWindow(options) => handle_ipc_cmd_focus_window(context.state, options),
        IPCCommand::UpdateProfileList => {
            match read_profiles(&context.state.config, &context.state.config_dir) {
                Ok(profiles) => {
                    if let Some(pid) = &context.state.cur_profile_id
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
            native_notify_updated_options(context.state);
        }
        IPCCommand::UpdateAvatars => {
            update_and_native_notify_avatars(context);
        }
        IPCCommand::UpdateProfileOrder => {
            native_notify_updated_profile_order(context.state);
        }
    }

    log::trace!("Execution complete!");
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
                            None => format!("moz-extension://{}/src/entries/winfocus/index.html", extension_id)
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
    BadStatus,
    SerializationError(serde_cbor::Error),
    IoError(io::Error),
    NetworkError(nng::Error)
}

fn send_ipc_cmd(context: &AppContext, target_profile_id: &str, cmd: IPCCommand) -> std::result::Result<(), IpcError> {
    log::trace!("Sending IPC command {:?} to profile: {}", cmd, target_profile_id);
    let cur_profile_id = context.state.cur_profile_id.as_deref();
    if cur_profile_id.is_some() && cur_profile_id.unwrap() == target_profile_id {
        log::trace!("Fast-pathing IPC command...");
        handle_ipc_cmd(context, cmd);
        Ok(())
    } else {
        let socket_name = get_ipc_socket_name(target_profile_id, false)
            .map_err(IpcError::IoError)?;

        let conn = Socket::new(Protocol::Req0).map_err(IpcError::NetworkError)?;
        conn.set_opt::<SendTimeout>(Some(Duration::from_millis(500)));
        conn.set_opt::<RecvTimeout>(Some(Duration::from_millis(3000)));
        conn.dial(&socket_name).map_err(IpcError::NetworkError)?;
        log::trace!("Writing IPC command...");
        let serialized = serde_cbor::to_vec(&cmd)
            .map_err(IpcError::SerializationError)?;
        conn.send(Message::from(&serialized));
        log::trace!("IPC command written, reading status...");
        let resp = conn.recv()
            .map_err(IpcError::NetworkError)?;
        let status = resp.first().unwrap_or(&1);
        log::trace!("IPC command status is: {}", status);
        if *status == 0 {
            Ok(())
        } else {
            Err(IpcError::BadStatus)
        }
    }
}

// Notify another instance to focus it's window
pub fn notify_focus_window(context: &AppContext, target_profile_id: &String, url: Option<String>) -> Result<(), IpcError> {
    send_ipc_cmd(context, target_profile_id, IPCCommand::FocusWindow(FocusWindowCommand {
        url
    }))
}

// Notify all running instances to update their profile list
pub fn notify_profile_changed(context: &AppContext, profiles: &ProfilesIniState) {
    for profile in &profiles.profile_entries {
        send_ipc_cmd(context, &profile.id, IPCCommand::UpdateProfileList);
    }
}

// Notify all running instances to update their options
pub fn notify_options_changed(context: &AppContext, profiles: &ProfilesIniState) {
    for profile in &profiles.profile_entries {
        send_ipc_cmd(context, &profile.id, IPCCommand::UpdateOptions);
    }
}

// Notify all other running instances to close their managers
pub fn notify_close_manager(context: &AppContext, profiles: &ProfilesIniState) {
    for profile in &profiles.profile_entries {
        if Some(&profile.id) != context.state.cur_profile_id.as_ref() {
            send_ipc_cmd(context, &profile.id, IPCCommand::CloseManager);
        }
    }
}

// Notify all running instances to update their avatars
pub fn notify_update_avatars(context: &AppContext, profiles: &ProfilesIniState) {
    for profile in &profiles.profile_entries {
        send_ipc_cmd(context, &profile.id, IPCCommand::UpdateAvatars);
    }
}

// Notify all running instances to update their profile order
pub fn notify_update_profile_order(context: &AppContext, profiles: &ProfilesIniState) {
    for profile in &profiles.profile_entries {
        send_ipc_cmd(context, &profile.id, IPCCommand::UpdateProfileOrder);
    }
}
