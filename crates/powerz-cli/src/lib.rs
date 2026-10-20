//! Library side of the `powerz` command-line tool (FR STQEA1).
//!
//! Every subcommand is a function over a
//! [`Session`](powerz_protocol::session::Session) (or a device list) and a
//! [`Write`](std::io::Write) sink, so tests drive it with
//! [`MockTransport`](powerz_protocol::transport::MockTransport) and an
//! in-memory buffer. `main.rs` only parses arguments and calls [`run`].
//!
//! Command functions receive a session that is already open (and, on bulk,
//! already CONNECTed: memory read is REJECTed without it); they send only the requests their command
//! needs, starting with the session's next transaction id.
//!
//! Modules: [`args`] (clap definitions and validation), [`commands`] (one
//! function per subcommand), [`error`] (exit codes), [`record`] (CSV
//! helpers), [`trace`] (`--trace` transport wrapper).

mod app;
pub mod args;
pub mod commands;
pub mod error;
mod output;
pub mod record;
pub mod trace;

pub use app::run;
pub use args::{Cli, Command, TransportKind, validate};
pub use commands::{
    StreamOptions, StreamSummary, WatchOptions, info, list, read, select_device, stream, watch,
};
pub use error::CliError;
