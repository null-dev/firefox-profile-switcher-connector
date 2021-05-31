use crate::state::AppState;
use crate::profiles::ProfilesIniState;
use crate::native_req::NativeMessageLaunchProfile;
use crate::native_resp::{NativeResponse, NativeResponseData};
use crate::ipc::{IPC_CMD_FOCUS_WINDOW, send_ipc_cmd};
use std::path::PathBuf;
use std::{io, env};
use std::process::{exit, Child, Command, Stdio};
use std::env::VarError;
use cfg_if::cfg_if;
use crate::cmd::launch_profile::GetParentProcError::NoCrashReporterEnvVar;

cfg_if! {
    if #[cfg(target_family = "unix")] {
        extern crate nix;
        extern crate libc;

        use nix::unistd::ForkResult;
        use nix::sys::wait::waitpid;
    } else if #[cfg(target_family = "windows")] {
        extern crate winapi;

        use std::os::windows::process::CommandExt;
    } else {
        compile_error!("Unknown OS!");
    }
}

pub fn process_cmd_launch_profile(app_state: &AppState,
                              profiles: &ProfilesIniState,
                              msg: NativeMessageLaunchProfile) -> NativeResponse {
    // Match ID with profile
    let profile = match profiles.profile_entries.iter().find(|p| p.id == msg.profile_id) {
        Some(p) => p,
        None => return NativeResponse::error("No profile with the specified id could be found!")
    };

    log::trace!("Launching profile: {}", profile.id);

    match send_ipc_cmd(app_state, &msg.profile_id, IPC_CMD_FOCUS_WINDOW) {
        Ok(_) => { return NativeResponse::success(NativeResponseData::ProfileLaunched); }
        Err(e) => { log::info!("Failed to focus current browser window, launching new window: {:?}", e); }
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

    log::trace!("Browser binary found: {:?}", parent_proc);

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
            // TODO Change app ID to separate on taskbar?
            match spawn_browser_proc(&parent_proc, &profile.name) {
                Ok(_) => NativeResponse::success(NativeResponseData::ProfileLaunched),
                Err(e) => NativeResponse::error_with_dbg_msg("Failed to launch browser with new profile!", e)
            }
        } else {
            compile_error!("Unknown OS!");
        }
    }
}

// === PROCESS UTILS ===

fn spawn_browser_proc(bin_path: &PathBuf, profile_name: &str) -> io::Result<Child> {
    let mut command = Command::new(bin_path);
    cfg_if! {
        if #[cfg(target_family = "windows")] {
            command.creation_flags((winapi::um::winbase::DETACHED_PROCESS | winapi::um::winbase::CREATE_BREAKAWAY_FROM_JOB) as u32);
        }
    }
    return command
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

    Ok(parent_binary)
}
