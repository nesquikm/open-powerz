//! Firmware status from real meter bytes (FR V9SK52, AC .2 / .3, bug found
//! on the real KM003C): the info blocks are read before HEAD, and a HEAD
//! that fails never costs them.
//!
//! CONNECT, its ACCEPT and both info-block reads (0x420, 0x4420) are
//! replayed from a session-1 capture (`vectors/firmware_status_session1.hex`,
//! firmware 2.0.6, bootloader 2.5). Neither block carries a serial or UID.
//! Every HEAD reply here is synthetic.

mod common;

use common::*;
use powerz_protocol::decode::head;
use powerz_protocol::firmware::version::FirmwareWarning;
use powerz_protocol::frame;
use powerz_protocol::session::{FirmwareProblem, FirmwareProblemKind, FirmwareStatus, Session};
use powerz_protocol::transport;
use powerz_protocol::transport::mock::{MockTransport, Step};

const SESSION1: &str = include_str!("vectors/firmware_status_session1.hex");

/// Captured frames, in order: CONNECT, ACCEPT, then for 0x420 and 0x4420
/// each request, confirmation and data.
fn captured() -> Vec<Vec<u8>> {
    let f = frames(SESSION1);
    assert_eq!(f.len(), 8, "fixture frame count");
    f
}

/// The captured CONNECT and both info-block reads (ids 1, 2, 3), then
/// `head` (HEAD id 4) and `rest`.
fn script(head: Step, rest: Vec<Step>) -> Vec<Step> {
    let f = captured();
    [
        vec![
            Step::transact(f[0].clone(), f[1].clone()),
            Step::transact(f[2].clone(), f[3].clone()),
            Step::read(f[4].clone()),
            Step::transact(f[5].clone(), f[6].clone()),
            Step::read(f[7].clone()),
            head,
        ],
        rest,
    ]
    .concat()
}

/// The recovery CONNECT (id 5) after a failed HEAD.
fn recovery_connect() -> Step {
    Step::transact(frame::connect(5).to_vec(), accept(5))
}

fn session(script: Vec<Step>) -> Session<MockTransport, FakeClock> {
    Session::with_clock(
        MockTransport::new(script),
        FakeClock::new(1_790_000_000_000),
    )
}

/// Connect, then read the firmware status the way `info` and the desktop
/// app do.
fn status_after(
    head: Step,
    rest: Vec<Step>,
) -> (FirmwareStatus, Session<MockTransport, FakeClock>) {
    let mut s = session(script(head, rest));
    s.connect().expect("captured CONNECT");
    let st = s
        .read_firmware_status_or_unknown()
        .expect("a failed HEAD only costs the update check");
    (st, s)
}

/// Lowercase text with every space removed, so a hex dump matches however
/// it is spaced.
fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// Both blocks carry the real model, version and date.
fn assert_real_blocks(st: &FirmwareStatus, case: &str) {
    let boot = st
        .bootloader
        .as_ref()
        .unwrap_or_else(|| panic!("{case}: bootloader block missing: {st:?}"));
    assert_eq!(boot.model(), "KM003C", "{case}");
    assert_eq!(boot.version(), "2.5", "{case}");
    assert_eq!(boot.date(), "2026.7.5", "{case}");
    let app = st
        .application
        .as_ref()
        .unwrap_or_else(|| panic!("{case}: application block missing: {st:?}"));
    assert_eq!(app.model(), "KM003C", "{case}");
    assert_eq!(app.version(), "2.0.6", "{case}");
    assert_eq!(app.date(), "2026.8.9", "{case}");
    assert_eq!(
        st.warnings(),
        vec![FirmwareWarning::Outdated],
        "{case}: 2.0.6 is older than the newest known"
    );
}

/// The text of the one HEAD problem.
fn head_problem<'a>(st: &'a FirmwareStatus, case: &str) -> &'a str {
    let p = st
        .problems
        .iter()
        .find(|p| p.kind == FirmwareProblemKind::Head)
        .unwrap_or_else(|| panic!("{case}: no HEAD problem in {:?}", st.problems));
    assert!(p.text.contains("HEAD"), "{case}: {p:?}");
    &p.text
}

/// The kinds of `st.problems`, in order.
fn kinds(st: &FirmwareStatus) -> Vec<FirmwareProblemKind> {
    st.problems.iter().map(|p| p.kind).collect()
}

/// A HEAD failure seen in the field.
struct BadHead {
    case: &'static str,
    head: Step,
    /// The reason phrase the problem text must carry, verbatim.
    reason: &'static str,
    /// Raw reply hex the problem text must carry, when there was a reply.
    raw: Option<&'static str>,
}

fn bad_heads() -> Vec<BadHead> {
    let req = head::request(4).to_vec();
    vec![
        BadHead {
            case: "reject",
            head: Step::transact(req.clone(), vec![0x06, 0x04, 0x00, 0x00]),
            reason: "the meter sent REJECT",
            raw: Some("06040000"),
        },
        BadHead {
            case: "other type",
            head: Step::transact(
                req.clone(),
                vec![0x41, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
            ),
            reason: "expected frame type 0x40, got 0x41",
            raw: Some("4104000100000000"),
        },
        BadHead {
            case: "timeout",
            head: Step::transact_err(req, transport::Error::Timeout),
            reason: "timed out",
            raw: None,
        },
    ]
}

// ---- the request bytes match the capture ----

#[test]
fn info_block_requests_are_the_captured_bytes_and_come_before_head() {
    let (_, s) = status_after(
        Step::transact(
            head::request(4).to_vec(),
            vec![0x40, 0x04, 0x00, 0x01, 0, 0, 0, 0],
        ),
        vec![],
    );
    let f = captured();
    let sent = s.transport().written();
    assert_eq!(sent.len(), 4, "CONNECT, 0x420, 0x4420, HEAD: {sent:02x?}");
    assert_eq!(sent[0], f[0], "CONNECT");
    assert_eq!(sent[1], f[2], "memory read 0x420 is the captured request");
    assert_eq!(sent[2], f[5], "memory read 0x4420 is the captured request");
    assert_eq!(sent[3], head::request(4).to_vec(), "HEAD comes last");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn a_good_head_gives_the_real_blocks_no_update_and_no_problem() {
    let (st, s) = status_after(
        Step::transact(
            head::request(4).to_vec(),
            vec![0x40, 0x04, 0x00, 0x01, 0, 0, 0, 0],
        ),
        vec![],
    );
    assert_real_blocks(&st, "good HEAD");
    assert_eq!(st.update_required, Some(false));
    assert_eq!(st.problems, Vec::<FirmwareProblem>::new());
    let connects = s
        .transport()
        .written()
        .iter()
        .filter(|f| f[0] == 0x02)
        .count();
    assert_eq!(connects, 1, "no recovery CONNECT when nothing failed");
}

// ---- a failed HEAD keeps both blocks ----

#[test]
fn a_failed_head_keeps_both_real_info_blocks_and_the_outdated_warning() {
    for BadHead { case, head, .. } in bad_heads() {
        let (st, _) = status_after(head, vec![recovery_connect()]);
        assert_real_blocks(&st, case);
    }
}

#[test]
fn a_failed_head_leaves_update_required_unknown_not_false() {
    for BadHead { case, head, .. } in bad_heads() {
        let (st, _) = status_after(head, vec![recovery_connect()]);
        assert_eq!(st.update_required, None, "{case}: {st:?}");
        assert!(
            !st.warnings().contains(&FirmwareWarning::UpdateRequired),
            "{case}"
        );
    }
}

#[test]
fn a_failed_head_is_a_visible_problem_with_the_reason_and_the_raw_reply() {
    for BadHead {
        case,
        head,
        reason,
        raw,
    } in bad_heads()
    {
        let (st, _) = status_after(head, vec![recovery_connect()]);
        let p = head_problem(&st, case);
        assert!(
            p.contains(reason),
            "{case}: the reason ({reason}) is in {p:?}"
        );
        if let Some(raw) = raw {
            assert!(
                squash(p).contains(raw),
                "{case}: the raw reply {raw} is in {p:?}"
            );
        }
    }
}

#[test]
fn a_failed_head_is_followed_by_one_recovery_connect() {
    for BadHead { case, head, .. } in bad_heads() {
        let (_, s) = status_after(head, vec![recovery_connect()]);
        let types: Vec<u8> = s
            .transport()
            .written()
            .iter()
            .map(|f| f[0] & 0x7F)
            .collect();
        assert_eq!(
            types,
            vec![0x02, 0x44, 0x44, 0x40, 0x02],
            "{case}: blocks, HEAD, then one recovery CONNECT"
        );
        assert_eq!(s.transport().remaining(), 0, "{case}");
    }
}

#[test]
fn a_long_wrong_type_head_reply_shows_at_most_32_bytes_of_hex_then_an_ellipsis() {
    let mut reply = vec![0x41, 0x04, 0x00, 0x01];
    reply.extend((0..196u32).map(|i| (i % 251) as u8));
    assert_eq!(reply.len(), 200);
    let (st, _) = status_after(
        Step::transact(head::request(4).to_vec(), reply.clone()),
        vec![recovery_connect()],
    );
    let p = head_problem(&st, "200-byte reply");
    let shown = squash(p);
    assert!(
        shown.contains(&head::hex(&reply[..32], "")),
        "the first 32 bytes are shown: {p:?}"
    );
    assert!(
        !shown.contains(&head::hex(&reply[..33], "")),
        "nothing past byte 32: {p:?}"
    );
    assert!(
        p.contains('\u{2026}') || p.contains("..."),
        "an ellipsis marks the cut: {p:?}"
    );
    assert!(
        p.len() < 200,
        "the text stays short ({} chars): {p:?}",
        p.len()
    );
}

// ---- several steps fail on the real bytes ----

#[test]
fn a_failed_application_block_then_a_good_head_names_only_that_block() {
    // 0x4420 (captured request, id 3) times out; recovery CONNECT (id 4);
    // HEAD (id 5) answers field 0.
    let f = captured();
    let mut s = session(vec![
        Step::transact(f[0].clone(), f[1].clone()),
        Step::transact(f[2].clone(), f[3].clone()),
        Step::read(f[4].clone()),
        Step::transact_err(f[5].clone(), transport::Error::Timeout),
        Step::transact(frame::connect(4).to_vec(), accept(4)),
        Step::transact(
            head::request(5).to_vec(),
            vec![0x40, 0x05, 0x00, 0x01, 0, 0, 0, 0],
        ),
    ]);
    s.connect().expect("captured CONNECT");
    let st = s
        .read_firmware_status_or_unknown()
        .expect("a failed block only costs that block");
    let boot = st.bootloader.as_ref().expect("the real bootloader block");
    assert_eq!(boot.version(), "2.5");
    assert_eq!(st.application, None);
    assert_eq!(st.update_required, Some(false), "HEAD field 0");
    assert_eq!(
        kinds(&st),
        vec![FirmwareProblemKind::ApplicationBlock],
        "{:?}",
        st.problems
    );
    let text = &st.problems[0].text;
    assert!(
        text.contains("application") && text.contains("0x4420"),
        "{text:?}"
    );
    assert!(text.contains("timed out"), "{text:?}");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn three_failures_in_a_row_each_recover_and_each_is_a_problem() {
    // 0x420 (captured request, id 2) is rejected; CONNECT (id 3); 0x4420
    // (id 4) times out; CONNECT (id 5); HEAD (id 6) has the wrong type;
    // CONNECT (id 7).
    let f = captured();
    let mut s = session(vec![
        Step::transact(f[0].clone(), f[1].clone()),
        Step::transact(f[2].clone(), reject(2)),
        Step::transact(frame::connect(3).to_vec(), accept(3)),
        Step::transact_err(
            memread_request(4, 0x0000_4420, 64),
            transport::Error::Timeout,
        ),
        Step::transact(frame::connect(5).to_vec(), accept(5)),
        Step::transact(
            head::request(6).to_vec(),
            vec![0x41, 0x06, 0x00, 0x01, 0, 0, 0, 0],
        ),
        Step::transact(frame::connect(7).to_vec(), accept(7)),
    ]);
    s.connect().expect("captured CONNECT");
    let st = s
        .read_firmware_status_or_unknown()
        .expect("every failure recovered");
    assert_eq!(st.bootloader, None);
    assert_eq!(st.application, None);
    assert_eq!(st.update_required, None);
    assert_eq!(
        kinds(&st),
        vec![
            FirmwareProblemKind::BootloaderBlock,
            FirmwareProblemKind::ApplicationBlock,
            FirmwareProblemKind::Head,
        ],
        "{:?}",
        st.problems
    );
    assert!(st.problems[0].text.contains("0x420"), "{:?}", st.problems);
    assert!(
        st.problems[1].text.contains("timed out"),
        "{:?}",
        st.problems
    );
    assert!(
        st.problems[2]
            .text
            .contains("expected frame type 0x40, got 0x41"),
        "{:?}",
        st.problems
    );
    let types: Vec<u8> = s
        .transport()
        .written()
        .iter()
        .map(|f| f[0] & 0x7F)
        .collect();
    assert_eq!(types, vec![0x02, 0x44, 0x02, 0x44, 0x02, 0x40, 0x02]);
    assert_eq!(s.transport().remaining(), 0);
}

// ---- AC-V9SK52.3 (changed 2026-10-26): HEAD in application mode ----
//
// The real meter answers HEAD (id 8 there, id 4 here) with type 0x40, id 0
// and field 0x00001000, then 12 stale bytes. That reply is the HEAD answer:
// it is taken from the first transfer, with no re-read, no timeout and no
// recovery CONNECT. Field 0 means current; any other field is shown as an
// unrecognised reply and never as "update required".

const HEAD_APP: &str = include_str!("vectors/firmware_head_app_mode.hex");

/// The real application-mode HEAD reply: 8 bytes as received, then 12
/// stale bytes equal to the tail of the last captured 0xC4 frame.
fn real_head_reply() -> Vec<u8> {
    let f = frames(HEAD_APP);
    assert_eq!(f.len(), 1, "one fixture line");
    let r = f[0].clone();
    assert_eq!(r.len(), 20, "{r:02x?}");
    assert_eq!(&r[..8], &[0x40, 0x00, 0x00, 0x01, 0x00, 0x10, 0x00, 0x00]);
    let c4 = &captured()[6];
    assert_eq!(c4[0], 0xC4, "the last 0xC4 frame of session 1");
    assert_eq!(&r[8..], &c4[c4.len() - 12..], "stale tail");
    r
}

/// The real reply with the field at offset 4 replaced by `field`.
fn id0_reply(field: u32) -> Vec<u8> {
    let mut r = real_head_reply();
    r[4..8].copy_from_slice(&field.to_le_bytes());
    r
}

/// The frame types sent so far.
fn sent_types(s: &Session<MockTransport, FakeClock>) -> Vec<u8> {
    s.transport()
        .written()
        .iter()
        .map(|f| f[0] & 0x7F)
        .collect()
}

#[test]
fn an_id_0_head_reply_with_field_0_is_the_answer_and_the_firmware_is_current() {
    let (st, s) = status_after(
        Step::transact(head::request(4).to_vec(), id0_reply(0)),
        vec![],
    );
    assert_real_blocks(&st, "id-0 HEAD, field 0");
    assert_eq!(st.update_required, Some(false), "{st:?}");
    assert_eq!(st.problems, Vec::<FirmwareProblem>::new());
    assert_eq!(
        sent_types(&s),
        vec![0x02, 0x44, 0x44, 0x40],
        "one HEAD, no re-read, no recovery CONNECT"
    );
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn the_real_application_mode_head_reply_is_unrecognised_not_update_required() {
    let real = real_head_reply();
    let (st, s) = status_after(
        Step::transact(head::request(4).to_vec(), real.clone()),
        vec![],
    );
    assert_real_blocks(&st, "real HEAD reply");
    assert_eq!(
        st.update_required, None,
        "unknown, never Some(true): {st:?}"
    );
    assert!(
        !st.warnings().contains(&FirmwareWarning::UpdateRequired),
        "{:?}",
        st.warnings()
    );
    assert_eq!(
        kinds(&st),
        vec![FirmwareProblemKind::Head],
        "{:?}",
        st.problems
    );
    let p = head_problem(&st, "real HEAD reply");
    assert!(
        p.to_lowercase().contains("recognised"),
        "says the reply was not recognised: {p:?}"
    );
    assert!(p.contains("0x00001000"), "names the field: {p:?}");
    assert!(
        squash(p).contains(&head::hex(&real, "")),
        "the raw reply: {p:?}"
    );
    assert!(!p.contains("timed out"), "no timeout: {p:?}");
    assert_eq!(
        sent_types(&s),
        vec![0x02, 0x44, 0x44, 0x40],
        "one HEAD, no re-read, no recovery CONNECT"
    );
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn a_long_unrecognised_id_0_head_reply_is_capped_at_32_bytes() {
    let mut reply = real_head_reply();
    reply.extend((0..180u32).map(|i| (i % 251) as u8));
    assert_eq!(reply.len(), 200);
    let (st, _) = status_after(
        Step::transact(head::request(4).to_vec(), reply.clone()),
        vec![],
    );
    assert_eq!(st.update_required, None);
    let p = head_problem(&st, "200-byte id-0 reply");
    assert!(p.contains("0x00001000"), "{p:?}");
    let shown = squash(p);
    assert!(shown.contains(&head::hex(&reply[..32], "")), "{p:?}");
    assert!(!shown.contains(&head::hex(&reply[..33], "")), "{p:?}");
    assert!(p.contains('\u{2026}') || p.contains("..."), "{p:?}");
}

#[test]
fn a_head_reply_with_neither_id_0_nor_the_request_id_is_still_re_read() {
    let (st, s) = status_after(
        Step::transact(
            head::request(4).to_vec(),
            vec![0x40, 0x07, 0x00, 0x01, 0x00, 0x10, 0x00, 0x00],
        ),
        vec![Step::read(vec![0x40, 0x04, 0x00, 0x01, 0, 0, 0, 0])],
    );
    assert_eq!(st.update_required, Some(false), "the re-read reply: {st:?}");
    assert_eq!(st.problems, Vec::<FirmwareProblem>::new());
    assert_eq!(sent_types(&s), vec![0x02, 0x44, 0x44, 0x40]);
    assert_eq!(
        s.transport().remaining(),
        0,
        "the stale id-7 reply was skipped"
    );
}

#[test]
fn a_reply_of_another_type_with_id_0_is_not_taken_as_head() {
    let (st, s) = status_after(
        Step::transact(
            head::request(4).to_vec(),
            vec![0x41, 0x00, 0x00, 0x01, 0x00, 0x10, 0x00, 0x00],
        ),
        vec![Step::read(vec![0x40, 0x04, 0x00, 0x01, 0, 0, 0, 0])],
    );
    assert_eq!(st.update_required, Some(false), "the re-read reply: {st:?}");
    assert_eq!(st.problems, Vec::<FirmwareProblem>::new());
    assert_eq!(sent_types(&s), vec![0x02, 0x44, 0x44, 0x40]);
    assert_eq!(s.transport().remaining(), 0, "the id-0 0x41 was skipped");
}
