//! `--trace`: a transport wrapper that logs every frame as hex.

use std::io::Write;
use std::time::Duration;

use powerz_protocol::transport::{Error, Transport};

/// Wraps a transport and writes each frame sent as `> <hex>` and each frame
/// received as `< <hex>` (one line per frame) to `sink`.
#[derive(Debug)]
pub struct TraceTransport<T, W> {
    inner: T,
    sink: W,
}

impl<T: Transport, W: Write> TraceTransport<T, W> {
    /// Trace `inner` into `sink`.
    pub fn new(inner: T, sink: W) -> TraceTransport<T, W> {
        TraceTransport { inner, sink }
    }

    /// The wrapped transport.
    pub fn inner(&self) -> &T {
        &self.inner
    }
}

impl<T: Transport, W: Write> TraceTransport<T, W> {
    /// Write one `marker hex` line; trace I/O errors never fail the transport.
    fn log(&mut self, marker: char, frame: &[u8]) {
        let mut line = String::with_capacity(2 + frame.len() * 3);
        line.push(marker);
        for b in frame {
            line.push_str(&format!(" {b:02x}"));
        }
        line.push('\n');
        let _ = self.sink.write_all(line.as_bytes());
        let _ = self.sink.flush();
    }
}

impl<T: Transport, W: Write> Transport for TraceTransport<T, W> {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        self.log('>', request);
        let reply = self.inner.transact(request, timeout)?;
        self.log('<', &reply);
        Ok(reply)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        let frame = self.inner.read(timeout)?;
        self.log('<', &frame);
        Ok(frame)
    }
}
