//! AC-AXHK2M.17 and .18 on the backend side: a listed meter whose vendor
//! interface string turns from `APP` to `BOOT` (the real bootloader's string)
//! or back, at the same USB address or a new one, is followed by the next
//! hotplug poll, and app.log gets one line per mode change naming the model,
//! the USB address and the old and new mode, never the serial.
//!
//! Only made-up identifiers are used; no real device is opened.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::*;
use open_powerz_desktop_lib::app_log::AppLog;
use open_powerz_desktop_lib::devices::{
    DeviceListing, DeviceManager, HOTPLUG_PERIOD_MS, Hotplug, Opener,
};
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::transport::{self, discovery::DeviceEntry, discovery::Model};

const SERIAL: &str = "SERIAL-MODE-0001";
const OTHER_SERIAL: &str = "SERIAL-MODE-0002";
const SUFFIX: &str = "(bootloader mode)";

fn entry(serial: &str, interface: &str, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: Some(serial.to_owned()),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some(interface.to_owned()),
        bus: "1".to_owned(),
        address,
    }
}

fn the_meter(list: &[DeviceListing], serial: &str) -> DeviceListing {
    list.iter()
        .find(|d| d.demo_scenario.is_none() && d.serial.as_deref() == Some(serial))
        .cloned()
        .unwrap_or_else(|| panic!("{serial} is not listed: {list:#?}"))
}

/// Polls `hp` every 100 ms from `from_ms` (the moment the OS list changed)
/// until it reports a list where the meter is in `mode`; that list and the
/// time it took.
fn follow(
    hp: &mut Hotplug,
    from_ms: u64,
    attached: &Arc<Mutex<Vec<DeviceEntry>>>,
    mode: DeviceMode,
) -> (DeviceListing, u64) {
    let mut t = from_ms;
    while t <= from_ms + 5_000 {
        let a = Arc::clone(attached);
        if let Some(list) = hp.tick(t, move || Ok(a.lock().unwrap().clone())) {
            let meter = the_meter(&list, SERIAL);
            if meter.mode == mode {
                return (meter, t - from_ms);
            }
        }
        t += 100;
    }
    panic!("hotplug never reported the meter in {mode:?}");
}

fn assert_flips(at_new_address: bool) {
    let attached = Arc::new(Mutex::new(vec![entry(SERIAL, "APP", 7)]));
    let mut hp = Hotplug::new();
    let a = Arc::clone(&attached);
    let first = hp
        .tick(0, move || Ok(a.lock().unwrap().clone()))
        .expect("the first tick lists");
    let app = the_meter(&first, SERIAL);
    assert_eq!(app.mode, DeviceMode::Application);
    assert!(!app.label.contains(SUFFIX), "{}", app.label);

    // Into the bootloader: "BOOT", as the real meter reports it.
    let boot_address = if at_new_address { 8 } else { 7 };
    let changed_at = 1_230;
    *attached.lock().unwrap() = vec![entry(SERIAL, "BOOT", boot_address)];
    let (boot, took) = follow(&mut hp, changed_at, &attached, DeviceMode::Bootloader);
    assert!(
        took <= 1_000,
        "bootloader mode listed {took} ms after the change"
    );
    assert!(took <= HOTPLUG_PERIOD_MS, "not the next poll: {took} ms");
    assert!(boot.label.ends_with(SUFFIX), "{}", boot.label);
    assert_eq!(boot.address, Some(boot_address));
    assert_eq!(boot.id, format!("usb:1:{boot_address}"));

    // And back to the application.
    let back_address = if at_new_address { 9 } else { 7 };
    let back_at = changed_at + 3_000;
    *attached.lock().unwrap() = vec![entry(SERIAL, "APP", back_address)];
    let (back, took) = follow(&mut hp, back_at, &attached, DeviceMode::Application);
    assert!(
        took <= 1_000,
        "application mode listed {took} ms after the change"
    );
    assert!(!back.label.contains(SUFFIX), "{}", back.label);
    assert_eq!(back.address, Some(back_address));
}

#[test]
fn ac_axhk2m_17_hotplug_follows_app_to_boot_and_back_at_the_same_address() {
    assert_flips(false);
}

#[test]
fn ac_axhk2m_17_hotplug_follows_app_to_boot_and_back_at_a_new_address() {
    assert_flips(true);
}

// ---------------------------------------------------------------- AC-AXHK2M.18

#[derive(Clone)]
struct NoOpen;

impl Opener for NoOpen {
    type Transport = SimDevice;
    fn open(&self, _device: &DeviceListing) -> Result<SimDevice, transport::Error> {
        panic!("a mode change must not open the meter");
    }
}

fn log_path(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("open-powerz-modelog-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("app.log")
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The app.log lines that tell a mode change: both mode names in one line.
fn mode_lines(path: &Path) -> Vec<String> {
    lines(path)
        .into_iter()
        .filter(|l| {
            let l = l.to_lowercase();
            l.contains("application") && l.contains("bootloader")
        })
        .collect()
}

/// The listing the hotplug watcher hands the manager for `entries`.
fn listed(entries: Vec<DeviceEntry>) -> Vec<DeviceListing> {
    let mut hp = Hotplug::new();
    hp.tick(0, move || Ok(entries))
        .expect("the first tick lists")
}

fn manager(path: &Path) -> DeviceManager<NoOpen, SimClock> {
    let mut m = DeviceManager::new(|| Ok(Vec::new()), NoOpen, clock());
    m.set_app_log(AppLog::open(path).expect("open app.log"));
    m
}

fn names_address(line: &str, address: u8) -> bool {
    line.contains(&format!("usb:1:{address}")) || line.contains(&format!("address {address}"))
}

fn assert_says_change(line: &str, address: u8, old: &str, new: &str) {
    assert!(line.contains("KM003C"), "no model: {line}");
    assert!(
        names_address(line, address),
        "no USB address {address}: {line}"
    );
    let lower = line.to_lowercase();
    let (o, n) = (
        lower.find(old).expect("old mode named"),
        lower.rfind(new).expect("new mode named"),
    );
    assert!(
        o < n,
        "old mode {old} must come before new mode {new}: {line}"
    );
    assert!(!line.contains(SERIAL), "the serial is logged: {line}");
    assert!(!line.contains(OTHER_SERIAL), "a serial is logged: {line}");
}

#[test]
fn ac_axhk2m_18_app_log_has_one_line_per_mode_change_without_the_serial() {
    let path = log_path("same-address");
    let mut m = manager(&path);
    let steady = entry(OTHER_SERIAL, "APP", 3);

    // First seen: no change yet, nothing to log.
    m.devices_changed(&listed(vec![entry(SERIAL, "APP", 7), steady.clone()]));
    assert!(mode_lines(&path).is_empty(), "{:#?}", lines(&path));

    // Into the bootloader at the same address: one line, for that meter only.
    let boot = listed(vec![entry(SERIAL, "BOOT", 7), steady.clone()]);
    m.devices_changed(&boot);
    // The watcher re-reports an unchanged list while it waits: no new line.
    m.devices_changed(&boot);
    let after_boot = mode_lines(&path);
    assert_eq!(after_boot.len(), 1, "{after_boot:#?}");
    assert_says_change(&after_boot[0], 7, "application", "bootloader");
    assert!(
        !names_address(&after_boot[0], 3),
        "the steady meter is logged: {}",
        after_boot[0]
    );

    // Back in the application: a second line, the other way round.
    m.devices_changed(&listed(vec![entry(SERIAL, "APP", 7), steady]));
    let after_back = mode_lines(&path);
    assert_eq!(after_back.len(), 2, "{after_back:#?}");
    assert_says_change(&after_back[1], 7, "bootloader", "application");

    let all = lines(&path).join("\n");
    assert!(!all.contains(SERIAL), "the serial is in app.log: {all}");
    assert!(!all.contains(OTHER_SERIAL), "a serial is in app.log: {all}");
}

#[test]
fn ac_axhk2m_18_a_mode_change_at_a_new_address_is_logged_once_with_that_address() {
    let path = log_path("new-address");
    let mut m = manager(&path);
    m.devices_changed(&listed(vec![entry(SERIAL, "APP", 7)]));
    m.devices_changed(&listed(vec![entry(SERIAL, "BOOT", 8)]));
    let changes = mode_lines(&path);
    assert_eq!(changes.len(), 1, "{:#?}", lines(&path));
    assert_says_change(&changes[0], 8, "application", "bootloader");
}
