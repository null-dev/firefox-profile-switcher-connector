use crate::AppContext;
use crate::profiles::ProfilesIniState;
use crate::native_req::NativeMessageLaunchProfile;
use crate::native_resp::{NativeResponse, NativeResponseData};
use crate::ipc::notify_focus_window;
use crate::process::{fork_browser_proc, ForkBrowserProcError};

pub fn process_cmd_launch_profile(context: &AppContext,
                              profiles: &ProfilesIniState,
                              msg: NativeMessageLaunchProfile) -> NativeResponse {
    // Match ID with profile
    let profile = match profiles.profile_entries.iter().find(|p| p.id == msg.profile_id) {
        Some(p) => p,
        None => return NativeResponse::error("No profile with the specified id could be found!")
    };

    log::trace!("Launching profile: {}", profile.id);

    match notify_focus_window(context, &msg.profile_id, msg.url.clone()) {
        Ok(_) => { return NativeResponse::success(NativeResponseData::ProfileLaunched); }
        Err(e) => { log::info!("Failed to focus current browser window, launching new window: {:?}", e); }
    }

    match fork_browser_proc(context.state, profile, msg.url) {
        Ok(_) => NativeResponse::success(NativeResponseData::ProfileLaunched),
        Err(e) => match e {
            ForkBrowserProcError::BadExitCode => NativeResponse::error_with_dbg_msg("Failed to launch browser with new profile (bad exit code)!", e),
            ForkBrowserProcError::ForkError { .. } => NativeResponse::error_with_dbg_msg("Failed to launch browser with new profile (fork error)!", e),
            ForkBrowserProcError::ProcessLaunchError(_) => NativeResponse::error_with_dbg_msg("Failed to launch browser with new profile!", e),
            ForkBrowserProcError::BinaryNotFound(_) => NativeResponse::error_with_dbg_msg("Unable to find browser binary!", e),
            ForkBrowserProcError::BinaryDoesNotExist => NativeResponse::error(concat!(
            "The version of your browser that is currently running can no longer be found. ",
            "This is usually because your browser has updated but you haven't restarted your browser recently to apply the update. ",
            "Please restart your browser to resolve this issue."
            )),
            ForkBrowserProcError::COMError { .. } => NativeResponse::error_with_dbg_msg("Failed to launch browser with new profile (Windows COM error)!", e),
            ForkBrowserProcError::MSIXProcessLaunchError { .. } => NativeResponse::error_with_dbg_msg("Failed to launch browser with new profile (Windows AAM error)!", e),
        }
    }
}
