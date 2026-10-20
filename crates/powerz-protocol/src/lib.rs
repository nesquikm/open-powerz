//! Unofficial protocol library for the ChargerLAB POWER-Z KM003C / KM002C
//! USB-C power meter and PD analyzer.
//!
//! Transports (vendor bulk, HID, CDC), framing, decoders and session
//! authentication land here milestone by milestone; see `specs/` for the
//! plan. The wire format is documented in `docs/protocol.md`.

pub mod crypto;
pub mod decode;
pub mod frame;
pub mod session;
pub mod transport;

/// USB vendor ID shared by all POWER-Z meters.
pub const VID: u16 = 0x5FC9;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_id_matches_power_z() {
        assert_eq!(VID, 0x5FC9);
    }
}
