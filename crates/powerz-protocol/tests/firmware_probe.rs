//! The read-only firmware probe (FR V9SK56, AC .6): the descriptor-level
//! USB identity with its text formatter, and `Session::probe`, which sends
//! only CONNECT and HEAD. Over a scripted mock transport; every value is
//! synthetic.

mod common;

use common::*;
use powerz_protocol::decode::head;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::frame;
use powerz_protocol::session::{ConnectOutcome, Error, HeadOutcome, ProbeReport, Session};
use powerz_protocol::transport;
use powerz_protocol::transport::discovery::{UsbIdentity, UsbInterface};
use powerz_protocol::transport::guard::FORBIDDEN_TYPES;
use powerz_protocol::transport::mock::{MockTransport, Step};

fn session(script: Vec<Step>) -> Session<MockTransport, FakeClock> {
    Session::with_clock(
        MockTransport::new(script),
        FakeClock::new(1_790_000_000_000),
    )
}

fn types(s: &Session<MockTransport, FakeClock>) -> Vec<u8> {
    s.transport()
        .written()
        .iter()
        .map(|f| f[0] & 0x7F)
        .collect()
}

fn head_reply(id: u8, field: u32) -> Vec<u8> {
    let mut f = vec![0x40, id, 0x00, 0x01];
    f.extend(field.to_le_bytes());
    f.extend([0u8; 12]);
    f
}

fn iface(number: u8, class: u8, string: Option<&str>) -> UsbInterface {
    UsbInterface {
        number,
        class,
        subclass: 0,
        protocol: 0,
        string: string.map(str::to_owned),
    }
}

/// The application-mode identity of a KM003C `[device]`.
fn app_identity() -> UsbIdentity {
    UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: Some("POWER-Z KM003C".to_owned()),
        interfaces: vec![
            iface(0, 0xFF, Some("APP")),
            iface(1, 0x02, Some("APP")),
            iface(2, 0x0A, None),
            iface(3, 0x03, None),
        ],
    }
}

/// The line of `text` describing interface `n` (`interface <n>`, any case).
fn interface_line(text: &str, n: u8) -> String {
    let want = format!("interface {n}");
    let lines: Vec<String> = text
        .lines()
        .map(str::to_ascii_lowercase)
        .filter(|l| l.contains(&want))
        .collect();
    assert_eq!(lines.len(), 1, "one line for interface {n} in:\n{text}");
    lines[0].clone()
}

// ---------------------------------------------------------------- identity

#[test]
fn identity_text_shows_vid_and_pid_as_0x_hex_and_the_product_string() {
    let text = app_identity().to_string();
    let lower = text.to_ascii_lowercase();
    assert!(lower.contains("0x5fc9"), "VID in:\n{text}");
    assert!(lower.contains("0x0063"), "PID in:\n{text}");
    assert!(text.contains("POWER-Z KM003C"), "product in:\n{text}");
}

#[test]
fn identity_text_has_one_line_per_interface_with_number_class_and_string() {
    let text = app_identity().to_string();
    let if0 = interface_line(&text, 0);
    assert!(if0.contains("0xff"), "class of IF0: {if0}");
    assert!(if0.contains("app"), "string of IF0: {if0}");
    let if1 = interface_line(&text, 1);
    assert!(if1.contains("0x02"), "class of IF1: {if1}");
    assert!(if1.contains("app"), "string of IF1: {if1}");
    let if2 = interface_line(&text, 2);
    assert!(if2.contains("0x0a"), "class of IF2: {if2}");
    assert!(!if2.contains("app"), "IF2 has no string: {if2}");
    let if3 = interface_line(&text, 3);
    assert!(if3.contains("0x03"), "class of IF3: {if3}");
}

#[test]
fn identity_text_of_a_bootloader_shows_its_mode_string() {
    let id = UsbIdentity {
        vid: 0x5FC9,
        pid: 0x0063,
        product: None,
        interfaces: vec![iface(0, 0xFF, Some("BOOT"))],
    };
    let text = id.to_string();
    let if0 = interface_line(&text, 0);
    assert!(if0.contains("0xff") && if0.contains("boot"), "{if0}");
    assert!(
        text.to_ascii_lowercase().contains("0x5fc9"),
        "VID even without a product string:\n{text}"
    );
}

#[test]
fn identity_mode_comes_from_the_vendor_interface_string() {
    assert_eq!(app_identity().mode(), DeviceMode::Application);
    let boot = UsbIdentity {
        interfaces: vec![iface(0, 0xFF, Some("BOOT"))],
        ..app_identity()
    };
    assert_eq!(boot.mode(), DeviceMode::Bootloader);
    // The vendor interface is found by class, never by number.
    let reordered = UsbIdentity {
        interfaces: vec![iface(0, 0x02, Some("BOOT")), iface(1, 0xFF, Some("APP"))],
        ..app_identity()
    };
    assert_eq!(reordered.mode(), DeviceMode::Application);
}

// ---------------------------------------------------------------- probe

#[test]
fn probe_sends_connect_then_head_and_reports_the_head_field() {
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(head::request(2).to_vec(), head_reply(2, 0)),
    ]);
    let report = s.probe().expect("probe");
    assert_eq!(
        report,
        ProbeReport {
            connect: ConnectOutcome::Accepted,
            head: HeadOutcome::Field(0),
        }
    );
    assert_eq!(types(&s), vec![0x02, 0x40], "only CONNECT and HEAD");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn probe_reports_a_non_zero_head_field() {
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(head::request(2).to_vec(), head_reply(2, 0x0102_0304)),
    ]);
    let report = s.probe().expect("probe");
    assert_eq!(report.connect, ConnectOutcome::Accepted);
    assert_eq!(report.head, HeadOutcome::Field(0x0102_0304));
}

#[test]
fn probe_takes_an_id_0_head_reply_as_the_answer() {
    // The real meter answers HEAD with id 0 (AC-V9SK52.3); the probe
    // reports its field without re-reading or judging it.
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(head::request(2).to_vec(), head_reply(0, 0x0000_1000)),
    ]);
    let report = s.probe().expect("an id-0 HEAD reply is data");
    assert_eq!(report.connect, ConnectOutcome::Accepted);
    assert_eq!(report.head, HeadOutcome::Field(0x0000_1000));
    assert_eq!(types(&s), vec![0x02, 0x40], "one HEAD");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn probe_does_not_take_an_id_0_reply_of_another_type_as_head() {
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(
            head::request(2).to_vec(),
            vec![0x41, 0x00, 0x00, 0x01, 0x00, 0x10, 0x00, 0x00],
        ),
        Step::read(head_reply(2, 0)),
    ]);
    let report = s.probe().expect("probe");
    assert_eq!(report.head, HeadOutcome::Field(0), "the re-read reply");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn probe_keeps_the_raw_head_reply_when_it_does_not_decode() {
    // Header only: too short for the field (a bootloader may answer so).
    let raw = vec![0x40, 2, 0x00, 0x01];
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(head::request(2).to_vec(), raw.clone()),
    ]);
    let report = s
        .probe()
        .expect("an undecodable HEAD is data, not an error");
    assert_eq!(report.connect, ConnectOutcome::Accepted);
    assert_eq!(report.head, HeadOutcome::Raw(raw));
    assert_eq!(types(&s), vec![0x02, 0x40]);
}

#[test]
fn probe_keeps_a_reply_of_another_type_as_raw() {
    // PUT_DATA instead of HEAD.
    let raw = vec![0x41, 2, 0x00, 0x00, 1, 2, 3, 4];
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(head::request(2).to_vec(), raw.clone()),
    ]);
    let report = s.probe().expect("probe");
    assert_eq!(report.head, HeadOutcome::Raw(raw));
}

#[test]
fn probe_reports_a_rejected_head() {
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(head::request(2).to_vec(), reject(2)),
    ]);
    let report = s.probe().expect("a REJECT is data");
    assert_eq!(report.connect, ConnectOutcome::Accepted);
    // The REJECT is kept as sent, so a hardware run records the bytes.
    assert_eq!(report.head, HeadOutcome::Rejected(reject(2)));
}

#[test]
fn probe_keeps_the_raw_bytes_of_a_rejected_head_as_sent() {
    // A REJECT with trailing bytes: everything the meter sent is kept.
    let raw = vec![0x06, 2, 0x00, 0x00, 0xAA, 0x55];
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(head::request(2).to_vec(), raw.clone()),
    ]);
    let report = s.probe().expect("a REJECT is data");
    assert_eq!(report.head, HeadOutcome::Rejected(raw));
    assert_eq!(types(&s), vec![0x02, 0x40]);
}

#[test]
fn probe_reports_a_head_timeout() {
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact_err(head::request(2).to_vec(), transport::Error::Timeout),
    ]);
    let report = s.probe().expect("a timeout is data");
    assert_eq!(report.connect, ConnectOutcome::Accepted);
    assert_eq!(report.head, HeadOutcome::TimedOut);
}

#[test]
fn probe_still_sends_head_after_a_rejected_connect() {
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), reject(1)),
        Step::transact(head::request(2).to_vec(), head_reply(2, 0)),
    ]);
    let report = s.probe().expect("a rejected CONNECT is data");
    assert_eq!(report.connect, ConnectOutcome::Rejected);
    assert_eq!(report.head, HeadOutcome::Field(0));
    assert_eq!(types(&s), vec![0x02, 0x40]);
}

#[test]
fn probe_reports_a_connect_timeout_and_sends_nothing_but_connect_and_head() {
    // Every transfer times out; however many CONNECT attempts the session
    // makes, only CONNECT and HEAD frames may go on the wire.
    struct Silent(Vec<Vec<u8>>);
    impl transport::Transport for Silent {
        fn transact(
            &mut self,
            request: &[u8],
            _t: std::time::Duration,
        ) -> Result<Vec<u8>, transport::Error> {
            self.0.push(request.to_vec());
            Err(transport::Error::Timeout)
        }
        fn read(&mut self, _t: std::time::Duration) -> Result<Vec<u8>, transport::Error> {
            Err(transport::Error::Timeout)
        }
    }
    let mut s = Session::with_clock(Silent(Vec::new()), FakeClock::new(1_790_000_000_000));
    let report = s.probe().expect("timeouts are data");
    assert_eq!(report.connect, ConnectOutcome::TimedOut);
    assert_eq!(report.head, HeadOutcome::TimedOut);
    let sent: Vec<u8> = s.transport().0.iter().map(|f| f[0] & 0x7F).collect();
    assert!(
        sent.iter().all(|t| *t == 0x02 || *t == 0x40),
        "only CONNECT and HEAD: {sent:02x?}"
    );
    assert_eq!(sent.iter().filter(|t| **t == 0x40).count(), 1, "one HEAD");
    assert!(sent.contains(&0x02), "CONNECT was sent: {sent:02x?}");
}

#[test]
fn probe_never_sends_a_forbidden_type_memory_read_get_data_or_auth() {
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(head::request(2).to_vec(), head_reply(2, 0)),
    ]);
    s.probe().expect("probe");
    for ty in types(&s) {
        assert!(!FORBIDDEN_TYPES.contains(&ty), "forbidden {ty:#04x}");
        assert!(
            ![0x0C, 0x44, 0x4C].contains(&ty),
            "not read-only: {ty:#04x}"
        );
    }
}

#[test]
fn probe_returns_an_error_when_the_meter_is_unplugged() {
    let mut s = session(vec![Step::transact_err(
        frame::connect(1).to_vec(),
        transport::Error::Disconnected,
    )]);
    let err = s.probe().expect_err("an unplugged meter cannot be probed");
    assert!(err.is_link_fatal(), "{err:?}");
}

// ---------------------------------------------------------------- probe errors
//
// Only a REJECT, a timeout or an undecodable reply is data. Any other
// transport error during CONNECT or HEAD is returned as an error: there is
// no "failed" outcome.

/// A transport whose every transfer fails with `error`; records requests.
struct Failing {
    error: transport::Error,
    sent: Vec<Vec<u8>>,
}

impl transport::Transport for Failing {
    fn transact(
        &mut self,
        request: &[u8],
        _t: std::time::Duration,
    ) -> Result<Vec<u8>, transport::Error> {
        self.sent.push(request.to_vec());
        Err(self.error.clone())
    }
    fn read(&mut self, _t: std::time::Duration) -> Result<Vec<u8>, transport::Error> {
        Err(self.error.clone())
    }
}

fn probe_over_failing(error: transport::Error) -> (Result<ProbeReport, Error>, Vec<u8>) {
    let mut s = Session::with_clock(
        Failing {
            error,
            sent: Vec::new(),
        },
        FakeClock::new(1_790_000_000_000),
    );
    let result = s.probe();
    let sent = s.transport().sent.iter().map(|f| f[0] & 0x7F).collect();
    (result, sent)
}

#[test]
fn probe_returns_an_error_for_an_io_failure_during_connect() {
    let (result, sent) = probe_over_failing(transport::Error::Io("pipe open failed".into()));
    match result {
        Err(Error::Transport(transport::Error::Io(m))) => {
            assert!(m.contains("pipe open failed"), "{m}")
        }
        other => panic!("an I/O failure is an error, not data: {other:?}"),
    }
    assert!(
        !sent.contains(&0x40),
        "no HEAD after a failed CONNECT: {sent:02x?}"
    );
}

#[test]
fn probe_returns_an_error_when_the_device_is_not_found() {
    let (result, sent) = probe_over_failing(transport::Error::NotFound);
    assert_eq!(
        result,
        Err(Error::Transport(transport::Error::NotFound)),
        "NotFound is an error, not data"
    );
    assert!(
        !sent.contains(&0x40),
        "no HEAD after a failed CONNECT: {sent:02x?}"
    );
}

#[test]
fn probe_returns_an_error_when_permission_is_denied_on_connect() {
    let (result, _) = probe_over_failing(transport::Error::Permission("access denied".into()));
    assert!(
        matches!(
            result,
            Err(Error::Transport(transport::Error::Permission(_)))
        ),
        "{result:?}"
    );
}

#[test]
fn probe_returns_an_error_for_an_io_failure_during_head() {
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact_err(
            head::request(2).to_vec(),
            transport::Error::Io("bulk read failed".into()),
        ),
    ]);
    match s.probe() {
        Err(Error::Transport(transport::Error::Io(m))) => {
            assert!(m.contains("bulk read failed"), "{m}")
        }
        other => panic!("an I/O failure during HEAD is an error, not data: {other:?}"),
    }
}

#[test]
fn probe_returns_an_error_when_the_meter_is_unplugged_during_head() {
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact_err(head::request(2).to_vec(), transport::Error::Disconnected),
    ]);
    assert_eq!(
        s.probe(),
        Err(Error::Transport(transport::Error::Disconnected))
    );
}
