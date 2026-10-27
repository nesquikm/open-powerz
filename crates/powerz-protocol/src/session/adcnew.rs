//! ADCNEW push stream on the CDC port: start with content and rate, poll
//! the pushed records, stop, and the keep-alive that stops the meter from
//! ending the stream after an hour. See "CDC and the ADCNEW stream" in
//! `docs/protocol.md`.

use super::stream::{StreamPoll, gaps_after};
use super::{Clock, Error, Session};
use crate::adcnew::{AdcnewCounters, Content, Frame, KEEP_ALIVE_PERIOD, MAX_RAW_FRAMES, Request};
use crate::decode::Rate;
use crate::transport::Transport;

/// State of a running ADCNEW stream.
#[derive(Debug, Clone, Copy)]
pub(super) struct AdcnewStream {
    /// Session-clock time of the start or the last keep-alive, ms.
    last_refresh_ms: u64,
    /// Device time of the last record returned.
    last_record_ms: Option<u32>,
}

impl<T: Transport, C: Clock> Session<T, C> {
    /// Start the ADCNEW stream with `content` at `rate` (`02 <att>`). No
    /// auth and no queue start are needed; a running stream is replaced.
    /// Only the CDC transport streams: elsewhere nothing is written.
    pub fn start_adcnew(&mut self, content: Content, rate: Rate) -> Result<(), Error> {
        self.transport
            .adcnew_send(Request::Start { content, rate })?;
        self.adcnew = Some(AdcnewStream {
            last_refresh_ms: self.clock.now_ms(),
            last_record_ms: None,
        });
        Ok(())
    }

    /// Stop the ADCNEW stream (`03`).
    pub fn stop_adcnew(&mut self) -> Result<(), Error> {
        self.transport.adcnew_send(Request::Stop)?;
        self.adcnew = None;
        Ok(())
    }

    /// Read the records pushed since the last poll, with the gaps before or
    /// between them. Sends the keep-alive (`01`) once
    /// [`KEEP_ALIVE_PERIOD`] has passed since the start or the last one.
    /// Frames other than records (PD, UFCS content) are kept for
    /// [`take_adcnew_raw`](Self::take_adcnew_raw).
    pub fn poll_adcnew(&mut self) -> Result<StreamPoll, Error> {
        let Some(mut stream) = self.adcnew else {
            return Err(Error::Protocol("the ADCNEW stream is not running".into()));
        };
        let now = self.clock.now_ms();
        let period = u64::try_from(KEEP_ALIVE_PERIOD.as_millis()).unwrap_or(u64::MAX);
        if now.saturating_sub(stream.last_refresh_ms) >= period {
            self.transport.adcnew_send(Request::KeepAlive)?;
            stream.last_refresh_ms = now;
        }
        let frames = self.transport.adcnew_frames();
        let mut poll = StreamPoll {
            records: Vec::new(),
            gaps: Vec::new(),
        };
        let result = frames.map(|frames| {
            for frame in frames {
                match frame {
                    Frame::Records { rate, records } => {
                        let interval = 1000 / rate.sps().max(1);
                        poll.gaps.extend(gaps_after(
                            &mut stream.last_record_ms,
                            &records,
                            interval.max(1),
                        ));
                        poll.records.extend(records);
                    }
                    raw @ Frame::Raw { .. } => {
                        // Nobody may ever take them: keep the newest only.
                        if self.adcnew_raw.len() >= MAX_RAW_FRAMES {
                            self.adcnew_raw.remove(0);
                        }
                        self.adcnew_raw.push(raw);
                    }
                }
            }
        });
        self.adcnew = Some(stream);
        result?;
        Ok(poll)
    }

    /// Frames of the stream lost on the host side since the transport was
    /// opened: dropped for a bad CRC or length, or at the hold cap.
    pub fn adcnew_counters(&self) -> AdcnewCounters {
        self.transport.adcnew_counters()
    }

    /// Take the non-record ADCNEW frames (PD / UFCS content, kept raw)
    /// received so far, at most [`MAX_RAW_FRAMES`](crate::adcnew::MAX_RAW_FRAMES) of the newest.
    pub fn take_adcnew_raw(&mut self) -> Vec<Frame> {
        std::mem::take(&mut self.adcnew_raw)
    }
}
