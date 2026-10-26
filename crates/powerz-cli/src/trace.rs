//! `--trace`: a transport wrapper that logs every frame as hex, and a port
//! wrapper that logs every PDM command written.

use std::io::{Read, Write};
use std::time::Duration;

use powerz_protocol::firmware::flash::{FlashTransport, Meter, MeterOpener};
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

    fn recover(&mut self) -> Result<(), Error> {
        self.inner.recover()
    }
}

/// The flash path's raw writes (jumps, the descriptor, payload blocks):
/// `>` once written, `x` when the flash guard refused them before sending.
impl<T: FlashTransport, W: Write> FlashTransport for TraceTransport<T, W> {
    fn write_raw(&mut self, data: &[u8], timeout: Duration) -> Result<(), Error> {
        match self.inner.write_raw(data, timeout) {
            Err(e @ Error::Forbidden { .. }) => {
                self.log('x', data);
                Err(e)
            }
            result => {
                self.log('>', data);
                result
            }
        }
    }
}

/// A [`MeterOpener`] whose opened pipes are traced to stderr, for
/// `firmware jump` and `firmware flash` with `--trace`.
#[derive(Debug)]
pub struct TracedOpener<O>(pub O);

impl<O: MeterOpener> MeterOpener for TracedOpener<O> {
    type Pipe = TraceTransport<O::Pipe, std::io::Stderr>;

    fn open(&mut self) -> Result<Option<Meter<Self::Pipe>>, Error> {
        Ok(self.0.open()?.map(|m| Meter {
            mode: m.mode,
            model: m.model,
            pipe: TraceTransport::new(m.pipe, std::io::stderr()),
        }))
    }
}

/// Wraps a PDM serial port and writes each command written to it as a
/// `> <command>` line to `sink`, and each chunk read as a `< <text>` line.
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

impl<P: Read, W: Write> Read for TracePort<P, W> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            // Printable ASCII as is; `\n` and other bytes escaped, so binary
            // capability blocks and frames stay on one trace line.
            let text: String = buf
                .get(..n)
                .unwrap_or_default()
                .iter()
                .map(|&b| match b {
                    b'\n' => "\\n".to_string(),
                    0x20..=0x7e => (b as char).to_string(),
                    _ => format!("\\x{b:02x}"),
                })
                .collect();
            // Trace I/O errors never fail the port.
            let _ = writeln!(self.sink, "< {text}");
            let _ = self.sink.flush();
        }
        Ok(n)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A port that answers every read with `reply` once.
    struct Echo {
        reply: Vec<u8>,
    }

    impl Read for Echo {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.reply.len().min(buf.len());
            buf[..n].copy_from_slice(&self.reply[..n]);
            self.reply.drain(..n);
            Ok(n)
        }
    }

    impl Write for Echo {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // ---- AC-PR41FT.2: replies are visible in --trace, binary bytes escaped ----

    #[test]
    fn received_bytes_are_traced_as_escaped_text() {
        let mut sink = Vec::new();
        {
            let mut p = TracePort::new(
                Echo {
                    reply: b"pdo:4\x0b\nok\n".to_vec(),
                },
                &mut sink,
            );
            p.write_all(b"entry pd").unwrap();
            let mut buf = [0u8; 64];
            let _ = p.read(&mut buf).unwrap();
        }
        let text = String::from_utf8(sink).unwrap();
        assert_eq!(text, "> entry pd\n< pdo:4\\x0b\\nok\\n\n");
    }
    /// A flash pipe that refuses one raw frame type and counts recovers.
    #[derive(Default)]
    struct Pipe {
        refuse: Option<u8>,
        recovers: usize,
    }

    impl Transport for Pipe {
        fn transact(&mut self, _request: &[u8], _timeout: Duration) -> Result<Vec<u8>, Error> {
            Ok(vec![0x05, 0x01, 0x00, 0x00])
        }

        fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, Error> {
            Err(Error::Timeout)
        }

        fn recover(&mut self) -> Result<(), Error> {
            self.recovers += 1;
            Ok(())
        }
    }

    impl FlashTransport for Pipe {
        fn write_raw(&mut self, data: &[u8], _timeout: Duration) -> Result<(), Error> {
            match self.refuse {
                Some(ty) if data.first() == Some(&ty) => Err(Error::Forbidden { ty }),
                _ => Ok(()),
            }
        }
    }

    const T: Duration = Duration::from_millis(10);

    // ---- AC-V9SK56.8: raw writes of the flash path are traced ----

    #[test]
    fn raw_writes_are_traced_as_sent_and_refused_ones_as_refused() {
        let mut sink = Vec::new();
        {
            let pipe = Pipe {
                refuse: Some(0x42),
                ..Pipe::default()
            };
            let mut t = TraceTransport::new(pipe, &mut sink);
            t.write_raw(&[0x09, 0x01, 0x00, 0x00], T).unwrap();
            assert!(t.write_raw(&[0x42, 0x02], T).is_err());
        }
        let text = String::from_utf8(sink).unwrap();
        assert_eq!(text, "> 09 01 00 00\nx 42 02\n");
    }

    #[test]
    fn recover_reaches_the_wrapped_transport() {
        let mut t = TraceTransport::new(Pipe::default(), Vec::new());
        t.recover().unwrap();
        assert_eq!(t.inner().recovers, 1);
    }
}
