//! PD sniffer session: start (control 0x10, mode 1), PD_PACKET polling and
//! stop (control 0x11), which is also sent on drop.

use std::time::Duration;

use super::{Clock, Error, Session};
use crate::decode::AdcSnapshot;
use crate::decode::pd_packet::{CaptureClock, PdChunk, PdRecord, PdStatus};
use crate::frame::{self, Attribute};
use crate::transport::Transport;

/// Options for [`PdSniffer::start`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PdOptions {
    /// Also request the ADC snapshot with every poll.
    pub with_adc: bool,
}

/// A captured PD record with its time since the capture start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedRecord {
    /// Ms since the capture start; never goes backwards.
    pub elapsed_ms: u64,
    /// The record as the meter sent it.
    pub record: PdRecord,
}

/// Result of one [`PdSniffer::poll`].
#[derive(Debug, Clone, PartialEq)]
pub struct PdPoll {
    /// Status block of the PD_PACKET section, if the reply had one.
    pub status: Option<PdStatus>,
    /// Records in the order they came.
    pub records: Vec<CapturedRecord>,
    /// Records skipped because they ran past their chunk.
    pub skipped: usize,
    /// ADC snapshot, when [`PdOptions::with_adc`] is set.
    pub adc: Option<AdcSnapshot>,
}

/// A running PD sniffer on a [`Session`]. Sends the stop request on
/// [`stop`](PdSniffer::stop) or, failing that, on drop.
#[derive(Debug)]
pub struct PdSniffer<'a, T: Transport, C: Clock> {
    session: &'a mut Session<T, C>,
    options: PdOptions,
    running: bool,
    clock: CaptureClock,
}

impl<'a, T: Transport, C: Clock> PdSniffer<'a, T, C> {
    /// Poll period of [`run`](PdSniffer::run), as the vendor app polls. \[static\]
    pub const POLL_PERIOD: Duration = Duration::from_millis(5);

    /// Start the PD sniffer, authenticating to level 1 first when needed.
    /// A REJECT is [`Error::Rejected`] and leaves nothing to stop.
    pub fn start(session: &'a mut Session<T, C>, options: PdOptions) -> Result<Self, Error> {
        session.start_pd_sniffer()?;
        Ok(PdSniffer {
            session,
            options,
            running: true,
            clock: CaptureClock::new(),
        })
    }

    /// Poll PD_PACKET once (with the ADC snapshot when asked for).
    pub fn poll(&mut self) -> Result<PdPoll, Error> {
        self.session.poll_pd_sniffer(self.options, &mut self.clock)
    }

    /// Poll every [`POLL_PERIOD`](Self::POLL_PERIOD), handing each poll to
    /// `on_poll` until it returns `false` or a poll fails.
    pub fn run(&mut self, mut on_poll: impl FnMut(&PdPoll) -> bool) -> Result<(), Error> {
        loop {
            let poll = self.poll()?;
            if !on_poll(&poll) {
                return Ok(());
            }
            self.session.sleep(Self::POLL_PERIOD);
        }
    }

    /// Stop the sniffer; a REJECT is [`Error::Rejected`].
    pub fn stop(mut self) -> Result<(), Error> {
        self.send_stop()
    }

    /// Send the stop request once.
    fn send_stop(&mut self) -> Result<(), Error> {
        if !self.running {
            return Ok(());
        }
        self.running = false;
        self.session.stop_pd_sniffer()
    }
}

/// The sniffer requests on their own, for a caller that keeps the session
/// between polls (the desktop worker): no borrow is held, so stopping is the
/// caller's job.
impl<T: Transport, C: Clock> Session<T, C> {
    /// Start the PD sniffer (0x10, mode 1), authenticating to level 1 first
    /// when needed. A REJECT is [`Error::Rejected`].
    pub fn start_pd_sniffer(&mut self) -> Result<(), Error> {
        if self.auth_level == 0 {
            self.authenticate(1)?;
        }
        let request = frame::start_pd_sniffer(self.counter.next());
        self.expect_accept(&request, "PD sniffer start")
    }

    /// Stop the PD sniffer (0x11); a REJECT is [`Error::Rejected`].
    pub fn stop_pd_sniffer(&mut self) -> Result<(), Error> {
        let request = frame::stop_analyzer(self.counter.next());
        self.expect_accept(&request, "PD sniffer stop")
    }

    /// Poll PD_PACKET once (with the ADC snapshot when `options` ask for
    /// it); `clock` turns record times into ms since the capture start.
    pub fn poll_pd_sniffer(
        &mut self,
        options: PdOptions,
        clock: &mut CaptureClock,
    ) -> Result<PdPoll, Error> {
        let mut mask = Attribute::PD_PACKET;
        if options.with_adc {
            mask = mask | Attribute::ADC;
        }
        let mut poll = PdPoll {
            status: None,
            records: Vec::new(),
            skipped: 0,
            adc: None,
        };
        for section in self.get_data(mask)? {
            if section.att == Attribute::PD_PACKET {
                // A section too short for its status block is counted like a
                // bad record; the capture goes on.
                let Ok(chunk) = PdChunk::parse(&section.payload) else {
                    poll.skipped += 1;
                    continue;
                };
                poll.status = Some(chunk.status);
                poll.skipped += chunk.skipped;
                for record in chunk.records {
                    poll.records.push(CapturedRecord {
                        elapsed_ms: clock.elapsed_ms(record.time_ms),
                        record,
                    });
                }
            } else if section.att == Attribute::ADC {
                poll.adc = Some(AdcSnapshot::decode(&section.payload)?);
            }
        }
        Ok(poll)
    }
}

impl<T: Transport, C: Clock> Drop for PdSniffer<'_, T, C> {
    fn drop(&mut self) {
        // Best effort: the meter may already be gone.
        let _ = self.send_stop();
    }
}
