//! `powerz firmware info FILE` (FR V9SK53), `powerz firmware probe`
//! (FR V9SK56, AC .6), `powerz firmware flash FILE --i-understand`
//! (FR V9SK54, AC .7) and `powerz firmware jump --to MODE --i-understand`
//! (FR V9SK56, AC .7).

use std::cell::Cell;
use std::io::Write;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use powerz_protocol::decode::head::hex;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::container::FirmwareContainer;
use powerz_protocol::firmware::flash::{
    BACK_BUTTON_HINT, FlashAck, FlashOutcome, FlashProgress, FlashTransport, JumpOutcome, JumpSent,
    JumpTarget, Meter, MeterOpener, RETURN_TIMEOUT, SWITCH_TIMEOUT, flash, jump,
};
use powerz_protocol::session::{
    Clock, ConnectOutcome, Error as SessionError, HeadOutcome, ProbePipe, ProbeReport, Session,
};
use powerz_protocol::transport::discovery::{
    self, DeviceEntry, StringDescriptor, UsbDetails, UsbIdentity,
};
use powerz_protocol::transport::{Error as TransportError, TimeoutDirection, Transport};

use crate::error::CliError;
use crate::output::write_json;

/// `powerz firmware info`: the metadata of the firmware container in
/// `bytes` (read from the file `name`) and its English release notes.
///
/// JSON keys `vendor`, `container_version`, `model`, `version`, `date`,
/// `crc` (8 uppercase hex digits), `size` (declared image bytes) and
/// `release_notes_en`. A refused container is a usage error naming the file
/// and the reason; nothing is written to `out` then. The payload is never
/// decrypted.
pub fn firmware_info(
    bytes: &[u8],
    name: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let c = FirmwareContainer::parse(bytes).map_err(|e| CliError::Usage(format!("{name}: {e}")))?;
    let crc = c.crc_hex();
    if json {
        return write_json(
            out,
            &serde_json::json!({
                "vendor": c.vendor(),
                "container_version": c.container_version(),
                "model": c.model(),
                "version": c.firmware_version(),
                "date": c.date(),
                "crc": crc,
                "size": c.size(),
                "release_notes_en": c.release_notes_en(),
            }),
        );
    }
    writeln!(out, "Model:      {}", c.model())?;
    writeln!(out, "Version:    {}", c.firmware_version())?;
    writeln!(out, "Date:       {}", c.date())?;
    writeln!(out, "CRC:        {crc}")?;
    writeln!(out, "Size:       {} bytes", c.size())?;
    writeln!(out, "Container:  {}", c.container_version())?;
    writeln!(out, "Vendor:     {}", c.vendor())?;
    writeln!(out, "Release notes:")?;
    for line in c.release_notes_en().lines() {
        writeln!(out, "  {line}")?;
    }
    Ok(())
}

/// `powerz firmware probe`: print `identity`, then send only CONNECT and
/// HEAD on `session` (which must not be connected yet) and print the
/// answers.
///
/// Text: the identity lines, then `CONNECT: accepted | rejected | timed
/// out`, then `HEAD field: <n>` (plus `HEAD reply (raw): <hex bytes>` for an
/// id-0 reply),
/// `HEAD reply (raw): <hex bytes>` alone (for a REJECT followed by `HEAD: rejected`) or `HEAD: timed out`. The identity is written (and flushed)
/// before anything is sent. Any other failure of CONNECT or HEAD (the
/// pipe would not open, I/O, unplugged) is returned as an error and no
/// CONNECT / HEAD line or JSON report is written. JSON keys `vid`, `pid`, `product`, `mode`,
/// `interfaces` (`number`, `class`, `subclass`, `protocol`, `string`),
/// `connect`, `head` (`field`, `raw`, `rejected` or `timed out`),
/// `head_field` and `head_raw` (hex, no spaces), `connect_timeout` and
/// `head_timeout` (`in` or `out`: which half timed out), `null` when absent.
/// A timed-out CONNECT or HEAD line names that half: `(OUT write)` or
/// `(IN read)`, when the pipe knows it.
pub fn firmware_probe<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    identity: &UsbIdentity,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    write_probe(identity, None, json, out, || {
        session.probe_with_head_reply()
    })
}

/// [`firmware_probe`] with the diagnostics of FR QF92WX: each interface
/// line is followed by its endpoints from `details`
/// (`    endpoint 0x82: IN, interrupt, max packet 8, interval 10`), and the
/// JSON interfaces carry an `endpoints` array (`address`, `direction`,
/// `transfer`, `max_packet_size`, `interval`).
pub fn firmware_probe_detailed<T: ProbePipe, C: Clock>(
    session: &mut Session<T, C>,
    identity: &UsbIdentity,
    details: &UsbDetails,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mode = identity.mode();
    write_probe(identity, Some(details), json, out, || {
        session.probe_with_variants(mode)
    })
}

/// The probe printer shared by [`firmware_probe`] and
/// [`firmware_probe_detailed`]: identity (with endpoints when `details`),
/// then the outcome `run` gives.
fn write_probe(
    identity: &UsbIdentity,
    details: Option<&UsbDetails>,
    json: bool,
    out: &mut dyn Write,
    run: impl FnOnce() -> Result<(ProbeReport, Option<Vec<u8>>), SessionError>,
) -> Result<(), CliError> {
    if !json {
        let mut text = String::new();
        identity
            .write_text(&mut text, details)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        write!(out, "{text}")?;
        out.flush()?;
    }
    let (report, head_reply) = run()?;
    let connect = connect_name(&report.connect);
    let head = head_name(&report.head);
    let (field, raw) = match &report.head {
        // An id-0 reply (the real meter's answer `[device]`) keeps its raw
        // bytes next to the field: they are not an echo of our request.
        HeadOutcome::Field(f) => (
            Some(*f),
            head_reply.as_deref().filter(|r| r.get(1) == Some(&0)),
        ),
        HeadOutcome::Raw(r) | HeadOutcome::Rejected(r) => (None, Some(r.as_slice())),
        HeadOutcome::TimedOut => (None, None),
    };
    if json {
        let raw = raw.map(|r| hex(r, ""));
        let mut v = probe_json(
            identity,
            details,
            Some(connect),
            Some(head),
            field,
            raw,
            [report.connect_timeout, report.head_timeout],
        );
        let variants: Vec<serde_json::Value> = report
            .variants
            .iter()
            .map(|o| {
                let field = match o.head {
                    HeadOutcome::Field(f) => Some(f),
                    _ => None,
                };
                let connect_raw = match &o.connect {
                    ConnectOutcome::Raw(r) => Some(hex(r, "")),
                    _ => None,
                };
                let head_raw = match &o.head {
                    HeadOutcome::Raw(r) | HeadOutcome::Rejected(r) => Some(hex(r, "")),
                    _ => None,
                };
                serde_json::json!({
                    "name": o.variant.name(),
                    "connect": connect_name(&o.connect),
                    "connect_raw": connect_raw,
                    "head": head_name(&o.head),
                    "head_field": field,
                    "head_raw": head_raw,
                })
            })
            .collect();
        v["variants"] = serde_json::Value::Array(variants);
        v["variant_answered"] = serde_json::json!(report.answered_variant().map(|a| a.name()));
        return write_json(out, &v);
    }
    let with_direction = |outcome: &str, dir: Option<TimeoutDirection>| match dir {
        Some(d) => format!("{outcome} ({})", d.label()),
        None => outcome.to_owned(),
    };
    writeln!(
        out,
        "CONNECT: {}",
        with_direction(connect, report.connect_timeout)
    )?;
    match (field, raw) {
        (Some(f), raw) => {
            writeln!(out, "HEAD field: {f}")?;
            if let Some(r) = raw {
                writeln!(out, "HEAD reply (raw): {}", hex(r, " "))?;
            }
        }
        (None, Some(r)) => {
            writeln!(out, "HEAD reply (raw): {}", hex(r, " "))?;
            if matches!(report.head, HeadOutcome::Rejected(_)) {
                writeln!(out, "HEAD: {head}")?;
            }
        }
        (None, None) => writeln!(out, "HEAD: {}", with_direction(head, report.head_timeout))?,
    }
    if report.variants.is_empty() {
        return Ok(());
    }
    for o in &report.variants {
        let head = match &o.head {
            HeadOutcome::Field(f) => format!("field {f}"),
            HeadOutcome::Raw(r) => format!("raw {}", hex(r, " ")),
            HeadOutcome::Rejected(r) => format!("rejected {}", hex(r, " ")),
            HeadOutcome::TimedOut => "timed out".to_owned(),
        };
        let connect = match &o.connect {
            ConnectOutcome::Raw(r) => format!("raw {}", hex(r, " ")),
            c => connect_name(c).to_owned(),
        };
        writeln!(
            out,
            "variant {}: CONNECT {connect}, HEAD {head}",
            o.variant.name()
        )?;
    }
    writeln!(
        out,
        "First variant to answer: {}",
        report.answered_variant().map_or("none", |v| v.name())
    )?;
    Ok(())
}

/// The CONNECT outcome as printed and in JSON.
fn connect_name(c: &ConnectOutcome) -> &'static str {
    match c {
        ConnectOutcome::Accepted => "accepted",
        ConnectOutcome::Rejected => "rejected",
        ConnectOutcome::Raw(_) => "raw",
        ConnectOutcome::TimedOut => "timed out",
    }
}

/// The HEAD outcome's kind as in JSON.
fn head_name(h: &HeadOutcome) -> &'static str {
    match h {
        HeadOutcome::Field(_) => "field",
        HeadOutcome::Raw(_) => "raw",
        HeadOutcome::Rejected(_) => "rejected",
        HeadOutcome::TimedOut => "timed out",
    }
}

/// `powerz firmware probe` on a meter without a vendor-class (0xFF)
/// interface: print `identity` (text or JSON with `connect` and `head`
/// `null`, `variants` empty) and say on `err` that the probe cannot run;
/// nothing is sent.
pub fn firmware_probe_unavailable(
    identity: &UsbIdentity,
    json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    if json {
        let mut v = probe_json(identity, None, None, None, None, None, [None, None]);
        v["variants"] = serde_json::Value::Array(Vec::new());
        v["variant_answered"] = serde_json::Value::Null;
        write_json(out, &v)?;
    } else {
        write!(out, "{identity}")?;
    }
    writeln!(
        err,
        "no vendor-class (0xFF) interface: the probe cannot run; nothing was sent"
    )?;
    Ok(())
}

/// The probe JSON object: `identity` plus the CONNECT / HEAD answers
/// (`None` becomes `null`).
fn probe_json(
    identity: &UsbIdentity,
    details: Option<&UsbDetails>,
    connect: Option<&str>,
    head: Option<&str>,
    head_field: Option<u32>,
    head_raw: Option<String>,
    [connect_timeout, head_timeout]: [Option<TimeoutDirection>; 2],
) -> serde_json::Value {
    let interfaces: Vec<serde_json::Value> = identity
        .interfaces
        .iter()
        .map(|i| {
            let mut v = serde_json::json!({
                "number": i.number,
                "class": i.class,
                "subclass": i.subclass,
                "protocol": i.protocol,
                "string": i.string,
            });
            if let Some(d) = details {
                let endpoints: Vec<serde_json::Value> = d
                    .endpoints_of(i.number)
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "address": e.address,
                            "direction": e.direction.name(),
                            "transfer": e.transfer.name(),
                            "max_packet_size": e.max_packet_size,
                            "interval": e.interval,
                        })
                    })
                    .collect();
                v["endpoints"] = serde_json::Value::Array(endpoints);
            }
            v
        })
        .collect();
    let mut v = serde_json::json!({
        "vid": identity.vid,
        "pid": identity.pid,
        "product": identity.product,
        "mode": identity.mode().name(),
        "interfaces": interfaces,
        "connect": connect,
        "head": head,
        "head_field": head_field,
        "head_raw": head_raw,
        "connect_timeout": connect_timeout.map(TimeoutDirection::name),
        "head_timeout": head_timeout.map(TimeoutDirection::name),
    });
    if let Some(d) = details {
        let strings: serde_json::Map<String, serde_json::Value> = d
            .strings
            .iter()
            .map(|(index, s)| {
                let value = match s {
                    StringDescriptor::Text(t) => serde_json::json!(t),
                    StringDescriptor::Absent => serde_json::Value::Null,
                    StringDescriptor::Unavailable(r) => serde_json::json!({ "unavailable": r }),
                };
                (index.to_string(), value)
            })
            .collect();
        v["strings"] = serde_json::Value::Object(strings);
    }
    v
}

/// What `firmware flash` says without `--i-understand`.
pub const FLASH_RISK: &str = "refusing to flash without --i-understand: flashing firmware is a \
     risk; a failed or interrupted flash leaves the meter in its bootloader (no measurements) \
     until the flash is retried. Keep the meter plugged in, then re-run with --i-understand to \
     accept the risk";

/// The parsed container of the firmware file `name`, or a usage error
/// naming the file and the reason. Runs before any device is chosen.
pub fn inspect_flash_file<'a>(
    bytes: &'a [u8],
    name: &str,
) -> Result<FirmwareContainer<'a>, CliError> {
    FirmwareContainer::parse(bytes).map_err(|e| CliError::Usage(format!("{name}: {e}")))
}

/// The meter a flash talks to, from the device list as it is now: the one
/// with the `--device` serial and never another one (`[device]` the serial
/// is the same in application and bootloader mode), `None` while it is not
/// attached; without a serial, the only POWER-Z attached, and two or more
/// are ambiguous, a usage error naming them (AC-STQEA1.6). The library's own
/// text only counts, since it reaches the desktop page.
pub fn find_flashed_meter<'a>(
    devices: &'a [DeviceEntry],
    serial: Option<&str>,
) -> Result<Option<&'a DeviceEntry>, CliError> {
    discovery::find_flashed_meter(devices, serial).map_err(|e| {
        CliError::Usage(format!(
            "several POWER-Z devices attached and no serial chosen; pick one with its serial or unplug the others: {}",
            e.serials.join(", ")
        ))
    })
}

/// `powerz firmware flash`: flash `container` onto the meter `opener`
/// finds, printing each stage on its own line (payload writes about every
/// tenth of the image) and the outcome. `stop` (set by Ctrl-C) stops the
/// flash at the engine's next safe point. A failure, a stop included, is a
/// device error carrying the engine's message, which says how to recover.
pub fn firmware_flash<O: MeterOpener, C: Clock>(
    ack: FlashAck,
    container: &FirmwareContainer<'_>,
    opener: &mut O,
    clock: C,
    stop: &AtomicBool,
    out: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(
        out,
        "Flashing {} firmware {} ({} bytes)",
        container.model(),
        container.firmware_version(),
        container.size()
    )?;
    let mut io: Result<(), std::io::Error> = Ok(());
    let mut last_tenth = None;
    let mut progress = |p: FlashProgress| {
        let line = match p {
            FlashProgress::Switching => "Switching the meter to its bootloader".to_owned(),
            FlashProgress::Erasing => "Erasing the application area".to_owned(),
            FlashProgress::Writing { done, total } => {
                let tenth = (done * 10).checked_div(total).unwrap_or(10);
                if last_tenth == Some(tenth) {
                    return;
                }
                last_tenth = Some(tenth);
                format!("Writing {done} / {total} bytes")
            }
            FlashProgress::Verifying => "Verifying".to_owned(),
            FlashProgress::Finished => "The bootloader accepted the image".to_owned(),
            FlashProgress::Returning => "Returning the meter to application mode".to_owned(),
            FlashProgress::Replug { to, reply } => {
                let frame = match to {
                    DeviceMode::Bootloader => JumpTarget::Bootloader.frame_name(),
                    _ => JumpTarget::Application.frame_name(),
                };
                let wait = match to {
                    DeviceMode::Bootloader => SWITCH_TIMEOUT,
                    _ => RETURN_TIMEOUT,
                };
                format!("{frame} reply: {reply}\n{}", replug_prompt(to, wait))
            }
        };
        if io.is_ok() {
            io = writeln!(out, "{line}").and_then(|()| out.flush());
        }
    };
    let outcome = flash(ack, container, opener, clock, stop, &mut progress)
        .map_err(|e| CliError::Device(format!("flash failed ({}): {e}", e.code())))?;
    io?;
    match outcome {
        FlashOutcome::Verified { version } => {
            writeln!(out, "Done: the meter now runs firmware {version}")?;
        }
        FlashOutcome::PowerCycleNeeded => writeln!(
            out,
            "The image was accepted, but the meter stayed in its bootloader: unplug it and \
             plug it back in"
        )?,
    }
    Ok(())
}

/// What `firmware jump` says without `--i-understand`.
pub const JUMP_RISK: &str = "refusing to jump without --i-understand: switching the meter's mode \
     is a risk; in its bootloader the meter takes no measurements until it is jumped back to \
     application mode or power-cycled. Re-run with --i-understand to accept the risk";

/// What to tell the user after a jump to `to`: a replug if the meter does
/// not restart by itself, naming the wait.
fn replug_prompt(to: DeviceMode, wait: Duration) -> String {
    format!(
        "Unplug the meter and plug it back in (no buttons) if it does not restart by itself; \
         waiting up to {} s for {} mode",
        wait.as_secs(),
        to.name()
    )
}

/// `powerz firmware jump`: send the one jump frame for `target` to the
/// meter `opener` finds and print the mode it was in and the mode it came
/// back in. Once the jump is sent, before the wait, it prints the `Was in:`
/// line, the meter's reply and the replug prompt; a failed write still
/// prints the `Was in:` line before the error. A refusal
/// (already there, unknown mode, Ctrl-C before the jump) prints nothing. A
/// refusal, a failed write, or a meter that does not come back in `target`
/// mode is a device error; the last names the mode seen (and, for the
/// bootloader, how to start it by hand).
pub fn firmware_jump<O: MeterOpener, C: Clock>(
    ack: FlashAck,
    target: JumpTarget,
    opener: &mut O,
    clock: C,
    stop: &AtomicBool,
    out: &mut impl Write,
) -> Result<(), CliError> {
    let frame = target.frame_name();
    let to = target.mode().name();
    let seen_first = Rc::new(Cell::new(None));
    let sent = Rc::new(Cell::new(false));
    let mut watched = Watched {
        inner: opener,
        first: Rc::clone(&seen_first),
        sent: Rc::clone(&sent),
    };
    let wait = match target {
        JumpTarget::Bootloader => SWITCH_TIMEOUT,
        JumpTarget::Application => RETURN_TIMEOUT,
    };
    let mut io: Result<(), std::io::Error> = Ok(());
    let reported = Cell::new(false);
    let mut report = |s: &JumpSent| {
        reported.set(true);
        io = writeln!(out, "Was in:  {} mode (sent {frame})", s.from.label())
            .and_then(|()| writeln!(out, "Reply:   {}", s.reply))
            .and_then(|()| writeln!(out, "{}", replug_prompt(target.mode(), wait)))
            .and_then(|()| out.flush());
    };
    let result = jump(ack, target, &mut watched, clock, stop, &mut report);
    io?;
    if sent.get() && !reported.get() {
        // Only a meter that is not in `target` mode is sent the jump.
        let from = seen_first.get().unwrap_or(DeviceMode::Unknown);
        writeln!(out, "Was in:  {} mode (sent {frame})", from.label())?;
    }
    let outcome =
        result.map_err(|e| CliError::Device(format!("jump failed ({}): {e}", e.code())))?;
    match outcome {
        JumpOutcome::Reached(mode) => {
            writeln!(out, "Now in:  {} mode", mode.label())?;
            Ok(())
        }
        JumpOutcome::NotReached { seen } => {
            let seen = seen.map_or("no meter".to_owned(), |m| format!("{} mode", m.label()));
            let hint = match target {
                JumpTarget::Bootloader => format!("; {BACK_BUTTON_HINT}"),
                JumpTarget::Application => {
                    "; unplug the meter and plug it back in to restart it".to_owned()
                }
            };
            Err(CliError::Device(format!(
                "the meter did not come back in {to} mode after {frame}: {seen} seen{hint}"
            )))
        }
    }
}

/// [`MeterOpener`] wrapper of [`firmware_jump`]: remembers the mode of the
/// first meter opened and whether a raw write (the jump) was attempted.
struct Watched<'a, O> {
    inner: &'a mut O,
    first: Rc<Cell<Option<DeviceMode>>>,
    sent: Rc<Cell<bool>>,
}

impl<O: MeterOpener> MeterOpener for Watched<'_, O> {
    type Pipe = WatchedPipe<O::Pipe>;

    fn open(&mut self) -> Result<Option<Meter<Self::Pipe>>, TransportError> {
        let meter = self.inner.open()?;
        if let Some(m) = &meter
            && self.first.get().is_none()
        {
            self.first.set(Some(m.mode));
        }
        Ok(meter.map(|m| Meter {
            mode: m.mode,
            model: m.model,
            pipe: WatchedPipe {
                inner: m.pipe,
                sent: Rc::clone(&self.sent),
            },
        }))
    }
}

/// A pipe of [`Watched`].
struct WatchedPipe<P> {
    inner: P,
    sent: Rc<Cell<bool>>,
}

impl<P: Transport> Transport for WatchedPipe<P> {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.inner.transact(request, timeout)
    }

    fn last_timeout_direction(&self) -> Option<TimeoutDirection> {
        self.inner.last_timeout_direction()
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.inner.read(timeout)
    }

    fn recover(&mut self) -> Result<(), TransportError> {
        self.inner.recover()
    }
}

impl<P: FlashTransport> FlashTransport for WatchedPipe<P> {
    fn write_raw(&mut self, data: &[u8], timeout: Duration) -> Result<(), TransportError> {
        self.sent.set(true);
        self.inner.write_raw(data, timeout)
    }
}
