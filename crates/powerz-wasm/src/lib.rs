//! WebAssembly binding for the POWER-Z protocol core (FR QFTCWY): request
//! builders, reply decoders and a demo meter over the simulator. Sans-IO:
//! the web page moves the bytes.

mod builders;
mod db_export;
mod decoders;
mod demo;
mod error;
mod logo;
mod meter_reads;
mod recording_file;
mod records_delete;
mod settings_write;
mod trigger;

pub use builders::{
    auth_request, connect_request, get_data_request, memory_read_request, pd_poll_request,
    start_pd_sniffer_request, start_queue_request, stop_pd_sniffer_request, stop_queue_request,
};
pub use db_export::{
    DbRows, db_create_sql, db_insert_param_sql, db_insert_row_sql, db_param_values, db_table_name,
};
pub use decoders::{
    Adc, DeviceInfoView, Direction, PdCleared, PdDecoder, PdDetails, PdPollResult, PdRow,
    PdStatusSample, PdcapFile, QueueSample, RowKind, decode_accept, decode_adc, decode_adc_js,
    decode_auth_reply, decode_device_info, decode_device_info_js, decode_memory_read, decode_queue,
    decode_queue_js,
};
pub use demo::{DemoCdc, DemoMeter};
pub use error::BindError;
pub use logo::{
    LogoPreviewView, LogoReadView, LogoReader, LogoSettingsCheckView, LogoWrite, LogoWrittenView,
    logo_preview, logo_preview_builtin, settings_save_check,
};
pub use meter_reads::{
    Chunk, OfflineRecordView, RecordBook, RecordEntryView, RecordListView, RecordSampleView,
    SettingsView, decode_settings, decode_settings_js, settings_request,
};
pub use recording_file::{
    DbRecording, OpenedRows, pick_db_table, read_recording_csv, read_recording_csv_js,
};
pub use records_delete::BackupFile;
pub use settings_write::{SettingsWrite, cdc_frame_length};
pub use trigger::{TriggerClockJs, TriggerEngine, TriggerPortJs, TriggerVbusJs};
