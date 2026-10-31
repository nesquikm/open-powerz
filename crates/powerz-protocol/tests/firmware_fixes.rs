//! Firmware fixes (FR 4PJRVJ): the version-string policy (AC .5) and the
//! probe's stop on any SYNC reply (AC .8). Over a scripted mock transport;
//! every value is synthetic.

mod common;

use common::*;
use powerz_protocol::decode::head;
use powerz_protocol::firmware::version::{FirmwareWarning, Version, warnings};
use powerz_protocol::frame;
use powerz_protocol::session::{Error, Session};
use powerz_protocol::transport::mock::{MockTransport, Step};

// ---------------------------------------------------------------- AC-4PJRVJ.5

#[test]
fn a_leading_v_is_dropped() {
    for s in ["v2.0.7", "V2.0.7", " v2.0.7 "] {
        assert_eq!(Version::parse(s), Some(Version::new(2, 0, 7)), "{s:?}");
    }
    assert_eq!(Version::parse("v2.5"), Some(Version::new(2, 5, 0)));
}

#[test]
fn a_build_suffix_is_ignored() {
    assert_eq!(Version::parse("2.0.7+build5"), Some(Version::new(2, 0, 7)));
}

#[test]
fn a_pre_release_sorts_below_its_release() {
    for s in ["2.0.7-beta", "2.0.7-rc.1", "v2.0.7-beta", "2.0.7-beta+5"] {
        let v = Version::parse(s).expect(s);
        assert!(v < Version::new(2, 0, 7), "{s:?}");
        assert!(v > Version::new(2, 0, 6), "{s:?}");
    }
}

#[test]
fn what_the_policy_does_not_cover_stays_unparsed() {
    for s in ["", "v", "beta", "-beta", "2.x.7", "2.0.7.1", "vv2.0.7"] {
        assert_eq!(Version::parse(s), None, "{s:?}");
    }
}

#[test]
fn tolerated_version_strings_get_the_same_warnings_as_plain_ones() {
    assert_eq!(warnings("v1.6.0", false), vec![FirmwareWarning::TooOld]);
    assert_eq!(
        warnings("2.0.6-beta", false),
        vec![FirmwareWarning::Outdated]
    );
    assert_eq!(
        warnings("v2.0.7-beta", false),
        vec![FirmwareWarning::Outdated]
    );
    assert_eq!(warnings("v2.0.7+build5", false), Vec::new());
}

#[test]
fn the_policy_is_documented_on_parse() {
    let source = include_str!("../src/firmware/version.rs");
    let doc: String = source
        .split("pub fn parse")
        .next()
        .expect("a parse function")
        .lines()
        .rev()
        // The last line is the indent before `pub fn parse` itself.
        .skip(1)
        .take_while(|l| l.trim_start().starts_with("///"))
        .collect();
    for example in ["v2.0.7", "2.0.7-beta"] {
        assert!(doc.contains(example), "parse's docs name {example}: {doc}");
    }
}

// ---------------------------------------------------------------- AC-4PJRVJ.8

/// A SYNC reply with an id no request carried.
const OTHER_ID_SYNC: [u8; 4] = [0x01, 0x77, 0x00, 0x00];

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

#[test]
fn the_probe_stops_on_a_sync_of_another_id_to_connect() {
    let mut s = session(vec![Step::transact(
        frame::connect(1).to_vec(),
        OTHER_ID_SYNC.to_vec(),
    )]);
    let r = s.probe();
    assert!(matches!(r, Err(Error::BootloaderMidDownload)), "{r:?}");
    assert_eq!(types(&s), vec![0x02], "no retried CONNECT and no HEAD");
}

#[test]
fn the_probe_stops_on_a_sync_of_another_id_to_head() {
    let mut s = session(vec![
        Step::transact(frame::connect(1).to_vec(), accept(1)),
        Step::transact(head::request(2).to_vec(), OTHER_ID_SYNC.to_vec()),
    ]);
    let r = s.probe();
    assert!(matches!(r, Err(Error::BootloaderMidDownload)), "{r:?}");
    assert_eq!(types(&s), vec![0x02, 0x40], "nothing after the HEAD");
}

#[test]
fn the_probe_stops_on_a_sync_with_id_0() {
    let mut s = session(vec![Step::transact(
        frame::connect(1).to_vec(),
        vec![0x01, 0x00, 0x00, 0x00],
    )]);
    let r = s.probe();
    assert!(matches!(r, Err(Error::BootloaderMidDownload)), "{r:?}");
    assert_eq!(types(&s), vec![0x02]);
}
