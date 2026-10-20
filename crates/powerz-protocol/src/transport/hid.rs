//! Restricted read-only HID fallback on `hidapi`: ADC snapshot only.

use std::time::Duration;

use super::{Error, GuardedTransport, Transport};
use crate::frame::{Attribute, DeclaredEnd, HEADER_LEN, MainHeader, declared_end, msg};

/// Vendor usage page of the meter's HID interface.
pub const USAGE_PAGE: u16 = 0xFF00;
/// Size of one HID input / output report (report ID excluded).
pub const REPORT_LEN: usize = 64;

/// The HID device operations the transport needs (hidapi-shaped).
pub trait HidIo {
    /// Write one output report (report ID byte first).
    fn write(&mut self, data: &[u8]) -> Result<usize, Error>;
    /// Read one input report; `Ok(0)` means nothing arrived in time.
    fn read_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, Error>;
}

/// One HID interface as enumerated by hidapi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidCandidate {
    /// hidapi device path.
    pub path: String,
    /// USB vendor ID.
    pub vid: u16,
    /// USB product ID.
    pub pid: u16,
    /// HID usage page.
    pub usage_page: u16,
    /// USB serial string.
    pub serial: Option<String>,
}

/// Pick the meter's usage-page-0xFF00 interface (VID 0x5FC9), optionally
/// matching `serial`.
pub fn pick_interface<'a>(
    candidates: &'a [HidCandidate],
    serial: Option<&str>,
) -> Option<&'a HidCandidate> {
    candidates.iter().find(|c| {
        c.vid == crate::VID
            && c.usage_page == USAGE_PAGE
            && serial.is_none_or(|s| c.serial.as_deref() == Some(s))
    })
}

/// Allow only GET_DATA with mask exactly ADC (0x001).
pub fn check_hid(request: &[u8]) -> Result<(), Error> {
    let header = match MainHeader::decode(request) {
        Ok(h) if request.len() == HEADER_LEN => h.control(),
        _ => return Err(Error::UnsupportedOnHid),
    };
    if header.ty == msg::GET_DATA
        && !header.extend
        && !header.enc
        && header.att == Attribute::ADC.bits()
    {
        Ok(())
    } else {
        Err(Error::UnsupportedOnHid)
    }
}

/// Report ID 0 followed by `frame` zero-padded to 64 bytes.
pub fn output_report(frame: &[u8]) -> Result<[u8; REPORT_LEN + 1], Error> {
    if frame.len() > REPORT_LEN {
        return Err(Error::UnsupportedOnHid);
    }
    let mut report = [0u8; REPORT_LEN + 1];
    report[1..=frame.len()].copy_from_slice(frame);
    Ok(report)
}

/// Total bytes a reply's headers declare: the main header plus every section
/// header and payload in the chain. Non-PUT_DATA replies declare only what
/// arrived.
fn declared_len(reply: &[u8]) -> usize {
    match MainHeader::decode(reply) {
        Ok(h) if h.control().ty == msg::PUT_DATA => {}
        _ => return reply.len(),
    }
    match declared_end(reply) {
        DeclaredEnd::Complete(end) | DeclaredEnd::PdStatus { size_end: end } => end,
        // The next section header lies past the report.
        DeclaredEnd::MissingHeader(offset) => offset + HEADER_LEN,
    }
}

/// HID transport over one hidapi device.
#[derive(Debug)]
pub struct HidTransport<D> {
    device: D,
}

impl<D: HidIo> HidTransport<D> {
    /// Wrap an open HID device.
    pub fn with_device(device: D) -> GuardedTransport<HidTransport<D>> {
        GuardedTransport::new(HidTransport { device })
    }
}

impl HidTransport<hidapi::HidDevice> {
    /// Open the meter's 0xFF00 HID interface, optionally by serial.
    pub fn open(serial: Option<&str>) -> Result<GuardedTransport<Self>, Error> {
        let api = hidapi::HidApi::new().map_err(hid_error)?;
        let candidates: Vec<HidCandidate> = api
            .device_list()
            .map(|d| HidCandidate {
                path: d.path().to_string_lossy().into_owned(),
                vid: d.vendor_id(),
                pid: d.product_id(),
                usage_page: d.usage_page(),
                serial: d.serial_number().map(str::to_owned),
            })
            .collect();
        let picked = pick_interface(&candidates, serial).ok_or(Error::NotFound)?;
        let path =
            std::ffi::CString::new(picked.path.clone()).map_err(|e| Error::Io(e.to_string()))?;
        let device = api.open_path(&path).map_err(hid_error)?;
        Ok(HidTransport::with_device(device))
    }
}

impl HidIo for hidapi::HidDevice {
    fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
        hidapi::HidDevice::write(self, data).map_err(hid_error)
    }

    fn read_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, Error> {
        let ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        hidapi::HidDevice::read_timeout(self, buf, ms).map_err(hid_error)
    }
}

fn hid_error(e: hidapi::HidError) -> Error {
    let os = std::env::consts::OS;
    match e {
        hidapi::HidError::IoError { error } => super::map_io_error(&error, os),
        other => hid_message_error(&other.to_string(), os),
    }
}

/// Map a hidapi error message to a transport error. hidapi reports most
/// failures as text, so unplug and access failures are recognised by wording.
pub(crate) fn hid_message_error(message: &str, target_os: &str) -> Error {
    let m = message.to_lowercase();
    if m.contains("permission denied") || m.contains("access denied") {
        super::permission_error(target_os, message)
    } else if m.contains("disconnected") || m.contains("no such device") {
        Error::Disconnected
    } else {
        Error::Io(message.to_string())
    }
}

impl<D: HidIo> Transport for HidTransport<D> {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        check_hid(request)?;
        let report = output_report(request)?;
        self.device.write(&report)?;
        let mut buf = [0u8; REPORT_LEN];
        let n = self.device.read_timeout(&mut buf, timeout)?;
        if n == 0 {
            return Err(Error::Timeout);
        }
        let reply = &buf[..n];
        let declared = declared_len(reply);
        if declared > REPORT_LEN {
            return Err(Error::HidReplyTooLong { declared });
        }
        Ok(reply.to_vec())
    }

    /// Follow-up reads are never safe over HID: one request, one report.
    fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, Error> {
        Err(Error::UnsupportedOnHid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::test_util::hex_vector;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    const ADC_52: &str = include_str!("../../tests/vectors/adc_52.hex");
    const T: Duration = Duration::from_millis(100);

    #[derive(Default)]
    struct State {
        writes: Vec<Vec<u8>>,
        reports: VecDeque<Vec<u8>>,
        reads: usize,
    }

    /// Fake HID device: records writes, returns scripted input reports, then
    /// times out (`Ok(0)`).
    #[derive(Clone, Default)]
    struct FakeHid(Rc<RefCell<State>>);

    impl FakeHid {
        fn with_reports(reports: Vec<Vec<u8>>) -> FakeHid {
            let d = FakeHid::default();
            d.0.borrow_mut().reports = reports.into();
            d
        }
        fn writes(&self) -> Vec<Vec<u8>> {
            self.0.borrow().writes.clone()
        }
        fn reads(&self) -> usize {
            self.0.borrow().reads
        }
    }

    impl HidIo for FakeHid {
        fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
            self.0.borrow_mut().writes.push(data.to_vec());
            Ok(data.len())
        }
        fn read_timeout(&mut self, buf: &mut [u8], _timeout: Duration) -> Result<usize, Error> {
            let mut s = self.0.borrow_mut();
            s.reads += 1;
            match s.reports.pop_front() {
                Some(r) => {
                    let n = r.len().min(buf.len());
                    buf[..n].copy_from_slice(&r[..n]);
                    Ok(n)
                }
                None => Ok(0),
            }
        }
    }

    fn padded(bytes: &[u8]) -> Vec<u8> {
        let mut r = bytes.to_vec();
        r.resize(REPORT_LEN, 0);
        r
    }

    /// A 64-byte report whose headers declare 236 bytes: ADC section with
    /// `next = 1`, then a 180-byte SETTINGS section header at offset 52.
    fn over_long_report() -> Vec<u8> {
        let mut r = vec![0x41, 0x01, 0xC0, 0x0E]; // PUT_DATA
        r.extend_from_slice(&[0x01, 0x80, 0x00, 0x0B]); // ADC, next, size 44
        r.extend_from_slice(&[0x11; 44]);
        r.extend_from_slice(&[0x08, 0x00, 0x00, 0x2D]); // SETTINGS, size 180
        r.resize(REPORT_LEN, 0x22);
        r
    }

    // ---- AC-SJGK7E.6: interface pick ----

    #[test]
    fn pick_interface_takes_vendor_usage_page() {
        let c = vec![
            HidCandidate {
                path: "kbd".into(),
                vid: 0x5FC9,
                pid: 0x0063,
                usage_page: 0x0001,
                serial: Some("SERIAL-0001".into()),
            },
            HidCandidate {
                path: "meter-b".into(),
                vid: 0x5FC9,
                pid: 0x0063,
                usage_page: 0xFF00,
                serial: Some("SERIAL-0002".into()),
            },
            HidCandidate {
                path: "meter-a".into(),
                vid: 0x5FC9,
                pid: 0x0063,
                usage_page: 0xFF00,
                serial: Some("SERIAL-0001".into()),
            },
            HidCandidate {
                path: "foreign".into(),
                vid: 0x1234,
                pid: 0x0001,
                usage_page: 0xFF00,
                serial: None,
            },
        ];
        assert_eq!(
            pick_interface(&c, Some("SERIAL-0001")).map(|h| h.path.as_str()),
            Some("meter-a")
        );
        assert_eq!(
            pick_interface(&c, None).map(|h| h.path.as_str()),
            Some("meter-b")
        );
        assert_eq!(pick_interface(&c[..1], None), None);
    }

    // ---- AC-SJGK7E.6: output report layout ----

    #[test]
    fn output_report_is_report_id_zero_plus_padded_frame() {
        let r = output_report(&[0x0C, 0x05, 0x02, 0x00]).unwrap();
        assert_eq!(r.len(), 65);
        assert_eq!(&r[..5], &[0x00, 0x0C, 0x05, 0x02, 0x00]);
        assert!(r[5..].iter().all(|&b| b == 0));
    }

    // ---- AC-SJGK7E.6: ADC-only allowlist ----

    #[test]
    fn check_hid_allows_only_adc_get_data() {
        assert_eq!(check_hid(&[0x0C, 0x07, 0x02, 0x00]), Ok(()));
        for req in [
            [0x0C, 0x01, 0x20, 0x00], // PD_PACKET 0x010
            [0x0C, 0x01, 0x80, 0x00], // QC_PACKET 0x040
            [0x0C, 0x01, 0x22, 0x00], // ADC | PD_PACKET
            [0x0C, 0x01, 0x12, 0x00], // ADC | SETTINGS
            [0x0C, 0x01, 0x00, 0x00], // empty mask
            [0x02, 0x01, 0x00, 0x00], // CONNECT
            [0x0E, 0x01, 0x02, 0x00], // START_QUEUE
            [0x44, 0x01, 0x00, 0x00], // memory read
        ] {
            assert_eq!(check_hid(&req), Err(Error::UnsupportedOnHid), "{req:02x?}");
        }
    }

    #[test]
    fn transact_adc_writes_one_padded_report_and_returns_one_input_report() {
        let adc = hex_vector(ADC_52);
        let dev = FakeHid::with_reports(vec![padded(&adc)]);
        let mut t = HidTransport::with_device(dev.clone());
        let reply = t.transact(&[0x0C, 0x05, 0x02, 0x00], T).unwrap();
        assert_eq!(reply.len(), REPORT_LEN);
        assert_eq!(&reply[..52], &adc[..]);
        let writes = dev.writes();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].len(), 65);
        assert_eq!(&writes[0][..5], &[0x00, 0x0C, 0x05, 0x02, 0x00]);
        assert!(writes[0][5..].iter().all(|&b| b == 0));
        assert_eq!(dev.reads(), 1);
    }

    #[test]
    fn transact_refuses_non_adc_without_writing() {
        let dev = FakeHid::default();
        let mut t = HidTransport::with_device(dev.clone());
        for req in [
            [0x0C, 0x01, 0x20, 0x00],
            [0x0C, 0x01, 0x80, 0x00],
            [0x02, 0x01, 0x00, 0x00],
        ] {
            assert_eq!(
                t.transact(&req, T),
                Err(Error::UnsupportedOnHid),
                "{req:02x?}"
            );
        }
        assert!(dev.writes().is_empty());
    }

    // ---- AC-SJGK7E.6: over-length reply refusal ----

    #[test]
    fn transact_refuses_over_long_reply_and_reads_nothing_more() {
        let dev = FakeHid::with_reports(vec![over_long_report(), padded(&[0x33; 8])]);
        let mut t = HidTransport::with_device(dev.clone());
        match t.transact(&[0x0C, 0x01, 0x02, 0x00], T) {
            Err(Error::HidReplyTooLong { declared }) => assert!(declared > 64, "{declared}"),
            other => panic!("expected HidReplyTooLong, got {other:?}"),
        }
        assert_eq!(dev.reads(), 1, "must not read any further report");
    }

    #[test]
    fn transact_with_no_report_is_timeout() {
        let dev = FakeHid::default();
        let mut t = HidTransport::with_device(dev);
        assert_eq!(
            t.transact(&[0x0C, 0x01, 0x02, 0x00], T),
            Err(Error::Timeout)
        );
    }

    // ---- AC-SJGK7E.8: the guard runs on HID too ----

    #[test]
    fn transact_runs_the_guard_without_writing() {
        let dev = FakeHid::default();
        let mut t = HidTransport::with_device(dev.clone());
        assert_eq!(
            t.transact(&[0x09, 0x01, 0x00, 0x00], T),
            Err(Error::Forbidden { ty: 0x09 })
        );
        assert!(dev.writes().is_empty());
    }

    #[test]
    fn hid_message_error_maps_permission_and_unplug() {
        assert!(matches!(
            hid_message_error("hid_open_path: Permission denied", "linux"),
            Error::Permission(m) if m.contains("60-powerz.rules")
        ));
        assert!(matches!(
            hid_message_error(
                "IOHIDDeviceOpen failed: (0xE00002C5) access denied",
                "macos"
            ),
            Error::Permission(_)
        ));
        assert_eq!(
            hid_message_error("hid_read_timeout: device disconnected", "macos"),
            Error::Disconnected
        );
        assert_eq!(
            hid_message_error("hid_write: No such device", "linux"),
            Error::Disconnected
        );
        assert!(matches!(
            hid_message_error("something else", "linux"),
            Error::Io(_)
        ));
    }
}
