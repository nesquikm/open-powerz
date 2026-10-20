//! AES-128-ECB with 0xFF padding and IEEE CRC-32, as used by the memory
//! read (0x44) and authentication (0x4C) frames. The four key slots live in
//! [`keys`]. See `docs/protocol.md`.

pub mod keys;

use aes::Aes128;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};

pub use keys::KEYS;

/// AES block size in bytes.
pub const BLOCK_LEN: usize = 16;

/// Pad `data` with 0xFF up to the next multiple of [`BLOCK_LEN`]; data that
/// already fills whole blocks (including empty data) is returned unchanged.
pub fn pad_ff(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    let rem = out.len() % BLOCK_LEN;
    if rem != 0 {
        out.resize(out.len() + BLOCK_LEN - rem, 0xFF);
    }
    out
}

/// AES-128-ECB encryption of `data` under `key`, block by block, after
/// padding the last partial block with 0xFF.
pub fn encrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(&GenericArray::from(*key));
    let mut out = pad_ff(data);
    for block in out.as_chunks_mut::<BLOCK_LEN>().0 {
        cipher.encrypt_block(GenericArray::from_mut_slice(block.as_mut_slice()));
    }
    out
}

/// AES-128-ECB decryption of `data` (whole blocks) under `key`.
/// A trailing partial block, if any, is left as is.
pub fn decrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(&GenericArray::from(*key));
    let mut out = data.to_vec();
    for block in out.as_chunks_mut::<BLOCK_LEN>().0 {
        cipher.decrypt_block(GenericArray::from_mut_slice(block.as_mut_slice()));
    }
    out
}

/// IEEE (zlib) CRC-32 of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        crate::decode::test_util::hex_bytes(s)
    }

    /// Frames of a one-frame-per-line memread fixture (comments skipped).
    fn frames(text: &str) -> Vec<Vec<u8>> {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(hex)
            .collect()
    }

    const MEMREAD_0420: &str = include_str!("../../tests/vectors/memread_0420.hex");
    const MEMREAD_4420: &str = include_str!("../../tests/vectors/memread_4420.hex");

    // ---- AC-N6EHQC.4: AES-128-ECB, 0xFF padding, CRC-32, key slots ----

    #[test]
    fn crypto_aes128_matches_fips197_known_answer() {
        let key: [u8; 16] = hex("000102030405060708090a0b0c0d0e0f").try_into().unwrap();
        let plain = hex("00112233445566778899aabbccddeeff");
        let cipher = hex("69c4e0d86a7b0430d8cdb78070b4c55a");
        assert_eq!(encrypt(&key, &plain), cipher);
        assert_eq!(decrypt(&key, &cipher), plain);
    }

    #[test]
    fn crypto_ecb_encrypts_each_block_independently() {
        let key: [u8; 16] = hex("000102030405060708090a0b0c0d0e0f").try_into().unwrap();
        let plain = hex("00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff");
        let out = encrypt(&key, &plain);
        assert_eq!(out.len(), 32);
        assert_eq!(&out[..16], &hex("69c4e0d86a7b0430d8cdb78070b4c55a")[..]);
        assert_eq!(&out[16..], &out[..16]);
        assert_eq!(decrypt(&key, &out), plain);
    }

    #[test]
    fn crypto_pads_last_partial_block_with_ff() {
        assert_eq!(pad_ff(&[]), Vec::<u8>::new());
        let mut want = vec![1, 2, 3];
        want.extend([0xFF; 13]);
        assert_eq!(pad_ff(&[1, 2, 3]), want);
        assert_eq!(pad_ff(&[7; 16]), vec![7; 16]);
        let mut want = vec![9; 20];
        want.extend([0xFF; 12]);
        assert_eq!(pad_ff(&[9; 20]), want);
    }

    #[test]
    fn crypto_encrypt_pads_with_ff_before_encrypting() {
        let key = KEYS[0];
        let short = [0x11, 0x22, 0x33];
        let mut padded = short.to_vec();
        padded.extend([0xFF; 13]);
        let out = encrypt(&key, &short);
        assert_eq!(out.len(), 16);
        assert_eq!(out, encrypt(&key, &padded));
        assert_eq!(decrypt(&key, &out), padded);
        assert_eq!(encrypt(&key, &[]), Vec::<u8>::new());
    }

    #[test]
    fn crypto_crc32_is_ieee() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(&[]), 0);
    }

    #[test]
    fn crypto_has_four_distinct_key_slots_documented_as_static() {
        assert_eq!(KEYS.len(), 4);
        for (i, a) in KEYS.iter().enumerate() {
            assert_ne!(*a, [0u8; 16], "slot {i} is all zero");
            for b in &KEYS[i + 1..] {
                assert_ne!(a, b, "two key slots are equal");
            }
        }
        let src = include_str!("keys.rs");
        assert!(
            src.contains("[static]"),
            "keys.rs must tag its source [static]"
        );
        assert!(
            src.contains("V2.5.2"),
            "keys.rs must name the vendor app version"
        );
    }

    #[test]
    fn crypto_slot0_reproduces_captured_memread_request() {
        let f = frames(MEMREAD_0420);
        let request = &f[0];
        assert_eq!(&request[..4], &[0x44, 0x02, 0x01, 0x01]);
        let plain = decrypt(&KEYS[0], &request[4..]);
        let mut want = Vec::new();
        want.extend(0x420u32.to_le_bytes());
        want.extend(64u32.to_le_bytes());
        want.extend(0xFFFF_FFFFu32.to_le_bytes());
        let crc = crc32(&want);
        want.extend(crc.to_le_bytes());
        want.extend([0xFF; 16]);
        assert_eq!(plain, want);
        assert_eq!(encrypt(&KEYS[0], &want), request[4..].to_vec());
    }

    #[test]
    fn crypto_slot0_decrypts_captured_info_blocks() {
        let data = &frames(MEMREAD_0420)[2];
        let block = decrypt(&KEYS[0], data);
        assert_eq!(&block[0x10..0x16], b"KM003C");
        assert_eq!(&block[0x1C..0x1F], b"2.5");
        let data = &frames(MEMREAD_4420)[2];
        let block = decrypt(&KEYS[0], data);
        assert_eq!(&block[0..4], &0x4000u32.to_le_bytes());
        assert_eq!(&block[0x1C..0x21], b"2.0.6");
    }
}
