//! FR JT38C8 (AC .5) on the backend side: a meter on the old bootloader
//! (2.1: one vendor interface named after the product, no HID) is listed as
//! `(old bootloader, not supported yet)`, sent to the UI as
//! `legacy_bootloader`, and Connect refuses it before opening anything,
//! saying to unplug and replug it to return to the application firmware.
//!
//! Only made-up identifiers are used; no real device is opened.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener, list_devices_from};
use open_powerz_desktop_lib::ui_error::ErrorKind;
use open_powerz_desktop_lib::worker::BackendEvent;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::transport::{self, discovery::DeviceEntry, discovery::Model};

const SERIAL: &str = "SERIAL-JT38-0001";
const SUFFIX: &str = "(old bootloader, not supported yet)";

/// The field-reported 2.1 bootloader as the OS lists it.
fn legacy_meter() -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: Some(SERIAL.to_owned()),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some("POWER-Z KM003C".to_owned()),
        vendor_subclass: Some(0xFF),
        has_hid: false,
        bus: "1".to_owned(),
        address: 9,
    }
}

fn the_meter(list: &[DeviceListing]) -> DeviceListing {
    list.iter()
        .find(|d| d.demo_scenario.is_none())
        .cloned()
        .unwrap_or_else(|| panic!("the meter is not listed: {list:#?}"))
}

#[test]
fn jt38c8_the_old_bootloader_meter_is_labelled_as_such() {
    let meter = the_meter(&list_devices_from(Ok(vec![legacy_meter()])));
    assert!(meter.label.ends_with(SUFFIX), "{}", meter.label);
    assert!(
        !meter.label.contains("(bootloader mode)"),
        "{}",
        meter.label
    );
}

#[test]
fn jt38c8_the_old_bootloader_meter_is_sent_to_the_ui_as_legacy_bootloader() {
    let meter = the_meter(&list_devices_from(Ok(vec![legacy_meter()])));
    assert_eq!(meter.mode, DeviceMode::LegacyBootloader);
    let v = serde_json::to_value(&meter).unwrap();
    assert_eq!(v["mode"], "legacy_bootloader", "{v}");
}

/// Opener that counts its calls.
#[derive(Clone)]
struct Counting {
    clock: SimClock,
    opens: Arc<AtomicUsize>,
}

impl Opener for Counting {
    type Transport = SimDevice;
    fn open(&self, _device: &DeviceListing) -> Result<SimDevice, transport::Error> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(SimDevice::with_clock(
            Persona::Bulk,
            Scenario::Idle,
            self.clock.clone(),
        ))
    }
}

#[test]
fn jt38c8_connect_refuses_the_old_bootloader_before_opening_anything() {
    let clock = clock();
    let opens = Arc::new(AtomicUsize::new(0));
    let opener = Counting {
        clock: clock.clone(),
        opens: Arc::clone(&opens),
    };
    let mut m = DeviceManager::new(|| Ok(vec![legacy_meter()]), opener, clock);
    let id = the_meter(&m.list_devices()).id;
    let err = m
        .connect(&id, 50, |_e: BackendEvent| {})
        .expect_err("an old-bootloader meter was connected");
    assert_eq!(err.kind, ErrorKind::Usage);
    assert_eq!(opens.load(Ordering::SeqCst), 0, "the meter was opened");
    let text = format!("{} {}", err.message, err.hint.unwrap_or_default()).to_lowercase();
    assert!(
        text.contains("unplug") && text.contains("replug"),
        "says to unplug and replug: {text}"
    );
    assert!(
        text.contains("application firmware"),
        "names the application firmware: {text}"
    );
}
