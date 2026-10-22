//! One function per subcommand. Each takes an open [`Session`] (or a device
//! list for `list`) and [`Write`] sinks, so tests drive them with
//! [`MockTransport`](powerz_protocol::transport::MockTransport) and in-memory
//! buffers.
//!
//! [`Session`]: powerz_protocol::session::Session
//! [`Write`]: std::io::Write

mod info;
mod list;
mod pd;
mod read;
mod stream;
mod watch;

pub use info::info;
pub use list::{list, select_device};
pub use pd::pd;
pub use read::read;
pub use stream::{StreamOptions, StreamSummary, stream};
pub use watch::{WatchOptions, watch};
