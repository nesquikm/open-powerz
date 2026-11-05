//! USB vendor names for a cable's e-marker VID (FR AXHK2P).
//!
//! The table is our own, a small set of well-known assignments written from
//! the public USB-IF vendor ID list. It comes from no vendor software.
//! Anything else has no name and shows as its hex VID, which is the normal
//! case for a cable.

/// `(VID, name)`, sorted by VID for the binary search.
const VENDORS: &[(u16, &str)] = &[
    (0x03F0, "HP"),
    (0x045E, "Microsoft"),
    (0x04E8, "Samsung"),
    (0x050D, "Belkin"),
    (0x05AC, "Apple"),
    (0x05E3, "Genesys Logic"),
    (0x0BDA, "Realtek"),
    (0x17EF, "Lenovo"),
    (0x18D1, "Google"),
    (0x2109, "VIA Labs"),
    (0x291A, "Anker"),
    (0x413C, "Dell"),
    (0x8087, "Intel"),
];

/// The name of USB vendor `vid`; `None` when our table does not list it.
pub fn vendor_name(vid: u16) -> Option<&'static str> {
    VENDORS
        .binary_search_by_key(&vid, |&(id, _)| id)
        .ok()
        .and_then(|i| VENDORS.get(i))
        .map(|&(_, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_stays_sorted_for_the_binary_search() {
        assert!(VENDORS.windows(2).all(|w| w[0].0 < w[1].0));
    }
}
