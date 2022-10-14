use crate::ipc::notify_close_manager;
use crate::native_resp::{NativeResponse, NativeResponseData};
use crate::profiles::ProfilesIniState;
use crate::AppContext;

pub fn process_cmd_close_manager(
    context: &AppContext,
    profiles: &ProfilesIniState,
) -> NativeResponse {
    notify_close_manager(context, profiles);

    NativeResponse::success(NativeResponseData::ManagerClosed)
}
