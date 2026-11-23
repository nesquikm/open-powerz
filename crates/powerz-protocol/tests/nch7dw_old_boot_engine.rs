//! FR NCH7DW in the flash engine: the after-switch error.
//!
//! - AC .2: the error for a meter that switched to the old bootloader
//!   without the opt-in says it is now in its old bootloader, that the
//!   experimental flag can retry it while it stays there, and that a
//!   replug returns it to the application; vendor code -110.
//!
//! AC .6 is in `nch7dw_head_strays.rs`, which uses no API the fix adds.

use powerz_protocol::firmware::flash::FlashError;

/// The flag the message must name.
const FLAG: &str = "--experimental-old-bootloader";

// ---------------------------------------------------------------- AC-NCH7DW.2

#[test]
fn nch7dw_switched_to_old_boot_says_the_meter_is_now_in_its_old_bootloader() {
    let msg = FlashError::SwitchedToLegacyBootloader
        .to_string()
        .to_lowercase();
    assert!(msg.contains("now in its old"), "{msg}");
}

#[test]
fn nch7dw_switched_to_old_boot_names_the_experimental_flag_for_a_retry() {
    let msg = FlashError::SwitchedToLegacyBootloader.to_string();
    assert!(
        msg.contains(FLAG),
        "the message does not name {FLAG}: {msg}"
    );
    assert!(
        msg.to_lowercase().contains("retr") || msg.to_lowercase().contains("again"),
        "the message does not say it can be retried: {msg}"
    );
}

#[test]
fn nch7dw_switched_to_old_boot_says_a_replug_returns_the_application() {
    let msg = FlashError::SwitchedToLegacyBootloader
        .to_string()
        .to_lowercase();
    assert!(msg.contains("unplug"), "no replug: {msg}");
    assert!(msg.contains("application"), "no application: {msg}");
}

#[test]
fn nch7dw_switched_to_old_boot_differs_from_the_refusal_at_start() {
    // The refusal at start asks for the flag; after the switch the text must
    // say what happened, not repeat it.
    assert_ne!(
        FlashError::SwitchedToLegacyBootloader.to_string(),
        FlashError::LegacyBootloader.to_string()
    );
}

#[test]
fn nch7dw_switched_to_old_boot_has_vendor_code_minus_110() {
    assert_eq!(FlashError::SwitchedToLegacyBootloader.code(), -110);
}
