use std::{io, env};
use std::env::VarError;
use cfg_if::cfg_if;
use std::path::PathBuf;
use std::process::{exit, Child, Command, Stdio};
use once_cell::sync::Lazy;
use crate::state::AppState;
use crate::profiles::ProfileEntry;

cfg_if! {
    if #[cfg(target_family = "unix")] {
        use nix::unistd::ForkResult;
        use nix::sys::wait::waitpid;
    } else if #[cfg(target_family = "windows")] {
        use windows::Win32::System::Threading as win_threading;
        use windows::Win32::UI::Shell::{ApplicationActivationManager, IApplicationActivationManager, AO_NONE};
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
        use windows::Win32::Foundation::PWSTR;
        use std::os::windows::process::CommandExt;
        use crate::config::get_msix_package;
    } else {
        compile_error!("Unknown OS!");
    }
}


#[derive(Debug)]
pub enum ForkBrowserProcError {
    BadExitCode,
    ForkError { error_message: String },
    ProcessLaunchError(io::Error),
    MSIXProcessLaunchError { error_message: String },
    BinaryNotFound,
    BinaryDoesNotExist,
    COMError { error_message: String }
}

pub fn fork_browser_proc(app_state: &AppState, profile: &ProfileEntry, url: Option<String>) -> Result<(), ForkBrowserProcError> {
    // Special case on Windows when FF is installed from Microsoft Store
    cfg_if! {
        if #[cfg(target_family = "windows")] {
            if let Ok(msix_package) = get_msix_package() {
                let aam: IApplicationActivationManager = unsafe {
                    CoCreateInstance(
                        &ApplicationActivationManager,
                        None,
                        CLSCTX_ALL
                    )
                }.map_err(|e| ForkBrowserProcError::COMError {
                    error_message: e.message().to_string_lossy()
                })?;

                let browser_args = build_browser_args(&profile.name, url)
                    .iter()
                    // Surround each arg with quotes and escape quotes with triple quotes
                    // See: https://stackoverflow.com/questions/7760545/escape-double-quotes-in-parameter
                    .map(|a| format!(r#""{}""#, a.replace(r#"""#, r#"""""#)))
                    .collect::<Vec<String>>()
                    .join(" ");

                log::trace!("Browser args: {:?}", browser_args);

                let aumid = format!("{}!App", msix_package);
                unsafe {
                    aam.ActivateApplication(
                        aumid.as_str(),
                        browser_args.as_str(),
                        AO_NONE
                    )
                }.map_err(|e| ForkBrowserProcError::MSIXProcessLaunchError {
                    error_message: e.message().to_string_lossy()
                })?;

                return Ok(());
            }
        }
    }

    let parent_proc = match app_state.config.browser_binary() {
        Some(v) => v,
        None => match get_parent_proc_path() {
            Ok(v) => v,
            Err(_) => return Err(ForkBrowserProcError::BinaryNotFound)
        }
    };

    if !parent_proc.exists() {
        return Err(ForkBrowserProcError::BinaryDoesNotExist)
    }

    log::trace!("Browser binary found: {:?}", parent_proc);

    let browser_args = build_browser_args(&profile.name, url);

    log::trace!("Browser args: {:?}", browser_args);

    cfg_if! {
        if #[cfg(target_family = "unix")] {
            match unsafe { nix::unistd::fork() } {
                Ok(ForkResult::Parent {child}) => {
                    match waitpid(child, None) {
                        Ok(nix::sys::wait::WaitStatus::Exited(_, 0)) => Ok(()),
                        _ => Err(ForkBrowserProcError::BadExitCode)
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
                        match spawn_browser_proc(&parent_proc, browser_args) {
                            Ok(_) => 0,
                            Err(_) => 1
                        }
                    },
                    Err(_) => 2
                }),
                Err(e) => Err(ForkBrowserProcError::ForkError { error_message: format!("{:?}", e) })
            }
        } else if #[cfg(target_family = "windows")] {
            // TODO Change app ID to separate on taskbar?
            match spawn_browser_proc(&parent_proc, browser_args) {
                Ok(_) => Ok(()),
                Err(e) => Err(ForkBrowserProcError::ProcessLaunchError(e))
            }
        } else {
            compile_error!("Unknown OS!");
        }
    }
}

fn build_browser_args(profile_name: &str, url: Option<String>) -> Vec<String> {
    let mut vec = vec![
        "-P".to_owned(),
        profile_name.to_owned()
    ];
    if let Some(url) = url {
        vec.push("--new-tab".to_owned());
        vec.push(url);
    }
    vec
}

fn spawn_browser_proc(bin_path: &PathBuf, args: Vec<String>) -> io::Result<Child> {
    let mut command = Command::new(bin_path);
    cfg_if! {
        if #[cfg(target_family = "windows")] {
            command.creation_flags((win_threading::DETACHED_PROCESS | win_threading::CREATE_BREAKAWAY_FROM_JOB).0);
        }
    }
    command.args(args);
    log::trace!("Executing command: {:?}", command);
    return command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[derive(Debug)]
pub enum GetParentProcError {
    NoCrashReporterEnvVar(VarError),
    LinuxOpenCurProcFailed(io::Error),
    LinuxFailedToParsePidString(String),
    LinuxCouldNotFindPPid,
    LinuxResolveParentExeFailed(io::Error)
}

static PARENT_PROC: Lazy<Result<PathBuf, GetParentProcError>> = Lazy::new(|| {
    // Get browser binary by reading crash-reporter env var
    env::var("MOZ_CRASHREPORTER_RESTART_ARG_0")
        .map(PathBuf::from)
        .map_err(GetParentProcError::NoCrashReporterEnvVar)
});

pub fn get_parent_proc_path() -> Result<&'static PathBuf, &'static GetParentProcError> {
    PARENT_PROC.as_ref()
}
