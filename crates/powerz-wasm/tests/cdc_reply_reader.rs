//! FR X3H3JY: the browser's settings pipe reads the meter's serial port as the
//! desktop app does.
//!
//! - AC-X3H3JY.1 / .2: `CdcReplyReader` (over
//!   `powerz_protocol::adcnew::CdcDemux`) hands out whole binary replies only:
//!   ADCNEW stream frames (type 0x02), PDM text lines and noise bytes are
//!   dropped, a reply split over several pushes comes out once it is whole, a
//!   partial frame left from an earlier session does not hide the reply after
//!   it, and another attribute's data reply still comes out (so the pipe can
//!   see another program on the port).

use powerz_protocol::adcnew::crc8;
use powerz_wasm::{CdcReplyReader, DemoMeter, get_data_request, settings_request};

/// An ADCNEW record frame (att `0x10 | rate`) carrying `records` 20-byte
/// records; every record starts with 0x41 (a PUT_DATA type byte) at a 4-byte
/// aligned offset, which the bare CDC frame length took for a data reply.
fn stream_frame(records: usize, seed: u8) -> Vec<u8> {
    let mut body = Vec::with_capacity(records * 20);
    for r in 0..records {
        let mut rec = [0u8; 20];
        rec[0] = 0x41;
        rec[1] = seed.wrapping_add(u8::try_from(r).unwrap_or(0));
        rec[2] = 0x0b;
        rec[3] = 0x00;
        for (i, b) in rec.iter_mut().enumerate().skip(4) {
            *b = 0xa0 | u8::try_from(i & 0x0f).unwrap_or(0);
        }
        body.extend_from_slice(&rec);
    }
    let words = u8::try_from(body.len() / 4).expect("a short frame");
    let mut frame = vec![0x02, 0x12, words, crc8(&body)];
    frame.extend_from_slice(&body);
    frame
}

/// The last `n` bytes of a stream frame whose start never arrived.
fn partial_tail(n: usize) -> Vec<u8> {
    let frame = stream_frame(2, 0x30);
    frame[frame.len() - n..].to_vec()
}

/// The SETTINGS reply a fresh demo meter's CDC twin gives to request `id`.
fn settings_reply(id: u8) -> Vec<u8> {
    let meter = DemoMeter::new("load-5v-1a").expect("a known scenario");
    let mut twin = meter.cdc_twin();
    let reply = twin.transact(&settings_request(id));
    assert_eq!(reply.first().map(|b| b & 0x7f), Some(0x41));
    assert!(
        reply.len() > 64,
        "a SETTINGS reply spans several USB packets"
    );
    reply
}

/// The PUT_DATA ADC reply (attribute 0x001) of a fresh demo meter: another
/// program's reply on the settings port.
fn foreign_adc_reply(id: u8) -> Vec<u8> {
    let mut meter = DemoMeter::new("load-5v-1a").expect("a known scenario");
    let reply = meter.transact(&get_data_request(id, 0x001));
    assert_eq!(reply.first().map(|b| b & 0x7f), Some(0x41));
    reply
}

fn drain(reader: &mut CdcReplyReader) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while let Some(reply) = reader.next_reply() {
        out.push(reply);
        assert!(out.len() < 64, "next_reply never ran dry");
    }
    out
}

#[test]
fn an_empty_reader_has_no_reply() {
    let mut reader = CdcReplyReader::new();
    assert_eq!(reader.next_reply(), None);
}

#[test]
fn stream_frames_text_lines_and_noise_are_dropped_and_the_reply_comes_out_whole() {
    let reply = settings_reply(7);
    let mut reader = CdcReplyReader::new();
    reader.push(&stream_frame(2, 0x01));
    reader.push(b"Apple 2.4A\n");
    reader.push(&[0x99, 0x93, 0xa7, 0x00, 0xb5]);
    reader.push(&stream_frame(1, 0x11));
    reader.push(&reply);
    reader.push(&stream_frame(3, 0x21));
    reader.push(b"OK\n");
    assert_eq!(drain(&mut reader), vec![reply]);
}

#[test]
fn only_stream_frames_and_text_give_no_reply() {
    let mut reader = CdcReplyReader::new();
    for k in 0u8..8 {
        reader.push(&stream_frame(1 + usize::from(k % 3), k * 7));
        reader.push(b"pdm open\n");
    }
    assert_eq!(drain(&mut reader), Vec::<Vec<u8>>::new());
}

#[test]
fn a_reply_split_over_usb_packets_comes_out_once_whole() {
    let reply = settings_reply(9);
    let mut reader = CdcReplyReader::new();
    for chunk in reply.chunks(64) {
        assert_eq!(reader.next_reply(), None);
        reader.push(chunk);
    }
    assert_eq!(drain(&mut reader), vec![reply]);
}

#[test]
fn stream_frames_between_the_packets_of_two_replies_leave_both_whole() {
    let a = settings_reply(3);
    let b = settings_reply(4);
    let mut reader = CdcReplyReader::new();
    reader.push(&a);
    reader.push(&stream_frame(2, 0x40));
    reader.push(&stream_frame(2, 0x50));
    reader.push(&b);
    assert_eq!(drain(&mut reader), vec![a, b]);
}

#[test]
fn a_partial_frame_left_from_an_earlier_session_does_not_hide_the_next_reply() {
    let reply = settings_reply(5);
    let mut reader = CdcReplyReader::new();
    reader.push(&partial_tail(13));
    reader.push(&stream_frame(2, 0x60));
    reader.push(&reply);
    assert_eq!(drain(&mut reader), vec![reply]);
}

#[test]
fn another_attributes_data_reply_still_comes_out() {
    let foreign = foreign_adc_reply(0x5a);
    let mut reader = CdcReplyReader::new();
    reader.push(&stream_frame(2, 0x70));
    reader.push(&foreign);
    reader.push(b"OK\n");
    assert_eq!(drain(&mut reader), vec![foreign]);
}

#[test]
fn a_four_byte_control_reply_comes_out() {
    let accept = vec![0x05, 0x0c, 0x00, 0x00];
    let mut reader = CdcReplyReader::new();
    reader.push(&stream_frame(1, 0x01));
    reader.push(&accept);
    assert_eq!(drain(&mut reader), vec![accept]);
}

#[test]
fn a_stream_frame_inside_a_reply_breaks_only_that_reply() {
    // The meter put a stream frame between two USB packets of a SETTINGS
    // reply: that reply comes out with the frame's bytes in it (its block
    // check then fails and the settings step reads again), and the clean
    // reply to the next request still comes out whole.
    let spoiled = settings_reply(6);
    let clean = settings_reply(7);
    let frame = stream_frame(2, 0x05);
    let mut reader = CdcReplyReader::new();
    reader.push(&spoiled[..128]);
    reader.push(&frame);
    reader.push(&spoiled[128..]);
    reader.push(&stream_frame(1, 0x15));
    reader.push(&clean);
    let replies = drain(&mut reader);
    assert_eq!(replies.last(), Some(&clean));
    let first = replies.first().expect("the spoiled reply comes out");
    assert_eq!(first.len(), spoiled.len());
    assert_eq!(first[..128], spoiled[..128]);
    assert_ne!(first, &spoiled);
}
