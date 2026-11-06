//! FR A2XTES: the simulated logo flash, driven through the protocol's
//! public logo paths (`transact_logo_chunk`, `transact_logo_flag`,
//! `Session::logo_memory_read`) and, for frames the guard would refuse,
//! raw encrypted frames.
//!
//! AC-A2XTES.1 decrypt + block-header check, FINISHED / REJECT;
//! AC-A2XTES.2 erase policies after a large logo and Remove;
//! AC-A2XTES.3 auth toggle; AC-A2XTES.4 faults; AC-A2XTES.5 flag 0x1C; AC-A2XTES.6 reads of the 0x98300000 window;
//! AC-A2XTES.7 boot render on what the meter holds.

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::frame::Attribute;
use powerz_protocol::logo::{self, LogoBlob, LogoMode, Rgba};
use powerz_protocol::session::{memory_read_plaintext, memory_read_request};
use powerz_protocol::transport::guard::{LOGO_REGION_END, LOGO_REGION_START};
use powerz_protocol::transport::{Error as TError, GuardedTransport, Transport};
use powerz_sim::{BootScreen, ErasePolicy, LogoFault, boot_render};

const BASE: u32 = LOGO_REGION_START;
const WINDOW: u32 = 0x9830_0000;
const SECTOR: usize = 4096;

// ---- frame builders (layout from docs/protocol.md, "Startup logo") ----

fn le(w: u32) -> [u8; 4] {
    w.to_le_bytes()
}

/// Block header with explicit fields; the CRC covers the first 12 bytes
/// unless `crc` overrides it.
fn block_header(addr: u32, len2: u32, word3: u32, crc: Option<u32>) -> Vec<u8> {
    let mut h = [le(addr), le(len2), le(word3)].concat();
    let c = crc.unwrap_or_else(|| crypto::crc32(&h));
    h.extend(le(c));
    h
}

/// Plaintext chunk frame: main header, block header, data.
fn plain_with(id: u8, header: Vec<u8>, data: &[u8]) -> Vec<u8> {
    let len = data.len() as u32;
    let main = 0x4A | (u32::from(id) << 8) | (1 << 16) | ((len + 16) << 22);
    let mut f = le(main).to_vec();
    f.extend(header);
    f.extend_from_slice(data);
    f
}

fn plain_chunk(id: u8, addr: u32, data: &[u8]) -> Vec<u8> {
    plain_with(
        id,
        block_header(addr, data.len() as u32, 0xFFFF_FFFF, None),
        data,
    )
}

/// What goes on the wire: bytes 4.. encrypted with `key`.
fn encrypt_with(key: &[u8; 16], plain: &[u8]) -> Vec<u8> {
    let mut f = plain[..4].to_vec();
    f.extend(crypto::encrypt(key, &plain[4..]));
    f
}

fn remove_plain(id: u8) -> Vec<u8> {
    plain_chunk(id, BASE, &[0xFF; 16])
}

/// SET_DATA 0x1C with `value` (the guard's layout).
fn flag_frame(id: u8, value: u32) -> Vec<u8> {
    let mut f = le(0x48 | (u32::from(id) << 8) | (3 << 22)).to_vec();
    f.extend(le(0x1C | (4 << 22)));
    f.extend(le(value));
    f
}

fn image(w: u32, h: u32) -> Rgba {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            for c in 0..3u32 {
                px.push(((x * 3 + y * 5 + c * 17) % 251) as u8);
            }
            px.push(255);
        }
    }
    Rgba { w, h, px }
}

/// 48×48 opaque: 19 chunks.
fn small_blob() -> LogoBlob {
    logo::encode(&image(48, 48), LogoMode::Opaque).unwrap()
}

/// 240×240 opaque: 451 chunks.
fn large_blob() -> LogoBlob {
    logo::encode(&image(240, 240), LogoMode::Opaque).unwrap()
}

/// The plaintext chunk frames of `blob` (ids from 1).
fn chunks(blob: &LogoBlob) -> Vec<Vec<u8>> {
    blob.padded()
        .chunks(logo::CHUNK)
        .enumerate()
        .map(|(k, d)| plain_chunk((k + 1) as u8, BASE + (k * logo::CHUNK) as u32, d))
        .collect()
}

fn assert_finished(reply: &[u8], id: u8, what: &str) {
    assert_eq!(reply.len(), 4, "{what}: {reply:02x?}");
    assert_eq!(
        ty(reply),
        0x07,
        "{what}: FINISHED expected, got {reply:02x?}"
    );
    assert_eq!(reply[1], id, "{what}: reply id is the request's");
}

fn assert_reject(reply: &[u8], id: u8, what: &str) {
    assert_eq!(reply.len(), 4, "{what}: {reply:02x?}");
    assert_eq!(ty(reply), 0x06, "{what}: REJECT expected, got {reply:02x?}");
    assert_eq!(reply[1], id, "{what}: reply id is the request's");
}

/// Write every chunk of `blob`, each answered FINISHED.
fn write_blob<D: Transport>(d: &mut D, blob: &LogoBlob) {
    for (k, c) in chunks(blob).into_iter().enumerate() {
        let reply = d.transact_logo_chunk(&c, T).expect("chunk reply");
        assert_finished(&reply, c[1], &format!("chunk {k}"));
    }
}

fn region_len() -> usize {
    (LOGO_REGION_END - BASE) as usize
}

fn connected_dev(f: impl FnOnce(SimDevice) -> SimDevice) -> SimDevice {
    let clock = clock();
    raw_connect(f(device(Persona::Bulk, Scenario::Idle, &clock)))
}

// ---- AC-A2XTES.1: decrypt, check, FINISHED / REJECT ----

#[test]
fn a2xtes_chunks_land_decrypted_and_are_finished_with_the_request_id() {
    let mut d = connected_dev(|d| d);
    let padded = small_blob().padded();
    for (k, id) in [(0usize, 0x21u8), (1, 0x22), (7, 0x9F)] {
        let addr = BASE + (k * 256) as u32;
        let data = &padded[k * 256..k * 256 + 256];
        let reply = d
            .transact_logo_chunk(&plain_chunk(id, addr, data), T)
            .unwrap();
        assert_finished(&reply, id, &format!("chunk {k}"));
        assert_eq!(d.logo_flash().read(addr, 256), data.to_vec(), "chunk {k}");
    }
    // Chunks not written stay erased.
    assert_eq!(d.logo_flash().read(BASE + 2 * 256, 256), vec![0xFF; 256]);
}

#[test]
fn a2xtes_a_whole_logo_through_the_guard_reads_back_byte_for_byte() {
    let d = connected_dev(|d| d.with_logo_erase(ErasePolicy::SectorOnFirstWrite));
    let mut g = GuardedTransport::new(d);
    let blob = small_blob();
    write_blob(&mut g, &blob);
    let padded = blob.padded();
    assert_eq!(g.inner().logo_flash().read(BASE, padded.len()), padded);
}

#[test]
fn a2xtes_a_bad_block_header_is_rejected_and_writes_nothing() {
    let mut d = connected_dev(|d| d.with_logo_erase(ErasePolicy::NoErase));
    write_blob(&mut d, &small_blob());
    let before = d.logo_flash().read(BASE, 0x2000);
    let zeros = [0u8; 256];
    let addr = BASE + 256;
    let bad: Vec<(&str, Vec<u8>)> = vec![
        (
            "bad CRC",
            encrypt_with(
                &KEYS[0],
                &plain_with(
                    0x31,
                    block_header(addr, 256, 0xFFFF_FFFF, Some(0x1234_5678)),
                    &zeros,
                ),
            ),
        ),
        (
            "word 3 not 0xFFFFFFFF",
            encrypt_with(
                &KEYS[0],
                &plain_with(0x32, block_header(addr, 256, 0, None), &zeros),
            ),
        ),
        (
            "second length 16 for 256 data bytes",
            encrypt_with(
                &KEYS[0],
                &plain_with(0x33, block_header(addr, 16, 0xFFFF_FFFF, None), &zeros),
            ),
        ),
        (
            "second length 240 for 256 data bytes",
            encrypt_with(
                &KEYS[0],
                &plain_with(0x34, block_header(addr, 240, 0xFFFF_FFFF, None), &zeros),
            ),
        ),
        (
            "encrypted with key slot 1",
            encrypt_with(&KEYS[1], &plain_chunk(0x35, addr, &zeros)),
        ),
        ("sent in plaintext", plain_chunk(0x36, addr, &zeros)),
    ];
    for (what, frame) in bad {
        let id = frame[1];
        let reply = d.transact(&frame, T).expect("a reply");
        assert_reject(&reply, id, what);
        assert_eq!(
            d.logo_flash().read(BASE, 0x2000),
            before,
            "{what}: no flash byte changed"
        );
    }
    // A good chunk still goes through afterwards.
    let reply = d
        .transact(&encrypt_with(&KEYS[0], &plain_chunk(0x37, addr, &zeros)), T)
        .unwrap();
    assert_finished(&reply, 0x37, "good chunk after the rejects");
    assert_eq!(d.logo_flash().read(addr, 256), vec![0u8; 256]);
}

// ---- AC-A2XTES.2: erase policies after a large logo, then Remove ----

/// Large logo then Remove, both through the guard; returns the meter.
fn large_then_remove(policy: ErasePolicy) -> GuardedTransport<SimDevice> {
    let d = connected_dev(|d| d.with_logo_erase(policy));
    let mut g = GuardedTransport::new(d);
    write_blob(&mut g, &large_blob());
    let reply = g.transact_logo_chunk(&remove_plain(0x77), T).unwrap();
    assert_finished(&reply, 0x77, "Remove");
    let reply = g.transact_logo_flag(&flag_frame(0x78, 0), T).unwrap();
    assert_finished(&reply, 0x78, "flag 0");
    g
}

#[test]
fn a2xtes_sector_policy_remove_erases_only_the_first_sector() {
    let g = large_then_remove(ErasePolicy::SectorOnFirstWrite);
    let padded = large_blob().padded();
    let f = g.inner().logo_flash();
    assert_eq!(f.read(BASE, SECTOR), vec![0xFF; SECTOR]);
    assert_eq!(
        f.read(BASE + SECTOR as u32, padded.len() - SECTOR),
        padded[SECTOR..].to_vec(),
        "old pixels remain above the first sector"
    );
}

#[test]
fn a2xtes_region_policy_remove_erases_the_whole_region() {
    let g = large_then_remove(ErasePolicy::RegionOnStartWrite);
    let n = region_len();
    assert_eq!(g.inner().logo_flash().read(BASE, n), vec![0xFF; n]);
}

#[test]
fn a2xtes_no_erase_policy_remove_leaves_the_old_logo() {
    let g = large_then_remove(ErasePolicy::NoErase);
    let padded = large_blob().padded();
    assert_eq!(g.inner().logo_flash().read(BASE, padded.len()), padded);
}

// ---- AC-A2XTES.3: auth toggle ----

#[test]
fn a2xtes_without_the_toggle_chunks_are_accepted_without_auth() {
    let mut d = connected_dev(|d| d);
    let c = plain_chunk(0x41, BASE, &small_blob().padded()[..256]);
    let reply = d.transact_logo_chunk(&c, T).unwrap();
    assert_finished(&reply, 0x41, "unauthenticated chunk");
    assert_eq!(permi(&reply), 0, "session level 0");
}

#[test]
fn a2xtes_with_the_toggle_chunks_are_rejected_until_auth() {
    let clock = clock();
    let dev = device(Persona::Bulk, Scenario::Idle, &clock).with_logo_auth_required(true);
    let mut s = session(dev, &clock);
    s.connect().unwrap();
    let data = small_blob().padded()[..256].to_vec();
    let reply = s
        .transport_mut()
        .transact_logo_chunk(&plain_chunk(0x42, BASE, &data), T)
        .unwrap();
    assert_reject(&reply, 0x42, "before auth");
    assert_eq!(s.transport().logo_flash().read(BASE, 256), vec![0xFF; 256]);

    assert_eq!(s.authenticate(1), Ok(1));
    let reply = s
        .transport_mut()
        .transact_logo_chunk(&plain_chunk(0x43, BASE, &data), T)
        .unwrap();
    assert_finished(&reply, 0x43, "after auth");
    assert_eq!(s.transport().logo_flash().read(BASE, 256), data);
}

// ---- AC-A2XTES.4: faults ----

#[test]
fn a2xtes_no_reply_at_k_times_out_that_chunk_only() {
    let mut d = connected_dev(|d| d.with_logo_fault(LogoFault::NoReplyAt(1)));
    let cs = chunks(&small_blob());
    assert_finished(
        &d.transact_logo_chunk(&cs[0], T).unwrap(),
        cs[0][1],
        "chunk 0",
    );
    assert_eq!(d.transact_logo_chunk(&cs[1], T), Err(TError::Timeout));
    assert_eq!(d.read(T), Err(TError::Timeout), "nothing pending after it");
    assert_finished(
        &d.transact_logo_chunk(&cs[2], T).unwrap(),
        cs[2][1],
        "chunk 2",
    );
    assert_eq!(d.logo_flash().read(BASE + 512, 256), cs[2][20..].to_vec());
}

#[test]
fn a2xtes_reject_at_k_rejects_that_chunk_and_writes_nothing() {
    let mut d = connected_dev(|d| d.with_logo_fault(LogoFault::RejectAt(2)));
    let cs = chunks(&small_blob());
    for c in &cs[..2] {
        assert_finished(&d.transact_logo_chunk(c, T).unwrap(), c[1], "early");
    }
    let reply = d.transact_logo_chunk(&cs[2], T).unwrap();
    assert_reject(&reply, cs[2][1], "chunk 2");
    assert_eq!(d.logo_flash().read(BASE + 512, 256), vec![0xFF; 256]);
    assert_finished(
        &d.transact_logo_chunk(&cs[3], T).unwrap(),
        cs[3][1],
        "chunk 3",
    );
    assert_eq!(d.logo_flash().read(BASE + 768, 256), cs[3][20..].to_vec());
}

#[test]
fn a2xtes_unplug_after_k_answers_k_then_every_call_is_disconnected() {
    let mut d = connected_dev(|d| d.with_logo_fault(LogoFault::UnplugAfter(1)));
    let cs = chunks(&small_blob());
    assert_finished(
        &d.transact_logo_chunk(&cs[0], T).unwrap(),
        cs[0][1],
        "chunk 0",
    );
    assert_finished(
        &d.transact_logo_chunk(&cs[1], T).unwrap(),
        cs[1][1],
        "chunk 1",
    );
    assert_eq!(d.logo_flash().read(BASE + 256, 256), cs[1][20..].to_vec());
    assert_eq!(d.transact_logo_chunk(&cs[2], T), Err(TError::Disconnected));
    assert_eq!(
        d.transact(&get_data(0x50, Attribute::ADC), T),
        Err(TError::Disconnected)
    );
    assert_eq!(d.read(T), Err(TError::Disconnected));
    assert_eq!(
        d.transact_logo_flag(&flag_frame(0x51, 2), T),
        Err(TError::Disconnected)
    );
    assert_eq!(d.logo_flash().read(BASE + 512, 256), vec![0xFF; 256]);
    assert_eq!(d.logo_flag(), 0, "the flag was never written");
}

#[test]
fn a2xtes_fault_index_counts_0x4a_frames_from_when_it_was_set() {
    let mut d = connected_dev(|d| d);
    let cs = chunks(&small_blob());
    for c in &cs[..3] {
        assert_finished(
            &d.transact_logo_chunk(c, T).unwrap(),
            c[1],
            "before the fault",
        );
    }
    d.set_logo_fault(LogoFault::RejectAt(1));
    // Other requests do not count.
    assert_eq!(
        ty(&d.transact(&get_data(0x60, Attribute::ADC), T).unwrap()),
        0x41
    );
    assert_finished(
        &d.transact_logo_chunk(&cs[3], T).unwrap(),
        cs[3][1],
        "k = 0",
    );
    assert_eq!(
        ty(&d.transact(&get_data(0x61, Attribute::ADC), T).unwrap()),
        0x41
    );
    assert_reject(
        &d.transact_logo_chunk(&cs[4], T).unwrap(),
        cs[4][1],
        "k = 1",
    );
    assert_finished(
        &d.transact_logo_chunk(&cs[5], T).unwrap(),
        cs[5][1],
        "k = 2",
    );
}

// ---- AC-A2XTES.5: SET_DATA 0x1C ----

#[test]
fn a2xtes_flag_starts_at_0_and_stores_0_and_2() {
    let mut d = connected_dev(|d| d);
    assert_eq!(d.logo_flag(), 0);
    assert_finished(
        &d.transact_logo_flag(&flag_frame(0x11, 2), T).unwrap(),
        0x11,
        "flag 2",
    );
    assert_eq!(d.logo_flag(), 2);
    assert_finished(
        &d.transact_logo_flag(&flag_frame(0x12, 0), T).unwrap(),
        0x12,
        "flag 0",
    );
    assert_eq!(d.logo_flag(), 0);
    let mut d = connected_dev(|d| d.with_logo_flag(2));
    assert_eq!(d.logo_flag(), 2);
    assert_finished(
        &d.transact_logo_flag(&flag_frame(0x13, 0), T).unwrap(),
        0x13,
        "flag 0",
    );
    assert_eq!(d.logo_flag(), 0);
}

#[test]
fn a2xtes_any_other_flag_value_is_rejected_and_changes_nothing() {
    let mut d = connected_dev(|d| d.with_logo_flag(2));
    for (k, v) in [1u32, 3, 4, 0x100, 0x0200_0000, u32::MAX]
        .into_iter()
        .enumerate()
    {
        let id = 0x20 + k as u8;
        // Raw: the guard would refuse these before the meter sees them.
        let reply = d.transact(&flag_frame(id, v), T).unwrap();
        assert_reject(&reply, id, &format!("value {v:#x}"));
        assert_eq!(d.logo_flag(), 2, "value {v:#x}");
    }
}

// The settings side of the 0x1C write (it sets the startup duration and
// leaves block A's CRC stale, FR Z39H24) is in `z39h24_stale_settings.rs`;
// the old "mirror" model is gone.

// ---- AC-A2XTES.6: 0x44 reads of the logo window ----

#[test]
fn a2xtes_logo_window_reads_before_any_write_are_ff() {
    let (mut s, _clock) = connected(Scenario::Idle);
    assert_eq!(s.logo_memory_read(WINDOW, 16), Ok(vec![0xFF; 16]));
    assert_eq!(
        s.logo_memory_read(0x9832_0000 - 256, 256),
        Ok(vec![0xFF; 256])
    );
}

#[test]
fn a2xtes_logo_window_reads_serve_the_flash_model() {
    let (mut s, _clock) = connected(Scenario::Idle);
    let blob = small_blob();
    write_blob(s.transport_mut(), &blob);
    let padded = blob.padded();
    for (k, want) in padded.chunks(256).enumerate() {
        let addr = WINDOW + (k * 256) as u32;
        assert_eq!(
            s.logo_memory_read(addr, 256),
            Ok(want.to_vec()),
            "chunk {k}"
        );
    }
    // An unaligned read inside the window.
    assert_eq!(
        s.logo_memory_read(WINDOW + 0x10, 48),
        Ok(padded[0x10..0x40].to_vec())
    );
    // Past the logo: erased.
    let end = WINDOW + padded.len() as u32;
    assert_eq!(s.logo_memory_read(end, 256), Ok(vec![0xFF; 256]));
}

#[test]
fn a2xtes_logo_window_reads_use_the_confirmation_then_at_most_256_bytes() {
    let mut d = connected_dev(|d| d);
    let blob = small_blob();
    write_blob(&mut d, &blob);
    let padded = blob.padded();
    let id = 0x70;
    let confirm = d
        .transact(&memory_read_request(id, WINDOW, 512), T)
        .unwrap();
    assert_eq!(confirm.len(), 20, "{confirm:02x?}");
    assert_eq!(&confirm[..4], &[0xC4, id, 0x01, 0x01]);
    assert_eq!(&confirm[4..], &memory_read_plaintext(WINDOW, 512)[..16]);
    let data = d.read(T).unwrap();
    assert_eq!(data.len(), 256, "only the first 256 bytes come back");
    assert_eq!(crypto::decrypt(&KEYS[0], &data), padded[..256].to_vec());
    assert_eq!(d.read(T), Err(TError::Timeout), "then nothing");
    // A normal read is served exactly.
    assert_eq!(
        raw_memread(&mut d, 0x71, (WINDOW + 256, 64)),
        padded[256..320].to_vec()
    );
}

// ---- AC-A2XTES.7: boot render on what the meter holds ----

#[test]
fn a2xtes_boot_render_after_upload_flag_and_remove() {
    let d = connected_dev(|d| d.with_logo_erase(ErasePolicy::SectorOnFirstWrite));
    let mut g = GuardedTransport::new(d);
    let blob = small_blob();
    write_blob(&mut g, &blob);
    let on = Duration::from_secs(3);
    assert_eq!(
        boot_render(g.inner().logo_flash(), g.inner().logo_flag(), on),
        BootScreen::BuiltIn,
        "uploaded but flag still 0"
    );
    g.transact_logo_flag(&flag_frame(0x30, 2), T).unwrap();
    let want = logo::decode(&blob.bytes).unwrap();
    assert_eq!(
        boot_render(g.inner().logo_flash(), g.inner().logo_flag(), on),
        BootScreen::Custom(want)
    );
    assert_eq!(
        boot_render(
            g.inner().logo_flash(),
            g.inner().logo_flag(),
            Duration::ZERO
        ),
        BootScreen::Nothing
    );
    g.transact_logo_chunk(&remove_plain(0x31), T).unwrap();
    assert_eq!(
        boot_render(g.inner().logo_flash(), 2, on),
        BootScreen::BuiltIn,
        "flag 2 but the header is erased"
    );
}
