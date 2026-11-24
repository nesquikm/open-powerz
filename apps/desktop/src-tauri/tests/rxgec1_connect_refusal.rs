//! FR RXGEC1 (AC .3, .4): the backend's device-list label and connect
//! refusal of a meter on the old bootloader. The mock backend pins the same
//! texts (`mockBackend.rxgec1.test.ts`).
//!
//! Only made-up identifiers are used; no real device is opened.

mod common;

use common::*;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener, list_devices_from};
use open_powerz_desktop_lib::worker::BackendEvent;
use powerz_protocol::transport::{self, discovery::DeviceEntry, discovery::Model};

const REFUSAL: &str = "This meter is on an old (2.1-style) bootloader, which the desktop app cannot connect to or update. Unplug the meter and plug it back in (no buttons) to return to the application firmware. An experimental update, tested only in simulation, is available from the CLI: `powerz firmware flash FILE --i-understand --experimental-old-bootloader`.";

fn legacy_meter() -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: Some("SERIAL-RXGE-0001".to_owned()),
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

/// A refused meter is never opened.
#[derive(Clone)]
struct NeverOpened;

impl Opener for NeverOpened {
    type Transport = SimDevice;
    fn open(&self, _device: &DeviceListing) -> Result<SimDevice, transport::Error> {
        panic!("the meter was opened")
    }
}

#[test]
fn rxgec1_the_backend_labels_the_old_bootloader_meter_experimental_cli_update() {
    let meter = the_meter(&list_devices_from(Ok(vec![legacy_meter()])));
    assert!(
        meter
            .label
            .ends_with(" (old bootloader, experimental CLI update)"),
        "{}",
        meter.label
    );
}

#[test]
fn rxgec1_the_backend_connect_refusal_is_exact() {
    let mut m = DeviceManager::new(|| Ok(vec![legacy_meter()]), NeverOpened, clock());
    let id = the_meter(&m.list_devices()).id;
    let err = m
        .connect(&id, 50, |_e: BackendEvent| {})
        .expect_err("an old-bootloader meter was connected");
    assert_eq!(err.message, REFUSAL);
}
