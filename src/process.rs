use std::{io, env};
use std::env::VarError;
use cfg_if::cfg_if;
use std::path::PathBuf;
use std::process::{exit, Child, Command, Stdio};
use crate::state::AppState;
use crate::profiles::ProfileEntry;

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


#[derive(Debug)]
pub enum ForkBrowserProcError {
    BadExitCode,
    ForkError { error_message: String },
    ProcessLaunchError(io::Error),
    BinaryNotFound,
    BinaryDoesNotExist
}

pub fn fork_browser_proc(app_state: &AppState, profile: &ProfileEntry, url: Option<String>) -> Result<(), ForkBrowserProcError> {
    let parent_proc = match app_state.config.browser_binary() {
        Some(v) => v.clone(),
        None => match get_parent_proc_path() {
            Ok(v) => v,
            Err(e) => return Err(ForkBrowserProcError::BinaryNotFound)
        }
    };

    if !parent_proc.exists() {
        return Err(ForkBrowserProcError::BinaryDoesNotExist)
    }

    log::trace!("Browser binary found: {:?}", parent_proc);

    cfg_if! {
        if #[cfg(target_family = "unix")] {
            match unsafe { nix::unistd::fork() } {
                Ok(ForkResult::Parent {child}) => {
                    match waitpid(child, None) {
                        Ok(nix::sys::wait::WaitStatus::Exited(child, 0)) => Ok(()),
                        e => Err(ForkBrowserProcError::BadExitCode)
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
                        match spawn_browser_proc(&parent_proc, &profile.name, url) {
                            Ok(_) => 0,
                            Err(e) => 1
                        }
                    },
                    Err(_) => 2
                }),
                Err(e) => Err(ForkBrowserProcError::ForkError { error_message: format!("{:?}", e) })
            }
        } else if #[cfg(target_family = "windows")] {
            // TODO Change app ID to separate on taskbar?
            match spawn_browser_proc(&parent_proc, &profile.name, url) {
                Ok(_) => Ok(()),
                Err(e) => Err(ForkBrowserProcError::ProcessLaunchError(e))
            }
        } else {
            compile_error!("Unknown OS!");
        }
    }
}

fn spawn_browser_proc(bin_path: &PathBuf, profile_name: &str, url: Option<String>) -> io::Result<Child> {
    let mut command = Command::new(bin_path);
    cfg_if! {
        if #[cfg(target_family = "windows")] {
            command.creation_flags((winapi::um::winbase::DETACHED_PROCESS | winapi::um::winbase::CREATE_BREAKAWAY_FROM_JOB) as u32);
        }
    }
    command
        .arg("-P")
        .arg(profile_name);
    if let Some(url) = url {
        command
            .arg("--new-tab")
            .arg(url);
    }
    log::trace!("Executing command: {:?}", command);
    return command
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
        Err(e) => return Err(GetParentProcError::NoCrashReporterEnvVar(e))
    };

    Ok(parent_binary)
}
