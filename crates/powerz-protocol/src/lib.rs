//! Unofficial protocol library for the ChargerLAB POWER-Z KM003C / KM002C
//! USB-C power meter and PD analyzer.
//!
//! Transports (vendor bulk, HID, CDC), framing, decoders and session
//! authentication land here milestone by milestone; see `specs/` for the
//! plan. The wire format is documented in `docs/protocol.md`.

pub mod crypto;
pub mod decode;
pub mod frame;
pub mod pdcap;
pub mod session;
pub mod transport;

/// USB PD r3.2 message decoder (the `powerz-pd` crate).
pub use powerz_pd as pd;

/// USB vendor ID shared by all POWER-Z meters.
pub const VID: u16 = 0x5FC9;

/// Version of this protocol library (its `Cargo.toml` version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_id_matches_power_z() {
        assert_eq!(VID, 0x5FC9);
    }
}
