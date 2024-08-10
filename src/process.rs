
use std::{io, env};
use std::env::VarError;
use std::path::{PathBuf};
use std::process::{Child, Command, Stdio};
use sysinfo::{get_current_pid, ProcessRefreshKind, RefreshKind, System};
use cfg_if::cfg_if;
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
    LinuxResolveParentExeFailed(io::Error),
    CouldNotResolveExePath,
}

static PARENT_PROC: Lazy<Result<PathBuf, GetParentProcError>> = Lazy::new(|| {
    let crash_restart_arg = env::var("MOZ_CRASHREPORTER_RESTART_ARG_0");
    if crash_restart_arg.is_ok()
    {
        let crash_restart_bin_path = PathBuf::from(crash_restart_arg.unwrap());
        if crash_restart_bin_path.try_exists().is_ok_and(|exists| exists == true)
        {
            return Ok(crash_restart_bin_path);
        }
    }

    // Running version of Firefox where "MOZ_CRASHREPORTER_RESTART_ARG_0" is removed, try getting parent process and its path directly
    let myproc_id = get_current_pid();
    if myproc_id.is_ok()
    {
        let sys = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::new().with_exe(sysinfo::UpdateKind::OnlyIfNotSet))
       );
        let myproc = sys.process(myproc_id.unwrap());
        
        let parent_id = myproc.unwrap().parent();
        if !parent_id.is_none()
        {
            let parentproc = sys.process(parent_id.unwrap());
            if !parentproc.is_none()
            {
                let proc_path = parentproc.unwrap().exe();
                if !proc_path.is_none()
                {
                    let bin_path = PathBuf::from(proc_path.unwrap());
                    let bin_exist = bin_path.try_exists();

                    if bin_exist.is_ok_and(|exists| exists == true)
                    {
                        return Ok(bin_path);
                    }
                }
            }
        }
    }
    // TODO: Add conditions to handle other systems than Windows

    //Failed using most reliable methods, so find the correct path using this other relative path var. Hopefully this one doesn't get removed too...
    // MOZ_CRASHREPORTER_STRINGS_OVERRIDE points to parent Firefox process's install-root-folder\browser\crashreporter-override.ini
    // So traverse up to get the location of the parent firefox.exe's path.
    // Won't work reliably for other forks that don't use firefox.exe as the name, or other operating systems until conditions added
    let crash_override_env_var = env::var("MOZ_CRASHREPORTER_STRINGS_OVERRIDE");
   
    if crash_override_env_var.is_ok()
    {
        let crash_override_path = PathBuf::from(crash_override_env_var.unwrap().replace("\\", "/")); //PathBuf parent() hopping seems to break with backwards slash paths...
        let firefox_browser_dir = crash_override_path.ancestors().find(|parent| parent.ends_with("browser") );
        if !firefox_browser_dir.is_none()
        {
            //Go up one more directory, as it should be Firefox root if we were at /browser/ path.
            let firefox_bin_path = PathBuf::from(firefox_browser_dir.unwrap().parent().unwrap().display().to_string() + "/firefox.exe");
            let ff_bin_exists = firefox_bin_path.try_exists();
            if ff_bin_exists.is_ok_and(|exists| exists == true)
            {
                return Ok(firefox_bin_path);
            }
        }
    }

    return Err(GetParentProcError::CouldNotResolveExePath);
});

pub fn get_parent_proc_path() -> Result<&'static PathBuf, &'static GetParentProcError> {
    PARENT_PROC.as_ref()
}
