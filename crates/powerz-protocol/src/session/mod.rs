//! Device session over any [`Transport`]: CONNECT, one-in-flight
//! request/reply matching, memory read, device info, authentication and
//! ADC-queue streaming. See `docs/protocol.md`.
//!
//! The [`Session`] methods are grouped by feature:
//!
//! - core (this module): [`connect`](Session::connect),
//!   [`get_data`](Session::get_data), [`read_adc`](Session::read_adc),
//!   [`read_settings`](Session::read_settings);
//! - memory read: [`memory_read`](Session::memory_read),
//!   [`device_info`](Session::device_info);
//! - authentication: [`authenticate`](Session::authenticate);
//! - ADC-queue streaming: [`start_stream`](Session::start_stream),
//!   [`stop_stream`](Session::stop_stream),
//!   [`poll_stream`](Session::poll_stream).

mod adcnew;
mod auth;
mod memory;
mod pd;
mod probe;
mod records;
mod stream;

use std::time::Duration;

use crate::decode::{AdcSnapshot, DecodeError, Rate, Settings};
use crate::frame::{self, Attribute, FrameError, Reply, SequenceCounter};
use crate::transport::{self, Transport};

pub use auth::{auth_plaintext, auth_request};
pub(crate) use memory::UID;
pub use memory::{
    DeviceInfo, FirmwareProblem, FirmwareProblemKind, FirmwareStatus, memory_read_plaintext,
    memory_read_request,
};
pub use pd::{CapturedRecord, PdOptions, PdPoll, PdSniffer};
pub use probe::{ConnectOutcome, HeadOutcome, ProbeReport};
pub use stream::{Gap, SnapshotCadence, StreamPoll, Streamer};

/// Errors from a [`Session`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The transport failed (timeout, unplug, ...).
    #[error("transport: {0}")]
    Transport(#[from] transport::Error),
    /// The meter answered with something the protocol does not allow here.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// The meter answered REJECT.
    #[error("request type {request_type:#04x} was rejected")]
    Rejected {
        /// Message type of the rejected request.
        request_type: u8,
    },
    /// The memory read is not one of the known safe blocks; nothing was sent.
    #[error("memory read of {len} bytes at {addr:#x} is not allowed")]
    Forbidden {
        /// Requested address.
        addr: u32,
        /// Requested length.
        len: u32,
    },
    /// Authentication did not succeed.
    #[error("authentication failed")]
    AuthFailed,
    /// The meter answered SYNC (`01 id 00 00`): a bootloader left in its
    /// download phase `[device]`, which answers every frame that way. Only a
    /// power cycle into the bootloader gets it out.
    #[error(
        "the meter answered SYNC: its bootloader was left mid-download and answers nothing \
         else; unplug the meter, then hold Back while plugging it back in"
    )]
    BootloaderMidDownload,
    /// A reply could not be decoded.
    #[error(transparent)]
    Decode(#[from] DecodeError),
    /// A reply frame was malformed.
    #[error(transparent)]
    Frame(#[from] FrameError),
}

impl Error {
    /// The meter was unplugged or the OS refused access: retrying on this
    /// link cannot help.
    pub fn is_link_fatal(&self) -> bool {
        matches!(
            self,
            Error::Transport(transport::Error::Disconnected | transport::Error::Permission(_))
        )
    }
}

/// Source of wall-clock time, randomness and sleeping, injected so that
/// authentication and request spacing are deterministic in tests.
pub trait Clock {
    /// Current wall-clock time in milliseconds since the Unix epoch; a
    /// [`sleep`](Clock::sleep) must advance it.
    fn now_ms(&mut self) -> u64;
    /// A fresh random value.
    fn random_u64(&mut self) -> u64;
    /// Block for `duration`.
    fn sleep(&mut self, duration: Duration);
}

/// The real [`Clock`]: system time, OS-seeded randomness, thread sleep.
#[derive(Debug, Clone, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64)
    }
    fn random_u64(&mut self) -> u64 {
        // The auth nonce comes from the OS random source.
        getrandom::u64().unwrap_or_else(|_| self.now_ms().rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// One PUT_DATA section, owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedSection {
    /// Attribute of the section.
    pub att: Attribute,
    /// Section payload, header excluded.
    pub payload: Vec<u8>,
}

/// Mismatched-id replies discarded before a request fails.
const MAX_STALE_REPLIES: usize = 3;

/// Error for a reply that does not answer `request` the way the protocol
/// requires: REJECT becomes [`Error::Rejected`], anything else
/// [`Error::Protocol`] naming the request as `what`.
fn unexpected_reply(request: &[u8], what: &str, reply: Reply) -> Error {
    match reply {
        Reply::Reject(_) => Error::Rejected {
            request_type: request[0],
        },
        other => Error::Protocol(format!("unexpected reply to {what}: {other:?}")),
    }
}

/// Whether `reply` is SYNC (`01 id 00 00`), what a bootloader in its
/// download phase answers to every frame `[device]`.
fn is_sync(reply: &[u8]) -> bool {
    matches!(reply, [0x01, _, 0x00, 0x00])
}

/// A session with the meter over `T`.
#[derive(Debug)]
pub struct Session<T: Transport, C: Clock = SystemClock> {
    transport: T,
    clock: C,
    counter: SequenceCounter,
    auth_level: u8,
    last_auth_ms: Option<u64>,
    stream: Option<Rate>,
    /// The meter's own queue rate, as last reported by ADC or SETTINGS.
    meter_rate: Option<Rate>,
    /// Device time of the last record returned by `poll_stream`.
    last_record_ms: Option<u32>,
    /// Read windows of the records in the last decoded RecordList: the only
    /// record-area reads [`Session::download_record`] may send.
    record_windows: Vec<(u32, u32)>,
    /// The running ADCNEW stream on CDC, if any.
    adcnew: Option<adcnew::AdcnewStream>,
    /// PD / UFCS (and other non-record) ADCNEW frames not yet taken.
    adcnew_raw: Vec<crate::adcnew::Frame>,
}

impl<T: Transport> Session<T, SystemClock> {
    /// Open a session over `transport`; no CONNECT is sent.
    pub fn open(transport: T) -> Self {
        Session::with_clock(transport, SystemClock)
    }
}

impl<T: Transport, C: Clock> Session<T, C> {
    /// Open a session with an injected clock; no CONNECT is sent.
    pub fn with_clock(transport: T, clock: C) -> Self {
        Session {
            transport,
            clock,
            counter: SequenceCounter::new(),
            auth_level: 0,
            last_auth_ms: None,
            stream: None,
            meter_rate: None,
            last_record_ms: None,
            record_windows: Vec::new(),
            adcnew: None,
            adcnew_raw: Vec::new(),
        }
    }

    /// The session clock's current time, ms since the Unix epoch.
    pub fn now_ms(&mut self) -> u64 {
        self.clock.now_ms()
    }

    /// Block for `duration` on the session clock.
    pub fn sleep(&mut self, duration: Duration) {
        self.clock.sleep(duration);
    }

    /// The underlying transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// The underlying transport, mutably.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Auth level last reported by the meter (0, 1 or 2).
    pub fn auth_level(&self) -> u8 {
        self.auth_level
    }

    /// Send `request` with the transport's default timeout; return the reply
    /// whose id matches the request id (id 0 for a 0x4C request). Up to
    /// [`MAX_STALE_REPLIES`] mismatched replies are discarded.
    fn exchange(&mut self, request: &[u8]) -> Result<Vec<u8>, Error> {
        self.exchange_within(request, transport::default_timeout(request))
    }

    /// [`exchange`](Self::exchange) waiting `timeout` for each transfer.
    fn exchange_within(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        self.exchange_taking_first(request, timeout, |_| false)
    }

    /// [`exchange_within`](Self::exchange_within) that also takes the first
    /// transfer when `take_first` accepts it, whatever its id. Re-reads still
    /// need the request id.
    fn exchange_taking_first(
        &mut self,
        request: &[u8],
        timeout: Duration,
        take_first: impl Fn(&[u8]) -> bool,
    ) -> Result<Vec<u8>, Error> {
        let expected_id = if request[0] == 0x4C { 0 } else { request[1] };
        let mut reply = self.transport.transact(request, timeout)?;
        if take_first(&reply) {
            return Ok(reply);
        }
        let mut rereads = 0;
        while reply.get(1) != Some(&expected_id) {
            if rereads == MAX_STALE_REPLIES {
                return Err(Error::Protocol(format!(
                    "no reply with id {expected_id:#04x} after {MAX_STALE_REPLIES} re-reads"
                )));
            }
            reply = self.transport.read(timeout)?;
            rereads += 1;
        }
        Ok(reply)
    }

    /// Send a control request that must be answered with ACCEPT; `what`
    /// names the request in the error message.
    fn expect_accept(&mut self, request: &[u8], what: &str) -> Result<(), Error> {
        let timeout = transport::default_timeout(request);
        self.expect_accept_within(request, what, timeout)
    }

    /// [`expect_accept`](Self::expect_accept) waiting `timeout` per transfer.
    fn expect_accept_within(
        &mut self,
        request: &[u8],
        what: &str,
        timeout: Duration,
    ) -> Result<(), Error> {
        self.expect_accept_checked(request, what, timeout, false)
    }

    /// [`expect_accept_within`](Self::expect_accept_within); with
    /// `stop_on_sync`, a SYNC reply is [`Error::BootloaderMidDownload`].
    fn expect_accept_checked(
        &mut self,
        request: &[u8],
        what: &str,
        timeout: Duration,
        stop_on_sync: bool,
    ) -> Result<(), Error> {
        let reply = self.exchange_within(request, timeout)?;
        if stop_on_sync && is_sync(&reply) {
            return Err(Error::BootloaderMidDownload);
        }
        match frame::classify_reply(&reply)? {
            Reply::Accept(_) => Ok(()),
            other => Err(unexpected_reply(request, what, other)),
        }
    }

    /// Send CONNECT; `Ok` only on ACCEPT. Stale replies with another id
    /// (e.g. a leftover DISCONNECT) are skipped. When no matching ACCEPT
    /// arrives within [`transport::FIRST_CONNECT_TIMEOUT`], the transport is
    /// recovered once ([`Transport::recover`]) and CONNECT is retried once.
    /// A REJECT is returned at once.
    pub fn connect(&mut self) -> Result<(), Error> {
        self.connect_checked(false)
    }

    /// [`connect`](Self::connect); with `stop_on_sync`, a SYNC reply to
    /// either CONNECT is [`Error::BootloaderMidDownload`] and nothing more
    /// is sent.
    fn connect_checked(&mut self, stop_on_sync: bool) -> Result<(), Error> {
        let request = frame::connect(self.counter.next());
        let first = match self.expect_accept_checked(
            &request,
            "CONNECT",
            transport::FIRST_CONNECT_TIMEOUT,
            stop_on_sync,
        ) {
            Ok(()) => return Ok(()),
            // No reply, or only stale frames: the pipe may be stuck.
            Err(e @ (Error::Transport(transport::Error::Timeout) | Error::Protocol(_))) => e,
            // A REJECT, an unplugged meter or an I/O failure: recovery cannot help.
            Err(e) => return Err(e),
        };
        // A recovery that fails says less than the CONNECT that failed.
        if self.transport.recover().is_err() {
            return Err(first);
        }
        let request = frame::connect(self.counter.next());
        let timeout = transport::default_timeout(&request);
        self.expect_accept_checked(&request, "CONNECT", timeout, stop_on_sync)
    }

    /// GET_DATA with `mask`; a DISCONNECT reply gives an empty list.
    pub fn get_data(&mut self, mask: Attribute) -> Result<Vec<OwnedSection>, Error> {
        let request = frame::get_data(self.counter.next(), mask);
        let reply = self.exchange(&request)?;
        match frame::classify_reply(&reply)? {
            Reply::PutData(h) => {
                self.auth_level = h.data().permi;
                if reply.len() == frame::HEADER_LEN {
                    return Ok(Vec::new());
                }
                // A cut last ADC_QUEUE section is kept [device]; see
                // `frame::parse_put_data_cut_queue`.
                Ok(frame::parse_put_data_cut_queue(&reply)?
                    .into_iter()
                    .map(|s| OwnedSection {
                        att: s.att,
                        payload: s.payload.to_vec(),
                    })
                    .collect())
            }
            Reply::Disconnect(_) => Ok(Vec::new()),
            other => Err(unexpected_reply(&request, "GET_DATA", other)),
        }
    }

    /// GET_DATA for the single attribute `att`; return the payload of its
    /// section, `name` naming it in the error when the reply lacks it.
    fn get_section(&mut self, att: Attribute, name: &str) -> Result<Vec<u8>, Error> {
        self.get_data(att)?
            .into_iter()
            .find(|s| s.att == att)
            .map(|s| s.payload)
            .ok_or_else(|| Error::Protocol(format!("reply has no {name} section")))
    }

    /// Read and decode one ADC snapshot.
    pub fn read_adc(&mut self) -> Result<AdcSnapshot, Error> {
        let payload = self.get_section(Attribute::ADC, "ADC")?;
        let adc = AdcSnapshot::decode(&payload)?;
        if let Some(rate) = adc.rate_code().and_then(meter_rate_from_code) {
            self.meter_rate = Some(rate);
        }
        Ok(adc)
    }

    /// Read and decode the SETTINGS block.
    pub fn read_settings(&mut self) -> Result<Settings, Error> {
        let payload = self.get_section(Attribute::SETTINGS, "SETTINGS")?;
        let settings = Settings::decode(&payload)?;
        if let Some(rate) = meter_rate_from_code(settings.rate_code()) {
            self.meter_rate = Some(rate);
        }
        Ok(settings)
    }

    /// Read the SETTINGS block and decode its typed fields; a CRC mismatch
    /// is reported as [`DecodeError::Crc`].
    pub fn read_meter_settings(&mut self) -> Result<crate::decode::settings::MeterSettings, Error> {
        Ok(self.read_settings()?.meter_settings()?)
    }
}

/// The rate the meter actually runs at for a reported code: code 4 is
/// accepted by FW 2.0.6 but behaves like code 0 (2 SPS, 0.1 mV units).
fn meter_rate_from_code(code: u8) -> Option<Rate> {
    if code == 4 {
        Some(Rate::Sps2)
    } else {
        Rate::from_code(code)
    }
}
