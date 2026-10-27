//! ADCNEW push stream on the CDC port (FR 1HHT02, AC .1 and .2): the frame
//! decoder (4-byte header, body length in words, CRC-8 poly 0x29 over the
//! body, 20-byte records) and the CDC demultiplexer that separates ADCNEW
//! frames, binary replies and PDM text lines on one byte stream.
//!
//! Device frames below were rewritten from the T11 session (firmware 2.0.6,
//! `notes/device/vectors/t11_cdc.jsonl`) as our own fixtures; they carry
//! measurements only, no serial number or UID.

use powerz_protocol::adcnew::{self, CdcDemux, CdcItem, Content, Frame, Request};
use powerz_protocol::decode::Rate;
use proptest::prelude::*;

// ---------------------------------------------------------------- fixtures

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Reply to a bare `02` (meter rate, 50/s): the att-0x03 frame (160 bytes:
/// section-chained ADC + a cut-short queue section), then three record
/// frames `02 12 05 xx`. [device]
const BARE_START_REPLY: &str = "
    020328c3
    0180000b64100000c9ffffff7f100000a5ffffffe3100000f5ffffff020f3203
    2601ce02f702307d008002f00000ff0002001f05903801003d100000a1ffffff
    52001d0044004d00a43801003d10000099ffffff52001c0048004a00b8380100
    a10f000099ffffff51001d0045004b00cc380100a10f000091ffffff52001c00
    48004a00e0380100a10f000089ffffff52001c0043004f00f4380100a10f0000
    02120598 fc3a010064100000c5ffffff52001c0049004600
    021205e9 103b010064100000c1ffffff53001c0047004800
    0212055d 243b0100ef0f0000b5ffffff4f00200045004b00";

/// Two 50/s record frames, one record each. [device]
const TWO_FRAMES_50: &str = "
    0212057c 383b0100a10f0000b5ffffff53001c0041004c00
    021205e4 4c3b0100a10f0000b5ffffff53001b0046004b00";

/// The first two 1000/s frames: one record, then 11 records (obj 0x37). [device]
const FRAMES_1000: &str = "
    02130544 b2330100a10f0000c2feffff4b001b0042004b00
    02133733 b3330100a10f0000c2feffff52001c004d004400
             b4330100a10f0000c2feffff590021004b004500
             b5330100a10f0000c2feffff5a00200047004f00
             b6330100a10f0000c2feffff5600220044004700
             b7330100a10f0000c2feffff5700210041004c00
             b8330100a10f0000c2feffff5a00200044004b00
             b9330100a10f0000c2feffff5800240045004d00
             ba330100a10f0000c2feffff5900270045004a00
             bb330100a10f0000c2feffff590023004a005200
             bc330100a10f0000c2feffff5900230048004500
             bd330100a10f000041ffffff5600240046004d00";

/// The 52-byte binary ADC reply (`GET_DATA` mask 0x001, id 1) read on the
/// CDC port in the same session. [device]
const CDC_ADC_REPLY: &str = "
    41018002 0100000b 64100000e9ffffff651000008bffffffc9100000dbffffff
    010f33032701d402f402337d008002f00000ff00";

/// Reference CRC-8: poly 0x29, init 0, no reflection, xorout 0.
fn ref_crc8(body: &[u8]) -> u8 {
    let mut c = 0u8;
    for &b in body {
        c ^= b;
        for _ in 0..8 {
            c = if c & 0x80 != 0 {
                (c << 1) ^ 0x29
            } else {
                c << 1
            };
        }
    }
    c
}

/// One 20-byte record: u32 ms time, i32 VBUS µV, i32 IBUS µA, u16 CC1, CC2, D+, D−.
fn record(time_ms: u32, vbus_uv: i32, ibus_ua: i32, lines: [u16; 4]) -> Vec<u8> {
    let mut r = Vec::with_capacity(20);
    r.extend(time_ms.to_le_bytes());
    r.extend(vbus_uv.to_le_bytes());
    r.extend(ibus_ua.to_le_bytes());
    for l in lines {
        r.extend(l.to_le_bytes());
    }
    r
}

/// A well-formed ADCNEW frame with `att` around `body` (a multiple of 4 bytes).
fn frame(att: u8, body: &[u8]) -> Vec<u8> {
    assert_eq!(body.len() % 4, 0);
    let mut f = vec![0x02, att, (body.len() / 4) as u8, ref_crc8(body)];
    f.extend_from_slice(body);
    f
}

/// Split a byte run of back-to-back frames by their `obj` field.
fn split_frames(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 4 <= bytes.len() {
        let len = 4 + 4 * usize::from(bytes[at + 2]);
        out.push(bytes[at..at + len].to_vec());
        at += len;
    }
    assert_eq!(at, bytes.len(), "fixture is not whole frames");
    out
}

fn approx(got: f64, want: f64) {
    assert!((got - want).abs() < 1e-9, "got {got}, want {want}");
}

fn records_of(f: &Frame) -> Vec<powerz_protocol::decode::QueueRecord> {
    match f {
        Frame::Records { records, .. } => records.clone(),
        other => panic!("expected a record frame, got {other:?}"),
    }
}

/// Every item the demux has ready; a text error fails the test.
fn drain(d: &mut CdcDemux) -> Vec<CdcItem> {
    let mut items = Vec::new();
    while let Some(item) = d.next_item() {
        items.push(item.unwrap_or_else(|e| panic!("demux error: {e:?}")));
    }
    items
}

/// Push `bytes` in one go and drain.
fn demux_all(bytes: &[u8]) -> (Vec<CdcItem>, CdcDemux) {
    let mut d = CdcDemux::new();
    d.push(bytes);
    let items = drain(&mut d);
    (items, d)
}

fn frames_only(items: &[CdcItem]) -> Vec<Frame> {
    items
        .iter()
        .filter_map(|i| match i {
            CdcItem::Frame(f) => Some(f.clone()),
            _ => None,
        })
        .collect()
}

fn record_times(items: &[CdcItem]) -> Vec<u32> {
    frames_only(items)
        .iter()
        .flat_map(|f| records_of(f).into_iter().map(|r| r.time_ms()))
        .collect()
}

// ---- AC-1HHT02.1: CRC-8 poly 0x29 over the body ----

#[test]
fn crc8_matches_every_device_frame() {
    let mut all = hex(BARE_START_REPLY);
    all.extend(hex(TWO_FRAMES_50));
    all.extend(hex(FRAMES_1000));
    let frames = split_frames(&all);
    assert_eq!(frames.len(), 4 + 2 + 2);
    for f in frames {
        assert_eq!(adcnew::crc8(&f[4..]), f[3], "frame {:02x?}", &f[..4]);
    }
}

#[test]
fn crc8_has_no_init_or_xorout() {
    assert_eq!(adcnew::crc8(&[]), 0x00);
    assert_eq!(adcnew::crc8(&[0u8; 20]), 0x00);
    assert_eq!(adcnew::crc8(&[0x01]), 0x29);
    assert_eq!(adcnew::crc8(&[0x80]), ref_crc8(&[0x80]));
    let body: Vec<u8> = (0u8..=255).collect();
    assert_eq!(adcnew::crc8(&body), ref_crc8(&body));
}

// ---- AC-1HHT02.1: one frame, its header and its 20-byte records ----

#[test]
fn decodes_a_50_sps_record_frame() {
    let f = &split_frames(&hex(TWO_FRAMES_50))[0];
    let decoded = adcnew::decode_frame(f).expect("valid device frame");
    match &decoded {
        Frame::Records { rate, records } => {
            assert_eq!(*rate, Rate::Sps50, "att 0x12 is 0x10 | rate code 2");
            assert_eq!(records.len(), 1);
            let r = records[0];
            assert_eq!(r.time_ms(), 80_696);
            assert_eq!(r.vbus_uv(), 4_001);
            assert_eq!(r.ibus_ua(), -75);
            assert_eq!(r.raw_lines(), [83, 28, 65, 76]);
            // 1 mV per unit at rate codes 1..3.
            approx(r.cc1_v(), 0.083);
            approx(r.dm_v(), 0.076);
            assert_eq!(r.rate(), Rate::Sps50);
        }
        other => panic!("expected records, got {other:?}"),
    }
}

#[test]
fn decodes_a_1000_sps_frame_of_eleven_records() {
    let frames = split_frames(&hex(FRAMES_1000));
    let first = adcnew::decode_frame(&frames[0]).unwrap();
    assert_eq!(records_of(&first).len(), 1);
    let eleven = adcnew::decode_frame(&frames[1]).unwrap();
    match &eleven {
        Frame::Records { rate, records } => {
            assert_eq!(*rate, Rate::Sps1000);
            let times: Vec<u32> = records.iter().map(|r| r.time_ms()).collect();
            assert_eq!(times, (78_771..=78_781).collect::<Vec<u32>>());
            assert_eq!(records[10].ibus_ua(), -191);
            assert_eq!(records[0].raw_lines(), [82, 28, 77, 68]);
        }
        other => panic!("expected records, got {other:?}"),
    }
}

#[test]
fn rate_code_0_uses_the_tenth_millivolt_scale() {
    let body = record(1_000, 5_000_000, 1_000_000, [16_800, 0, 6_000, 6_000]);
    let f = adcnew::decode_frame(&frame(0x10, &body)).unwrap();
    let r = records_of(&f)[0];
    assert_eq!(r.rate(), Rate::Sps2);
    approx(r.cc1_v(), 1.68);
    approx(r.dp_v(), 0.6);
    approx(r.vbus_v(), 5.0);
    approx(r.ibus_a(), 1.0);
}

#[test]
fn the_first_frame_after_a_bare_start_is_kept_raw() {
    let frames = split_frames(&hex(BARE_START_REPLY));
    let first = adcnew::decode_frame(&frames[0]).unwrap();
    assert_eq!(
        first,
        Frame::Raw {
            att: 0x03,
            body: frames[0][4..].to_vec()
        }
    );
    if let Frame::Raw { body, .. } = &first {
        assert_eq!(body.len(), 160);
    }
    // The record frames after it decode at the meter's rate.
    for f in &frames[1..] {
        match adcnew::decode_frame(f).unwrap() {
            Frame::Records { rate, records } => {
                assert_eq!(rate, Rate::Sps50);
                assert_eq!(records.len(), 1);
            }
            other => panic!("expected records, got {other:?}"),
        }
    }
}

#[test]
fn pd_and_ufcs_content_frames_are_kept_raw() {
    for att in [0x20, 0x21, 0x30, 0x42] {
        let body = [0xA5u8; 12];
        let f = frame(att, &body);
        assert_eq!(
            adcnew::decode_frame(&f).unwrap(),
            Frame::Raw {
                att,
                body: body.to_vec()
            },
            "att {att:#04x}"
        );
    }
}

#[test]
fn a_bad_crc_is_refused() {
    let mut f = split_frames(&hex(TWO_FRAMES_50))[0].clone();
    f[3] ^= 0x01;
    assert!(adcnew::decode_frame(&f).is_err());
    let mut g = split_frames(&hex(TWO_FRAMES_50))[0].clone();
    g[10] ^= 0x40; // a body byte
    assert!(adcnew::decode_frame(&g).is_err());
}

#[test]
fn a_bad_length_is_refused() {
    let f = split_frames(&hex(TWO_FRAMES_50))[0].clone();
    // Shorter or longer than 4 + obj × 4.
    assert!(adcnew::decode_frame(&f[..f.len() - 4]).is_err());
    let mut longer = f.clone();
    longer.extend([0, 0, 0, 0]);
    assert!(adcnew::decode_frame(&longer).is_err());
    // A record frame whose body is not whole 20-byte records, CRC correct.
    assert!(adcnew::decode_frame(&frame(0x12, &[0x11; 16])).is_err());
    assert!(adcnew::decode_frame(&frame(0x13, &[0x11; 44])).is_err());
    // Too short for a header; not a stream frame.
    assert!(adcnew::decode_frame(&[0x02, 0x12, 0x00]).is_err());
    assert!(adcnew::decode_frame(&[0x41, 0x12, 0x00, 0x00]).is_err());
}

// ---- AC-1HHT02.1: requests ----

#[test]
fn requests_encode_as_the_device_takes_them() {
    assert_eq!(Request::StartDefault.encode(), vec![0x02]);
    assert_eq!(
        Request::Start {
            content: Content::Adc,
            rate: Rate::Sps50
        }
        .encode(),
        vec![0x02, 0x02]
    );
    assert_eq!(
        Request::Start {
            content: Content::AdcPd,
            rate: Rate::Sps1000
        }
        .encode(),
        vec![0x02, 0x13]
    );
    assert_eq!(
        Request::Start {
            content: Content::AdcUfcs,
            rate: Rate::Sps10
        }
        .encode(),
        vec![0x02, 0x21]
    );
    assert_eq!(
        Request::Start {
            content: Content::AdcPdUfcs,
            rate: Rate::Sps2
        }
        .encode(),
        vec![0x02, 0x30]
    );
    assert_eq!(Request::Stop.encode(), vec![0x03]);
    assert_eq!(Request::KeepAlive.encode(), vec![0x01]);
}

// ---- AC-1HHT02.1: framing, drop and resync in the stream reader ----

#[test]
fn frames_split_over_many_pushes_are_reassembled() {
    let bytes = hex(TWO_FRAMES_50);
    let mut d = CdcDemux::new();
    let mut items = Vec::new();
    for b in &bytes {
        d.push(std::slice::from_ref(b));
        items.extend(drain(&mut d));
    }
    assert_eq!(record_times(&items), vec![80_696, 80_716]);
    assert_eq!(d.dropped_frames(), 0);
}

#[test]
fn a_frame_with_a_bad_crc_is_dropped_and_the_next_ones_decode() {
    let mut bytes = hex(TWO_FRAMES_50);
    bytes.extend(split_frames(&hex(BARE_START_REPLY))[3].clone());
    bytes[3] ^= 0xFF; // CRC of the first frame
    let (items, d) = demux_all(&bytes);
    assert_eq!(record_times(&items), vec![80_716, 80_676]);
    assert_eq!(d.dropped_frames(), 1);
}

#[test]
fn a_frame_with_a_corrupt_length_is_dropped_and_the_stream_resyncs() {
    let mut bytes = hex(TWO_FRAMES_50);
    bytes.extend(split_frames(&hex(BARE_START_REPLY))[3].clone());
    bytes[2] = 0x07; // 28-byte body: runs into the next frame's header
    let (items, d) = demux_all(&bytes);
    assert_eq!(record_times(&items), vec![80_716, 80_676]);
    assert!(d.dropped_frames() >= 1);
}

#[test]
fn a_huge_corrupt_length_does_not_swallow_the_frames_after_it() {
    let good: Vec<Vec<u8>> = (0..20u32)
        .map(|i| {
            frame(
                0x12,
                &record(1_000 + 20 * i, 5_000_000, 1_000_000, [1680, 0, 600, 600]),
            )
        })
        .collect();
    let mut bytes = frame(
        0x12,
        &record(980, 5_000_000, 1_000_000, [1680, 0, 600, 600]),
    );
    bytes[2] = 0x40; // claims 256 body bytes
    for g in &good {
        bytes.extend(g);
    }
    let (items, d) = demux_all(&bytes);
    assert_eq!(
        record_times(&items),
        (0..20u32).map(|i| 1_000 + 20 * i).collect::<Vec<u32>>()
    );
    assert!(d.dropped_frames() >= 1);
}

#[test]
fn a_record_frame_of_partial_records_is_dropped() {
    let mut bytes = frame(0x12, &[0x11; 16]);
    bytes.extend(frame(0x12, &record(2_000, 1, 2, [3, 4, 5, 6])));
    let (items, d) = demux_all(&bytes);
    assert_eq!(record_times(&items), vec![2_000]);
    assert_eq!(d.dropped_frames(), 1);
}

#[test]
fn an_incomplete_frame_waits_for_more_bytes() {
    let bytes = hex(TWO_FRAMES_50);
    let mut d = CdcDemux::new();
    d.push(&bytes[..30]);
    let first = drain(&mut d);
    assert_eq!(record_times(&first), vec![80_696]);
    assert_eq!(d.dropped_frames(), 0, "a partial frame is not a bad one");
    d.push(&bytes[30..]);
    assert_eq!(record_times(&drain(&mut d)), vec![80_716]);
}

// ---- AC-1HHT02.2: frames, binary replies and text lines on one stream ----

/// Frame, `ok`, the ADC reply, frame, a reply line, a control reply,
/// a text line that starts with `A`, frame.
fn interleaved() -> (Vec<u8>, Vec<CdcItem>) {
    let f50 = split_frames(&hex(TWO_FRAMES_50));
    let adc = hex(CDC_ADC_REPLY);
    let unknown = vec![0x0F, 0x00, 0x00, 0x55];
    let last = frame(0x12, &record(80_736, 4_001, -75, [83, 28, 65, 76]));
    let mut bytes = Vec::new();
    bytes.extend(&f50[0]);
    bytes.extend(b"ok\n");
    bytes.extend(&adc);
    bytes.extend(&f50[1]);
    bytes.extend(b"ready:5191mV,0mA\n");
    bytes.extend(&unknown);
    bytes.extend(b"Apple 2.4A\n");
    bytes.extend(&last);
    let want = vec![
        CdcItem::Frame(adcnew::decode_frame(&f50[0]).unwrap()),
        CdcItem::Line("ok".into()),
        CdcItem::Reply(adc),
        CdcItem::Frame(adcnew::decode_frame(&f50[1]).unwrap()),
        CdcItem::Line("ready:5191mV,0mA".into()),
        CdcItem::Reply(unknown),
        CdcItem::Line("Apple 2.4A".into()),
        CdcItem::Frame(adcnew::decode_frame(&last).unwrap()),
    ];
    (bytes, want)
}

#[test]
fn demux_separates_frames_replies_and_text_in_order() {
    let (bytes, want) = interleaved();
    let (items, d) = demux_all(&bytes);
    assert_eq!(items, want);
    assert_eq!(d.dropped_frames(), 0);
    assert_eq!(d.buffered(), 0);
}

#[test]
fn demux_splits_a_device_read_of_frames_around_a_pdm_line() {
    // One tty read from a PDM session (session 2): two 50/s frames, the
    // `ready:` reply line, then another frame. [device]
    let bytes = hex("
        021205314392f602a0286900b9fffffff905040009001d00
        021205215792f602a9f95e00b1fffffffa0505000a001e00
        72656164793a353537306d562c306d410a
        021205a26b92f60282215300b1fffffff90503000d002000");
    let (items, d) = demux_all(&bytes);
    assert_eq!(items.len(), 4, "{items:?}");
    assert_eq!(items[2], CdcItem::Line("ready:5570mV,0mA".into()));
    assert_eq!(frames_only(&items).len(), 3);
    assert_eq!(d.dropped_frames(), 0);
}

#[test]
fn demux_hands_out_a_binary_reply_between_1000_sps_frames() {
    // The 1000/s frames around a binary reply: the reply comes out whole,
    // framed by its section chain, and both frames survive.
    let f = split_frames(&hex(FRAMES_1000));
    let adc = hex(CDC_ADC_REPLY);
    let mut bytes = f[0].clone();
    bytes.extend(&adc);
    bytes.extend(&f[1]);
    let (items, _) = demux_all(&bytes);
    assert_eq!(items.len(), 3, "{items:?}");
    assert_eq!(items[1], CdcItem::Reply(adc));
    assert_eq!(record_times(&items).len(), 12);
}

#[test]
fn a_bad_frame_does_not_swallow_a_following_text_line() {
    let mut bad = split_frames(&hex(TWO_FRAMES_50))[0].clone();
    bad[3] ^= 0x5A;
    let mut bytes = bad;
    bytes.extend(b"ok\n");
    bytes.extend(split_frames(&hex(TWO_FRAMES_50))[1].clone());
    let (items, d) = demux_all(&bytes);
    assert_eq!(d.dropped_frames(), 1);
    assert!(
        items.contains(&CdcItem::Line("ok".into())),
        "the reply line was lost: {items:?}"
    );
    assert_eq!(record_times(&items), vec![80_716]);
}

#[test]
fn a_bad_frame_does_not_swallow_a_following_binary_reply() {
    let mut bad = split_frames(&hex(TWO_FRAMES_50))[0].clone();
    bad[3] ^= 0x5A;
    let adc = hex(CDC_ADC_REPLY);
    let mut bytes = bad;
    bytes.extend(&adc);
    bytes.extend(split_frames(&hex(TWO_FRAMES_50))[1].clone());
    let (items, d) = demux_all(&bytes);
    assert_eq!(d.dropped_frames(), 1);
    assert!(
        items.contains(&CdcItem::Reply(adc)),
        "the binary reply was lost: {items:?}"
    );
    assert_eq!(record_times(&items), vec![80_716]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// However the port chops the stream into reads, the items are the same.
    #[test]
    fn demux_is_independent_of_read_boundaries(cuts in proptest::collection::vec(any::<prop::sample::Index>(), 0..12)) {
        let (bytes, want) = interleaved();
        let mut points: Vec<usize> = cuts.iter().map(|i| i.index(bytes.len())).collect();
        points.sort_unstable();
        points.dedup();
        let mut d = CdcDemux::new();
        let mut items = Vec::new();
        let mut at = 0;
        for p in points.into_iter().chain([bytes.len()]) {
            d.push(&bytes[at..p]);
            items.extend(drain(&mut d));
            at = p;
        }
        prop_assert_eq!(items, want);
        prop_assert_eq!(d.dropped_frames(), 0);
    }

    /// Arbitrary bytes never panic the demux, and good frames pushed after
    /// a text line end still come out once enough of them arrived to rule
    /// out any frame the garbage started (at most 4 + 255 × 4 bytes).
    /// Binary-reply types that declare long payloads (0x41, 0x44, 0x4C) are
    /// left out of the garbage: a real reply of that size is legitimate.
    #[test]
    fn demux_never_panics_on_garbage(raw in proptest::collection::vec(any::<u8>(), 0..300)) {
        let garbage: Vec<u8> = raw
            .iter()
            .map(|&b| if matches!(b & 0x7F, 0x41 | 0x44 | 0x4C) { 0x00 } else { b })
            .collect();
        let mut d = CdcDemux::new();
        d.push(&garbage);
        while let Some(_item) = d.next_item() {}
        let good = frame(0x12, &record(7_777, 1, 2, [3, 4, 5, 6]));
        d.push(b"\n");
        for _ in 0..60 {
            d.push(&good);
        }
        let mut times = Vec::new();
        while let Some(item) = d.next_item() {
            if let Ok(CdcItem::Frame(Frame::Records { records, .. })) = item {
                times.extend(records.iter().map(|r| r.time_ms()));
            }
        }
        prop_assert!(times.contains(&7_777), "good frames lost after garbage {:02x?}", garbage);
    }
}
