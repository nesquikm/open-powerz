//! The shared firmware-status degrade helper (FR V9SK52, AC .2 / .3):
//! `Session::read_firmware_status_or_unknown`, `FirmwareStatus::unknown`
//! and `session::Error::is_link_fatal`, over a scripted mock transport.
//! Every byte vector is synthetic.

mod common;

use common::*;
use powerz_protocol::decode::DecodeError;
use powerz_protocol::decode::head;
use powerz_protocol::frame;
use powerz_protocol::session::{
    self, FirmwareProblem, FirmwareProblemKind, FirmwareStatus, Session,
};
use powerz_protocol::transport;
use powerz_protocol::transport::mock::{MockTransport, Step};

const BOOT_ADDR: u32 = 0x0000_0420;

fn session(script: Vec<Step>) -> Session<MockTransport, FakeClock> {
    Session::with_clock(
        MockTransport::new(script),
        FakeClock::new(1_790_000_000_000),
    )
}

fn connect_step(id: u8) -> Step {
    Step::transact(frame::connect(id).to_vec(), accept(id))
}

/// A session whose CONNECT (id 1) was accepted, with `rest` scripted after.
fn connected(rest: Vec<Step>) -> Session<MockTransport, FakeClock> {
    let mut s = session([vec![connect_step(1)], rest].concat());
    s.connect().expect("scripted CONNECT");
    s
}

fn types(s: &Session<MockTransport, FakeClock>) -> Vec<u8> {
    s.transport()
        .written()
        .iter()
        .map(|f| f[0] & 0x7F)
        .collect()
}

// ---------------------------------------------------------------- FirmwareStatus::unknown

#[test]
fn unknown_firmware_status_has_no_blocks_an_unknown_update_flag_and_no_warning() {
    let st = FirmwareStatus::unknown();
    assert_eq!(st.bootloader, None);
    assert_eq!(st.application, None);
    assert_eq!(st.update_required, None, "unknown, not false");
    assert_eq!(st.problems, Vec::<FirmwareProblem>::new());
    assert_eq!(st.warnings(), Vec::new());
    assert_eq!(
        st,
        FirmwareStatus {
            bootloader: None,
            application: None,
            update_required: None,
            problems: Vec::new(),
        }
    );
}

// ---------------------------------------------------------------- Error::is_link_fatal

#[test]
fn unplug_and_permission_errors_are_link_fatal() {
    assert!(session::Error::Transport(transport::Error::Disconnected).is_link_fatal());
    assert!(
        session::Error::Transport(transport::Error::Permission("denied".into())).is_link_fatal()
    );
}

#[test]
fn timeouts_and_bad_replies_are_not_link_fatal() {
    for e in [
        session::Error::Transport(transport::Error::Timeout),
        session::Error::Protocol("late reply".into()),
        session::Error::Rejected { request_type: 0x40 },
        session::Error::Decode(DecodeError::Length {
            what: "HEAD reply",
            expected: 8,
            got: 4,
        }),
    ] {
        assert!(!e.is_link_fatal(), "{e:?}");
    }
}

// ---------------------------------------------------------------- degrade + reconnect
//
// Order after CONNECT (id 1): bootloader block (id 2), application block
// (id 3), then HEAD (id 4). A HEAD or block that fails is followed by one
// recovery CONNECT and the remaining steps still run.

const APP_ADDR: u32 = 0x0000_4420;

/// Synthetic info block with `model`, `version` and `date`.
fn block(model: &str, version: &str, date: &str) -> Vec<u8> {
    let mut b = vec![0u8; 64];
    b[0x10..0x10 + model.len()].copy_from_slice(model.as_bytes());
    b[0x1C..0x1C + version.len()].copy_from_slice(version.as_bytes());
    b[0x28..0x28 + date.len()].copy_from_slice(date.as_bytes());
    b
}

fn boot_block() -> Vec<u8> {
    block("KM003C", "2.5", "2024.01.01")
}

fn app_block() -> Vec<u8> {
    block("KM003C", "2.0.6", "2025.06.01")
}

fn good_head(id: u8) -> Step {
    Step::transact(
        head::request(id).to_vec(),
        vec![0x40, id, 0x00, 0x01, 0, 0, 0, 0],
    )
}

/// Both blocks read fine (ids 2, 3).
fn both_blocks() -> Vec<Step> {
    [
        memread_steps(2, BOOT_ADDR, &boot_block()),
        memread_steps(3, APP_ADDR, &app_block()),
    ]
    .concat()
}

fn has_problem(st: &FirmwareStatus, words: &[&str]) -> bool {
    st.problems
        .iter()
        .any(|p| words.iter().any(|w| p.text.to_lowercase().contains(w)))
}

/// The kinds of `st.problems`, in order.
fn kinds(st: &FirmwareStatus) -> Vec<FirmwareProblemKind> {
    st.problems.iter().map(|p| p.kind).collect()
}

/// The one problem of `kind`.
fn problem_of(st: &FirmwareStatus, kind: FirmwareProblemKind) -> &FirmwareProblem {
    st.problems
        .iter()
        .find(|p| p.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} problem in {:?}", st.problems))
}

#[test]
fn an_undecodable_head_keeps_the_blocks_and_reconnects_once() {
    // HEAD (id 4) answered with a bare header: too short to decode.
    let mut s = connected(
        [
            both_blocks(),
            vec![
                Step::transact(head::request(4).to_vec(), vec![0x40, 4, 0x00, 0x01]),
                connect_step(5),
                Step::transact(get_data(6, 0x001), with_id(vector(ADC_52), 6)),
            ],
        ]
        .concat(),
    );
    let st = s
        .read_firmware_status_or_unknown()
        .expect("an undecodable HEAD only costs the update check");
    assert_eq!(st.bootloader.as_ref().map(|b| b.version()), Some("2.5"));
    assert_eq!(st.application.as_ref().map(|b| b.version()), Some("2.0.6"));
    assert_eq!(st.update_required, None);
    assert!(has_problem(&st, &["head"]), "{:?}", st.problems);
    assert_eq!(
        kinds(&st),
        vec![FirmwareProblemKind::Head],
        "{:?}",
        st.problems
    );
    assert_eq!(
        types(&s),
        vec![0x02, 0x44, 0x44, 0x40, 0x02],
        "one recovery CONNECT right after the failed HEAD"
    );
    // The pipe is usable again.
    s.read_adc().expect("a request after the recovery works");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn a_head_timeout_keeps_the_blocks_and_reconnects_once() {
    let mut s = connected(
        [
            both_blocks(),
            vec![
                Step::transact_err(head::request(4).to_vec(), transport::Error::Timeout),
                connect_step(5),
                Step::transact(get_data(6, 0x001), with_id(vector(ADC_52), 6)),
            ],
        ]
        .concat(),
    );
    let st = s
        .read_firmware_status_or_unknown()
        .expect("a HEAD timeout only costs the update check");
    assert!(
        st.bootloader.is_some() && st.application.is_some(),
        "{st:?}"
    );
    assert_eq!(st.update_required, None);
    assert!(has_problem(&st, &["head"]), "{:?}", st.problems);
    let p = problem_of(&st, FirmwareProblemKind::Head);
    assert!(p.text.contains("timed out"), "{p:?}");
    assert_eq!(p.to_string(), p.text, "Display is the text");
    assert_eq!(types(&s), vec![0x02, 0x44, 0x44, 0x40, 0x02]);
    s.read_adc().expect("a request after the recovery works");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn link_fatal_errors_on_head_are_returned_and_send_no_connect() {
    for err in [
        transport::Error::Disconnected,
        transport::Error::Permission("denied".into()),
    ] {
        let mut s = connected(
            [
                both_blocks(),
                vec![Step::transact_err(head::request(4).to_vec(), err.clone())],
            ]
            .concat(),
        );
        let r = s.read_firmware_status_or_unknown();
        assert_eq!(r, Err(session::Error::Transport(err.clone())), "{err:?}");
        assert_eq!(
            types(&s),
            vec![0x02, 0x44, 0x44, 0x40],
            "{err:?}: no CONNECT after a link-fatal error"
        );
    }
}

#[test]
fn link_fatal_errors_on_an_info_block_are_returned_and_send_no_connect() {
    for err in [
        transport::Error::Disconnected,
        transport::Error::Permission("denied".into()),
    ] {
        let mut s = connected(vec![Step::transact_err(
            memread_request(2, BOOT_ADDR, 64),
            err.clone(),
        )]);
        let r = s.read_firmware_status_or_unknown();
        assert_eq!(r, Err(session::Error::Transport(err.clone())), "{err:?}");
        assert_eq!(types(&s), vec![0x02, 0x44], "{err:?}: no CONNECT, no HEAD");
    }
}

#[test]
fn a_bootloader_block_timeout_is_a_visible_problem_and_the_rest_still_runs() {
    // 0x420 (id 2) times out; recovery CONNECT (id 3); 0x4420 (id 4) and
    // HEAD (id 5) still answer.
    let mut s = connected(
        [
            vec![
                Step::transact_err(memread_request(2, BOOT_ADDR, 64), transport::Error::Timeout),
                connect_step(3),
            ],
            memread_steps(4, APP_ADDR, &app_block()),
            vec![
                good_head(5),
                Step::transact(get_data(6, 0x001), with_id(vector(ADC_52), 6)),
            ],
        ]
        .concat(),
    );
    let st = s
        .read_firmware_status_or_unknown()
        .expect("an info-block timeout only costs that block");
    assert_eq!(st.bootloader, None);
    assert_eq!(st.application.as_ref().map(|b| b.version()), Some("2.0.6"));
    assert_eq!(st.update_required, Some(false), "HEAD still answered");
    assert!(
        has_problem(&st, &["bootloader", "0x420"]),
        "a problem names the bootloader block: {:?}",
        st.problems
    );
    assert!(
        !has_problem(&st, &["application", "0x4420"]),
        "{:?}",
        st.problems
    );
    assert_eq!(
        kinds(&st),
        vec![FirmwareProblemKind::BootloaderBlock],
        "{:?}",
        st.problems
    );
    assert!(
        problem_of(&st, FirmwareProblemKind::BootloaderBlock)
            .text
            .contains("timed out"),
        "{:?}",
        st.problems
    );
    assert_eq!(types(&s), vec![0x02, 0x44, 0x02, 0x44, 0x40]);
    s.read_adc().expect("a request after the recovery works");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn an_application_block_timeout_is_a_visible_problem_and_head_still_runs() {
    // 0x420 (id 2) fine; 0x4420 (id 3) times out; CONNECT (id 4); HEAD (id 5).
    let mut s = connected(
        [
            memread_steps(2, BOOT_ADDR, &boot_block()),
            vec![
                Step::transact_err(memread_request(3, APP_ADDR, 64), transport::Error::Timeout),
                connect_step(4),
                good_head(5),
            ],
        ]
        .concat(),
    );
    let st = s
        .read_firmware_status_or_unknown()
        .expect("an info-block timeout only costs that block");
    assert_eq!(st.bootloader.as_ref().map(|b| b.version()), Some("2.5"));
    assert_eq!(st.application, None, "never a silent null: see problems");
    assert!(
        has_problem(&st, &["application", "0x4420"]),
        "a problem names the application block: {:?}",
        st.problems
    );
    assert_eq!(
        kinds(&st),
        vec![FirmwareProblemKind::ApplicationBlock],
        "{:?}",
        st.problems
    );
    assert_eq!(types(&s), vec![0x02, 0x44, 0x44, 0x02, 0x40]);
    assert_eq!(s.transport().remaining(), 0);
}

// ---------------------------------------------------------------- a failed recovery CONNECT
//
// The recovery CONNECT after a failed step may itself be REJECTed or time
// out. That is recorded as a recovery problem, the remaining steps are
// skipped (no further block read, no HEAD) and what was read is returned.
// A timed-out CONNECT is retried once by `Session::connect` after the
// transport recovery, so it shows up as two CONNECT frames.

/// Both CONNECT attempts (`id`, `id + 1`) time out.
fn connect_times_out(id: u8) -> Vec<Step> {
    vec![
        Step::transact_err(frame::connect(id).to_vec(), transport::Error::Timeout),
        Step::transact_err(frame::connect(id + 1).to_vec(), transport::Error::Timeout),
    ]
}

fn connect_rejected(id: u8) -> Step {
    Step::transact(frame::connect(id).to_vec(), reject(id))
}

#[test]
fn a_rejected_recovery_connect_after_a_block_is_a_problem_and_skips_the_rest() {
    // 0x420 (id 2) times out; the recovery CONNECT (id 3) is rejected.
    let mut s = connected(vec![
        Step::transact_err(memread_request(2, BOOT_ADDR, 64), transport::Error::Timeout),
        connect_rejected(3),
    ]);
    let st = s
        .read_firmware_status_or_unknown()
        .expect("a failed recovery CONNECT is a problem, not an error");
    assert_eq!(st.bootloader, None);
    assert_eq!(st.application, None);
    assert_eq!(st.update_required, None);
    assert_eq!(
        kinds(&st),
        vec![
            FirmwareProblemKind::BootloaderBlock,
            FirmwareProblemKind::Recovery
        ],
        "{:?}",
        st.problems
    );
    let p = problem_of(&st, FirmwareProblemKind::Recovery);
    let text = p.text.to_lowercase();
    assert!(
        text.contains("recovery connect") && text.contains("failed"),
        "says the recovery CONNECT failed: {p:?}"
    );
    assert!(text.contains("reject"), "and why: {p:?}");
    assert_eq!(
        types(&s),
        vec![0x02, 0x44, 0x02],
        "no application block and no HEAD after a failed recovery"
    );
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn a_timed_out_recovery_connect_after_a_block_is_a_problem_and_skips_the_rest() {
    // 0x420 (id 2) fine; 0x4420 (id 3) times out; the recovery CONNECT
    // (ids 4 and 5) times out.
    let mut s = connected(
        [
            memread_steps(2, BOOT_ADDR, &boot_block()),
            vec![Step::transact_err(
                memread_request(3, APP_ADDR, 64),
                transport::Error::Timeout,
            )],
            connect_times_out(4),
        ]
        .concat(),
    );
    let st = s
        .read_firmware_status_or_unknown()
        .expect("a timed-out recovery CONNECT is a problem, not an error");
    assert_eq!(
        st.bootloader.as_ref().map(|b| b.version()),
        Some("2.5"),
        "what was read is kept"
    );
    assert_eq!(st.application, None);
    assert_eq!(st.update_required, None, "HEAD was never sent");
    assert_eq!(
        kinds(&st),
        vec![
            FirmwareProblemKind::ApplicationBlock,
            FirmwareProblemKind::Recovery
        ],
        "{:?}",
        st.problems
    );
    let p = problem_of(&st, FirmwareProblemKind::Recovery);
    assert!(p.text.to_lowercase().contains("recovery connect"), "{p:?}");
    assert!(p.text.contains("timed out"), "{p:?}");
    assert_eq!(types(&s), vec![0x02, 0x44, 0x44, 0x02, 0x02], "no HEAD");
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn a_failed_recovery_connect_after_head_keeps_the_blocks() {
    for (case, recovery) in [
        ("rejected", vec![connect_rejected(5)]),
        ("timed out", connect_times_out(5)),
    ] {
        let mut s = connected(
            [
                both_blocks(),
                vec![Step::transact(
                    head::request(4).to_vec(),
                    vec![0x06, 4, 0x00, 0x00],
                )],
                recovery,
            ]
            .concat(),
        );
        let st = s
            .read_firmware_status_or_unknown()
            .unwrap_or_else(|e| panic!("{case}: a failed recovery is a problem: {e:?}"));
        assert_eq!(
            st.bootloader.as_ref().map(|b| b.version()),
            Some("2.5"),
            "{case}"
        );
        assert_eq!(
            st.application.as_ref().map(|b| b.version()),
            Some("2.0.6"),
            "{case}"
        );
        assert_eq!(st.update_required, None, "{case}");
        assert_eq!(
            kinds(&st),
            vec![FirmwareProblemKind::Head, FirmwareProblemKind::Recovery],
            "{case}: {:?}",
            st.problems
        );
        assert_eq!(s.transport().remaining(), 0, "{case}");
    }
}

#[test]
fn a_link_fatal_recovery_connect_is_still_returned() {
    for err in [
        transport::Error::Disconnected,
        transport::Error::Permission("denied".into()),
    ] {
        let mut s = connected(vec![
            Step::transact_err(memread_request(2, BOOT_ADDR, 64), transport::Error::Timeout),
            Step::transact_err(frame::connect(3).to_vec(), err.clone()),
        ]);
        let r = s.read_firmware_status_or_unknown();
        assert_eq!(r, Err(session::Error::Transport(err.clone())), "{err:?}");
        assert_eq!(types(&s), vec![0x02, 0x44, 0x02], "{err:?}");
    }
}
