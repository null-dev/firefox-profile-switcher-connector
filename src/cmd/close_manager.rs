use crate::AppContext;
use crate::profiles::ProfilesIniState;
use crate::native_resp::{NativeResponse, NativeResponseData};
use crate::ipc::notify_close_manager;

pub fn process_cmd_close_manager(context: &AppContext, profiles: &ProfilesIniState) -> NativeResponse {
    notify_close_manager(context, profiles);

    return NativeResponse::success(NativeResponseData::ManagerClosed)
}

