use crate::state::AppState;
use crate::profiles::ProfilesIniState;
use crate::native_resp::{NativeResponse, NativeResponseData};
use crate::ipc::{IPC_CMD_CLOSE_MANAGER, send_ipc_cmd};

pub fn process_cmd_close_manager(app_state: &AppState, profiles: &ProfilesIniState) -> NativeResponse {
    for profile in &profiles.profile_entries {
        if Some(&profile.id) != app_state.cur_profile_id.as_ref() {
            send_ipc_cmd(app_state, &profile.id, IPC_CMD_CLOSE_MANAGER);
        }
    }

    return NativeResponse::success(NativeResponseData::ManagerClosed)
}

