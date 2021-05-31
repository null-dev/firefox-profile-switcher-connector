use crate::state::AppState;
use crate::profiles::ProfilesIniState;
use crate::native_resp::{NativeResponse, NativeResponseData};
use crate::ipc::notify_close_manager;

pub fn process_cmd_close_manager(app_state: &AppState, profiles: &ProfilesIniState) -> NativeResponse {
    notify_close_manager(app_state, profiles);

    return NativeResponse::success(NativeResponseData::ManagerClosed)
}

