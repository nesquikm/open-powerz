//! One function per subcommand. Each takes an open [`Session`] (or a device
//! list for `list`) and [`Write`] sinks, so tests drive them with
//! [`MockTransport`](powerz_protocol::transport::MockTransport) and in-memory
//! buffers.
//!
//! [`Session`]: powerz_protocol::session::Session
//! [`Write`]: std::io::Write

use std::io::{Read, Write};

use powerz_protocol::pdm::PdmSession;
use powerz_protocol::session::Clock;

use crate::error::CliError;

mod console;
mod detect;
mod firmware;
mod info;
mod list;
mod pd;
mod pd_send;
mod read;
mod records;
mod settings;
mod stream;
mod trigger;
mod watch;

pub use console::console;
pub use detect::{DetectOptions, detect};
pub use firmware::{
    FLASH_RISK, JUMP_RISK, find_flashed_meter, firmware_flash, firmware_info, firmware_jump,
    firmware_probe, firmware_probe_unavailable, inspect_flash_file,
};
pub use info::{info, info_in_mode};
pub use list::{list, select_device};
pub use pd::{pd, pd_decode, pd_save};
pub use pd_send::{PdSendOptions, pd_send};
pub use read::read;
pub use records::{
    DELETE_RISK, DeleteOptions, RecordSelection, records_delete, records_get, records_list,
};
pub use settings::{SETTINGS_RISK, SettingsOutput, settings, settings_set};
pub use stream::{CdcStreamSummary, StreamOptions, StreamSummary, stream, stream_cdc};
pub use trigger::{
    LegacyRequestOptions, PdRequestOptions, PdSettingsChange, trigger_legacy_list,
    trigger_legacy_request, trigger_pd_list, trigger_pd_request, trigger_pd_reset,
    trigger_pd_settings,
};
pub use watch::{WatchOptions, watch, watch_cdc};

/// Close the PDM `session` whatever `result` is; the first error wins.
fn close_after<T, P: Read + Write, C: Clock>(
    session: PdmSession<P, C>,
    result: Result<T, CliError>,
) -> Result<T, CliError> {
    let closed = session.close();
    let value = result?;
    closed?;
    Ok(value)
}
