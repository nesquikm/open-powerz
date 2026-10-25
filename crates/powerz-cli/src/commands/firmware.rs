//! `powerz firmware info FILE` (FR V9SK53) and `powerz firmware probe`
//! (FR V9SK56, AC .6).

use std::io::Write;

use powerz_protocol::firmware::container::FirmwareContainer;
use powerz_protocol::session::{Clock, ConnectOutcome, HeadOutcome, Session};
use powerz_protocol::transport::Transport;
use powerz_protocol::transport::discovery::UsbIdentity;

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
/// out`, then `HEAD field: <n>`, `HEAD reply (raw): <hex bytes>`, `HEAD:
/// rejected` or `HEAD: timed out`. The identity is written (and flushed)
/// before anything is sent. Any other failure of CONNECT or HEAD (the
/// pipe would not open, I/O, unplugged) is returned as an error and no
/// CONNECT / HEAD line or JSON report is written. JSON keys `vid`, `pid`, `product`, `mode`,
/// `interfaces` (`number`, `class`, `subclass`, `protocol`, `string`),
/// `connect`, `head` (`field`, `raw`, `rejected` or `timed out`),
/// `head_field` and `head_raw` (hex, no spaces), `null` when absent.
pub fn firmware_probe<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    identity: &UsbIdentity,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    if !json {
        write!(out, "{identity}")?;
        out.flush()?;
    }
    let report = session.probe()?;
    let connect = match &report.connect {
        ConnectOutcome::Accepted => "accepted".to_owned(),
        ConnectOutcome::Rejected => "rejected".to_owned(),
        ConnectOutcome::TimedOut => "timed out".to_owned(),
    };
    let (head, field, raw) = match &report.head {
        HeadOutcome::Field(f) => ("field".to_owned(), Some(*f), None),
        HeadOutcome::Raw(r) => ("raw".to_owned(), None, Some(r.as_slice())),
        HeadOutcome::Rejected => ("rejected".to_owned(), None, None),
        HeadOutcome::TimedOut => ("timed out".to_owned(), None, None),
    };
    if json {
        let raw = raw.map(|r| hex(r, ""));
        return write_json(
            out,
            &probe_json(identity, Some(&connect), Some(&head), field, raw),
        );
    }
    writeln!(out, "CONNECT: {connect}")?;
    match (field, raw) {
        (Some(f), _) => writeln!(out, "HEAD field: {f}")?,
        (None, Some(r)) => writeln!(out, "HEAD reply (raw): {}", hex(r, " "))?,
        (None, None) => writeln!(out, "HEAD: {head}")?,
    }
    Ok(())
}

/// `powerz firmware probe` on a meter without a vendor-class (0xFF)
/// interface: print `identity` (text or JSON with `connect` and `head`
/// `null`) and say on `err` that the probe cannot run; nothing is sent.
pub fn firmware_probe_unavailable(
    identity: &UsbIdentity,
    json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    if json {
        write_json(out, &probe_json(identity, None, None, None, None))?;
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
    connect: Option<&str>,
    head: Option<&str>,
    head_field: Option<u32>,
    head_raw: Option<String>,
) -> serde_json::Value {
    let interfaces: Vec<serde_json::Value> = identity
        .interfaces
        .iter()
        .map(|i| {
            serde_json::json!({
                "number": i.number,
                "class": i.class,
                "subclass": i.subclass,
                "protocol": i.protocol,
                "string": i.string,
            })
        })
        .collect();
    serde_json::json!({
        "vid": identity.vid,
        "pid": identity.pid,
        "product": identity.product,
        "mode": identity.mode().name(),
        "interfaces": interfaces,
        "connect": connect,
        "head": head,
        "head_field": head_field,
        "head_raw": head_raw,
    })
}

/// `bytes` as lowercase hex pairs joined by `sep`.
fn hex(bytes: &[u8], sep: &str) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(sep)
}
