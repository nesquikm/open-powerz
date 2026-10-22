//! PD_PACKET chunk and record parser (FR TGE83A, AC .2–.5): the 12-byte
//! status block, record splitting by `b0`, event decoding and the capture
//! clock. All chunks here are synthetic, built from the layout in
//! `docs/protocol.md` (`b0 | u32 ts ms | u8 sop/event | ...`).

use powerz_protocol::decode::DecodeError;
use powerz_protocol::decode::pd_packet::{
    CaptureClock, CcLine, MonitorType, PdChunk, PdEvent, PdRecord, PdRecordKind, PdStatus,
    ReceiveErrorCause,
};
use proptest::collection::vec;
use proptest::prelude::*;

// ---------------------------------------------------------------- builders

/// 12-byte status block.
fn status(ts: u32, vbus_mv: u16, ibus_ma: i16, cc1_mv: u16, cc2_mv: u16) -> Vec<u8> {
    let mut s = Vec::new();
    s.extend(ts.to_le_bytes());
    s.extend(vbus_mv.to_le_bytes());
    s.extend(ibus_ma.to_le_bytes());
    s.extend(cc1_mv.to_le_bytes());
    s.extend(cc2_mv.to_le_bytes());
    s
}

/// PD message record: `b0 = 0x80 | meter << 6 | (len - 1)`.
fn msg(meter: bool, ts: u32, sop: u8, header: u16, body: &[u8]) -> Vec<u8> {
    let len = 8 + body.len();
    assert!(len <= 64, "a bit-7 record holds at most 64 bytes");
    let b0 = 0x80 | if meter { 0x40 } else { 0 } | (len as u8 - 1);
    let mut r = vec![b0];
    r.extend(ts.to_le_bytes());
    r.push(sop);
    r.extend(header.to_le_bytes());
    r.extend(body);
    r
}

/// Six-byte event record: `b0 = 0x05 | meter << 6`, then the event byte.
fn event(meter: bool, ts: u32, code: u8) -> Vec<u8> {
    let b0 = 0x05 | if meter { 0x40 } else { 0 };
    let mut r = vec![b0];
    r.extend(ts.to_le_bytes());
    r.push(code);
    r
}

/// "Big" message record: `b0 = 0x29` (bit 5, base length 10), event byte 8,
/// u16 extra length at +6, PD header at +8, body from +10.
fn big(ts: u32, header: u16, body: &[u8]) -> Vec<u8> {
    let mut r = vec![0x29];
    r.extend(ts.to_le_bytes());
    r.push(0x08);
    r.extend((body.len() as u16).to_le_bytes());
    r.extend(header.to_le_bytes());
    r.extend(body);
    r
}

fn chunk(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

fn parse(bytes: &[u8]) -> PdChunk {
    PdChunk::parse(bytes).unwrap_or_else(|e| panic!("{bytes:02x?}: {e}"))
}

fn the_event(r: &PdRecord) -> PdEvent {
    match &r.kind {
        PdRecordKind::Event(e) => e.clone(),
        other => panic!("expected an event, got {other:?}"),
    }
}

// ---- AC-TGE83A.2: the 12-byte status block ----

#[test]
fn status_block_decodes_time_vbus_signed_ibus_cc1_cc2() {
    let c = parse(&status(0x02F7_0A25, 20_473, -1_234, 1_528, 3));
    assert_eq!(
        c.status,
        PdStatus {
            time_ms: 0x02F7_0A25,
            vbus_mv: 20_473,
            ibus_ma: -1_234,
            cc1_mv: 1_528,
            cc2_mv: 3,
        }
    );
    assert!(c.records.is_empty());
    assert_eq!(c.skipped, 0);
}

#[test]
fn status_block_ibus_keeps_its_sign() {
    // The vendor app shows abs(IBUS); we keep the sign.
    let c = parse(&status(1, 5_000, i16::MIN, 0, 0));
    assert_eq!(c.status.ibus_ma, i16::MIN);
    let c = parse(&status(1, 5_000, 3_000, 0, 0));
    assert_eq!(c.status.ibus_ma, 3_000);
}

#[test]
fn chunk_shorter_than_the_status_block_is_a_length_error() {
    for n in 0..12 {
        match PdChunk::parse(&vec![0u8; n]) {
            Err(DecodeError::Length { expected, got, .. }) => {
                assert_eq!((expected, got), (12, n));
            }
            other => panic!("{n} bytes: expected a length error, got {other:?}"),
        }
    }
}

// ---- AC-TGE83A.3: record splitting ----

#[test]
fn records_split_into_messages_big_messages_and_events() {
    let src_caps_body = [0x2C, 0x91, 0x81, 0x09, 0x2C, 0xD1, 0x02, 0x00];
    let big_body: Vec<u8> = (0..70u8).collect();
    let parts = [
        status(1000, 5_000, 0, 1_528, 0),
        event(true, 900, 0x11),
        msg(false, 901, 0, 0x21A1, &src_caps_body),
        msg(true, 902, 0, 0x0041, &[]),
        big(903, 0x9191, &big_body),
        msg(true, 904, 1, 0x108F, &[0x01, 0xA0, 0x00, 0xFF]),
    ];
    let c = parse(&chunk(&parts));
    assert_eq!(c.skipped, 0);
    assert_eq!(c.records.len(), 5, "{:?}", c.records);

    let times: Vec<u32> = c.records.iter().map(|r| r.time_ms).collect();
    assert_eq!(times, [900, 901, 902, 903, 904]);
    // Every record keeps its own bytes, b0 included.
    for (r, raw) in c.records.iter().zip(&parts[1..]) {
        assert_eq!(&r.raw, raw);
    }

    assert_eq!(the_event(&c.records[0]), PdEvent::Attach(CcLine::Cc1));

    match &c.records[1].kind {
        PdRecordKind::Message(m) => {
            assert_eq!(m.sop, 0);
            assert_eq!(m.header, 0x21A1);
            assert_eq!(m.body, src_caps_body);
        }
        other => panic!("expected a PD message, got {other:?}"),
    }
    assert!(!c.records[1].from_meter, "b0 0x8F: bit 6 clear");

    match &c.records[2].kind {
        PdRecordKind::Message(m) => {
            assert_eq!(m.header, 0x0041);
            assert!(m.body.is_empty());
        }
        other => panic!("expected a PD message, got {other:?}"),
    }
    assert!(c.records[2].from_meter, "b0 0xC7: bit 6 set");

    match &c.records[3].kind {
        PdRecordKind::BigMessage(m) => {
            assert_eq!(m.header, 0x9191);
            assert_eq!(m.body, big_body);
        }
        other => panic!("expected a big message, got {other:?}"),
    }
    assert_eq!(c.records[3].raw.len(), 10 + 70);

    match &c.records[4].kind {
        PdRecordKind::Message(m) => {
            assert_eq!(m.sop, 1, "SOP'");
            assert_eq!(m.header, 0x108F);
            assert_eq!(m.body, [0x01, 0xA0, 0x00, 0xFF]);
        }
        other => panic!("expected a PD message, got {other:?}"),
    }
}

#[test]
fn bit_7_wins_over_bit_5_for_the_record_length() {
    // 0xA3 has bits 7 and 5 set: a 36-byte PD message (the device sends its
    // EPR_Source_Capabilities chunks like this), not a big record.
    let body = [0u8; 28];
    let rec = msg(false, 77, 0, 0xF1B1, &body);
    assert_eq!(rec[0], 0xA3);
    let c = parse(&chunk(&[
        status(80, 0, 0, 0, 0),
        rec.clone(),
        event(true, 78, 0x04),
    ]));
    assert_eq!(c.records.len(), 2);
    assert!(matches!(c.records[0].kind, PdRecordKind::Message(_)));
    assert_eq!(c.records[0].raw, rec);
    assert_eq!(the_event(&c.records[1]), PdEvent::HardReset);
}

#[test]
fn record_running_past_the_chunk_is_skipped_and_counted() {
    let mut truncated = msg(false, 12, 0, 0x11A1, &[1, 2, 3, 4, 5, 6, 7, 8]);
    truncated.truncate(10);
    let c = parse(&chunk(&[
        status(20, 9_000, 100, 1_500, 0),
        msg(true, 11, 0, 0x0041, &[]),
        truncated,
    ]));
    assert_eq!(c.records.len(), 1, "{:?}", c.records);
    assert_eq!(c.records[0].time_ms, 11);
    assert_eq!(c.skipped, 1);
    assert_eq!(c.status.vbus_mv, 9_000);
}

#[test]
fn big_record_whose_extra_length_runs_past_the_chunk_is_skipped() {
    let mut rec = big(5, 0x9191, &[0xAA; 40]);
    rec.truncate(30);
    let c = parse(&chunk(&[status(6, 0, 0, 0, 0), event(false, 4, 0x03), rec]));
    assert_eq!(c.records.len(), 1);
    assert_eq!(the_event(&c.records[0]), PdEvent::BadMessageId);
    assert_eq!(c.skipped, 1);
}

/// Random bytes, biased toward plausible chunks: a status block followed by
/// random record bytes with b0 values of every kind.
fn chunks() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        vec(any::<u8>(), 0..400),
        (
            vec(any::<u8>(), 12),
            vec(
                (
                    prop::sample::select(vec![
                        0x00u8, 0x05, 0x45, 0x1F, 0x20, 0x29, 0x3F, 0x80, 0x87, 0xA3, 0xBF, 0xC7,
                        0xFF
                    ]),
                    vec(any::<u8>(), 0..70)
                ),
                0..8
            )
        )
            .prop_map(|(s, recs)| {
                let mut out = s;
                for (b0, rest) in recs {
                    out.push(b0);
                    out.extend(rest);
                }
                out
            }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn parser_never_panics_and_records_stay_inside_the_chunk(data in chunks()) {
        match PdChunk::parse(&data) {
            Ok(c) => {
                prop_assert!(data.len() >= 12);
                let used: usize = c.records.iter().map(|r| r.raw.len()).sum();
                prop_assert!(used <= data.len() - 12, "records use {} of {} bytes", used, data.len() - 12);
                for r in &c.records {
                    prop_assert!(!r.raw.is_empty());
                    prop_assert_eq!(r.from_meter, r.raw[0] & 0x40 != 0);
                    match &r.kind {
                        PdRecordKind::Message(m) => {
                            prop_assert!(r.raw[0] & 0x80 != 0);
                            prop_assert_eq!(r.raw.len(), usize::from(r.raw[0] & 0x3F) + 1);
                            prop_assert_eq!(m.body.len() + 8, r.raw.len());
                        }
                        PdRecordKind::BigMessage(m) => {
                            prop_assert!(r.raw[0] & 0x80 == 0 && r.raw[0] & 0x20 != 0);
                            prop_assert!(m.body.len() < r.raw.len());
                        }
                        PdRecordKind::Event(_) => {
                            prop_assert!(r.raw[0] & 0xA0 == 0);
                            prop_assert_eq!(r.raw.len(), usize::from(r.raw[0] & 0x1F) + 1);
                        }
                    }
                }
            }
            Err(_) => prop_assert!(data.len() < 12),
        }
    }
}

// ---- AC-TGE83A.4: event records ----

#[test]
fn attach_and_detach_carry_the_cc_line() {
    let cases = [
        (0x11, PdEvent::Attach(CcLine::Cc1)),
        (0x21, PdEvent::Attach(CcLine::Cc2)),
        (0x12, PdEvent::Detach(CcLine::Cc1)),
        (0x22, PdEvent::Detach(CcLine::Cc2)),
        (0x31, PdEvent::Attach(CcLine::Other(3))),
        (0xF2, PdEvent::Detach(CcLine::Other(15))),
    ];
    for (code, want) in cases {
        let c = parse(&chunk(&[status(0, 0, 0, 0, 0), event(true, 1, code)]));
        assert_eq!(the_event(&c.records[0]), want, "event byte {code:#04x}");
    }
}

#[test]
fn resets_carry_the_sent_by_the_meter_flag() {
    for (meter, code, want) in [
        (true, 0x04, PdEvent::HardReset),
        (false, 0x04, PdEvent::HardReset),
        (true, 0x05, PdEvent::CableReset),
        (false, 0x05, PdEvent::CableReset),
    ] {
        let c = parse(&chunk(&[status(0, 0, 0, 0, 0), event(meter, 7, code)]));
        let r = &c.records[0];
        assert_eq!(the_event(r), want);
        assert_eq!(r.from_meter, meter, "event {code:#04x}, meter {meter}");
        assert_eq!(r.time_ms, 7);
    }
}

#[test]
fn bad_message_id_and_big_message_events() {
    let c = parse(&chunk(&[
        status(0, 0, 0, 0, 0),
        event(false, 1, 0x03),
        event(false, 2, 0x08),
    ]));
    assert_eq!(the_event(&c.records[0]), PdEvent::BadMessageId);
    assert_eq!(the_event(&c.records[1]), PdEvent::BigMessage);
}

#[test]
fn receive_errors_carry_their_cause() {
    let causes = [
        ReceiveErrorCause::Bmc,
        ReceiveErrorCause::PulseNumber,
        ReceiveErrorCause::Freq,
        ReceiveErrorCause::Kcode,
        ReceiveErrorCause::Sop,
        ReceiveErrorCause::Eop,
        ReceiveErrorCause::CrcCheck,
        ReceiveErrorCause::DataCount,
        ReceiveErrorCause::Unknown,
        ReceiveErrorCause::Retries,
    ];
    for (param, want) in causes.into_iter().enumerate() {
        let code = 0x06 | (param as u8) << 4;
        let c = parse(&chunk(&[status(0, 0, 0, 0, 0), event(false, 1, code)]));
        assert_eq!(
            the_event(&c.records[0]),
            PdEvent::ReceiveError(want),
            "event byte {code:#04x}"
        );
    }
    let c = parse(&chunk(&[status(0, 0, 0, 0, 0), event(false, 1, 0xA6)]));
    assert_eq!(
        the_event(&c.records[0]),
        PdEvent::ReceiveError(ReceiveErrorCause::Other(10))
    );
}

#[test]
fn monitor_type_events() {
    for (code, want) in [
        (0x07, MonitorType::Pd),
        (0x17, MonitorType::Ufcs),
        (0x27, MonitorType::Vivo),
        (0x37, MonitorType::Other(3)),
    ] {
        let c = parse(&chunk(&[status(0, 0, 0, 0, 0), event(false, 1, code)]));
        assert_eq!(the_event(&c.records[0]), PdEvent::MonitorType(want));
    }
}

#[test]
fn unknown_event_codes_keep_their_raw_byte() {
    // The vendor aliases codes 9..15 onto 1..7; we keep them as they are.
    for code in [0x09u8, 0x0A, 0x1B, 0x0F, 0xFE] {
        let c = parse(&chunk(&[status(0, 0, 0, 0, 0), event(false, 1, code)]));
        assert_eq!(
            the_event(&c.records[0]),
            PdEvent::Unknown(code),
            "event byte {code:#04x}"
        );
    }
}

// ---- AC-TGE83A.5: capture clock ----

#[test]
fn capture_clock_counts_from_the_first_record() {
    let mut clock = CaptureClock::new();
    assert_eq!(clock.elapsed_ms(49_810_000), 0);
    assert_eq!(clock.elapsed_ms(49_810_030), 30);
    assert_eq!(clock.elapsed_ms(49_810_030), 30, "equal times are fine");
    assert_eq!(clock.elapsed_ms(49_811_000), 1_000);
}

#[test]
fn capture_clock_never_goes_backwards_when_the_meter_clock_restarts() {
    let mut clock = CaptureClock::new();
    let before: Vec<u64> = [1_000u32, 1_500, 2_000]
        .iter()
        .map(|&t| clock.elapsed_ms(t))
        .collect();
    assert_eq!(before, [0, 500, 1_000]);
    // The meter rebooted: its clock starts again near zero.
    let a = clock.elapsed_ms(10);
    let b = clock.elapsed_ms(60);
    let c = clock.elapsed_ms(1_060);
    assert!(a >= 1_000, "after a restart: {a} < 1000");
    assert_eq!(b - a, 50, "spacing after the restart is kept");
    assert_eq!(c - b, 1_000);
}

#[test]
fn capture_clock_survives_u32_wraparound() {
    let mut clock = CaptureClock::new();
    assert_eq!(clock.elapsed_ms(u32::MAX - 10), 0);
    let after = clock.elapsed_ms(5);
    let later = clock.elapsed_ms(25);
    assert!(later >= after);
    assert_eq!(later - after, 20);
}
