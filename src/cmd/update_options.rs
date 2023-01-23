use crate::AppContext;
use crate::profiles::ProfilesIniState;
use crate::native_req::NativeMessageUpdateOptions;
use crate::native_resp::{NativeResponse, NativeResponseData};
use crate::storage::global_options_data_path;
use crate::options::{read_global_options, write_global_options};
use crate::ipc::notify_options_changed;

pub fn process_cmd_update_options(context: &AppContext,
                              profiles: ProfilesIniState,
                              msg: NativeMessageUpdateOptions) -> NativeResponse {
    let options_data_path = global_options_data_path(&context.state.config_dir);
    let mut options = read_global_options(&options_data_path);

    for change in msg.changes {
        options.insert(change.0, change.1);
    }

    if let Err(e) = write_global_options(&options_data_path, &options) {
        return NativeResponse::error_with_dbg_msg("Failed to save new changes!", e);
    }
    notify_options_changed(context, &profiles);

    return NativeResponse::success(NativeResponseData::OptionsUpdated { options })
}

