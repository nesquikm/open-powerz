//! `--trace`: a transport wrapper that logs every frame as hex, and a port
//! wrapper that logs every PDM command written.

use std::io::{Read, Write};
use std::time::Duration;

use powerz_protocol::transport::{Error, Transport};

/// Wraps a transport and writes each frame sent as `> <hex>`, each frame
/// received as `< <hex>` and each frame the guard refused before sending as
/// `x <hex>` (one line per frame) to `sink`.
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
        match self.inner.transact(request, timeout) {
            // Refused before any byte was written: say so, never "sent".
            Err(
                e @ (Error::Forbidden { .. }
                | Error::UnsupportedOnHid
                | Error::UnsupportedOnCdc { .. }),
            ) => {
                self.log('x', request);
                Err(e)
            }
            result => {
                self.log('>', request);
                let reply = result?;
                self.log('<', &reply);
                Ok(reply)
            }
        }
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        let frame = self.inner.read(timeout)?;
        self.log('<', &frame);
        Ok(frame)
    }
}

/// Wraps a PDM serial port and writes each command written to it as a
/// `> <command>` line to `sink`; reads pass through untouched.
#[derive(Debug)]
pub struct TracePort<P, W> {
    inner: P,
    sink: W,
}

impl<P: Read + Write, W: Write> TracePort<P, W> {
    /// Trace `inner` into `sink`.
    pub fn new(inner: P, sink: W) -> TracePort<P, W> {
        TracePort { inner, sink }
    }
}

impl<P: Read, W> Read for TracePort<P, W> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<P: Write, W: Write> Write for TracePort<P, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        let text = String::from_utf8_lossy(buf.get(..n).unwrap_or_default());
        // Trace I/O errors never fail the port.
        let _ = writeln!(self.sink, "> {}", text.trim_end());
        let _ = self.sink.flush();
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
