//! Device discovery: list attached POWER-Z meters without opening them.

use super::Error;
use crate::firmware::DeviceMode;

/// USB product ID of the KM003C.
pub const PID_KM003C: u16 = 0x0063;
/// USB product ID of the KM002C.
pub const PID_KM002C: u16 = 0x0061;

/// Meter model, from the USB product ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Model {
    /// POWER-Z KM003C (PID 0x0063).
    Km003c,
    /// POWER-Z KM002C (PID 0x0061).
    Km002c,
    /// Any other product ID under the POWER-Z vendor ID.
    Unknown(u16),
}

impl Model {
    /// Map a product ID to a model.
    pub fn from_pid(pid: u16) -> Model {
        match pid {
            PID_KM003C => Model::Km003c,
            PID_KM002C => Model::Km002c,
            other => Model::Unknown(other),
        }
    }
}

/// Descriptor-level facts about one attached USB device (no handle held).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbDeviceInfo {
    /// idVendor.
    pub vid: u16,
    /// idProduct.
    pub pid: u16,
    /// iSerialNumber string, if the OS reported one.
    pub serial: Option<String>,
    /// iProduct string, if the OS reported one (the model name).
    pub product: Option<String>,
    /// Interface string of the vendor-class (0xFF) interface, if the OS
    /// reported one (`APP` in application mode `[device]`).
    pub interface_string: Option<String>,
    /// Bus identifier as reported by the OS.
    pub bus: String,
    /// Device address on the bus.
    pub address: u8,
}

/// One discovered POWER-Z meter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEntry {
    /// Model from the product ID.
    pub model: Model,
    /// Raw product ID.
    pub pid: u16,
    /// Serial string.
    pub serial: Option<String>,
    /// USB product string, if the OS reported one.
    pub product: Option<String>,
    /// Vendor-interface string, if the OS reported one; decides the mode.
    pub interface_string: Option<String>,
    /// Bus identifier.
    pub bus: String,
    /// Device address on the bus.
    pub address: u8,
}

impl DeviceEntry {
    /// Application or bootloader mode, from the vendor-interface string.
    pub fn mode(&self) -> DeviceMode {
        DeviceMode::from_interface_string(self.interface_string.as_deref())
    }
}

/// Keep every device with the POWER-Z vendor ID, in input order.
pub fn filter_devices(devices: &[UsbDeviceInfo]) -> Vec<DeviceEntry> {
    devices
        .iter()
        .filter(|d| d.vid == crate::VID)
        .map(|d| DeviceEntry {
            model: Model::from_pid(d.pid),
            pid: d.pid,
            serial: d.serial.clone(),
            product: d.product.clone(),
            interface_string: d.interface_string.clone(),
            bus: d.bus.clone(),
            address: d.address,
        })
        .collect()
}

/// List attached meters from the OS device list (never opens or claims).
pub fn list_devices() -> Result<Vec<DeviceEntry>, Error> {
    use nusb::MaybeFuture;
    let infos: Vec<UsbDeviceInfo> = nusb::list_devices()
        .wait()
        .map_err(|e| Error::Io(e.to_string()))?
        .map(|d| UsbDeviceInfo {
            vid: d.vendor_id(),
            pid: d.product_id(),
            serial: d.serial_number().map(str::to_owned),
            product: d.product_string().map(str::to_owned),
            interface_string: vendor_interface_string(&d),
            bus: d.bus_id().to_owned(),
            address: d.device_address(),
        })
        .collect();
    Ok(filter_devices(&infos))
}

/// Interface string of the vendor-class (0xFF) interface, picked by class,
/// never by number; read from the OS device list, nothing is opened.
fn vendor_interface_string(d: &nusb::DeviceInfo) -> Option<String> {
    d.interfaces()
        .filter(|i| i.class() == 0xFF)
        .find_map(|i| i.interface_string())
        .map(str::to_owned)
}

/// One USB interface as the OS describes it (no handle held).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbInterface {
    /// bInterfaceNumber.
    pub number: u8,
    /// bInterfaceClass (0xFF vendor, 0x02 CDC, 0x0A CDC data, 0x03 HID).
    pub class: u8,
    /// bInterfaceSubClass.
    pub subclass: u8,
    /// bInterfaceProtocol.
    pub protocol: u8,
    /// iInterface string, if the OS reported one.
    pub string: Option<String>,
}

/// The USB identity of one meter: IDs, product string and every interface,
/// read from descriptors without opening the device (FR V9SK56, AC .6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbIdentity {
    /// idVendor.
    pub vid: u16,
    /// idProduct.
    pub pid: u16,
    /// iProduct string, if the OS reported one.
    pub product: Option<String>,
    /// The interfaces, in descriptor order.
    pub interfaces: Vec<UsbInterface>,
}

impl UsbIdentity {
    /// Application or bootloader mode, from the string of the vendor-class
    /// (0xFF) interface, found by class, never by number.
    pub fn mode(&self) -> DeviceMode {
        DeviceMode::from_interface_string(self.vendor_interface().and_then(|i| i.string.as_deref()))
    }

    /// The first vendor-class (0xFF) interface, if any: the bulk pipe.
    pub fn vendor_interface(&self) -> Option<&UsbInterface> {
        self.interfaces.iter().find(|i| i.class == 0xFF)
    }
}

impl std::fmt::Display for UsbIdentity {
    /// Labelled lines: vendor and product IDs as `0x` hex, the product
    /// string, then one `interface <n>` line per interface.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Vendor ID:   {:#06x}", self.vid)?;
        writeln!(f, "Product ID:  {:#06x}", self.pid)?;
        writeln!(
            f,
            "Product:     {}",
            self.product.as_deref().unwrap_or("(no string)")
        )?;
        writeln!(f, "Mode:        {}", self.mode().name())?;
        for i in &self.interfaces {
            let string = i
                .string
                .as_deref()
                .map_or_else(|| "(no string)".to_owned(), |s| format!("{s:?}"));
            writeln!(
                f,
                "  interface {}: class {:#04x}, subclass {:#04x}, protocol {:#04x}, string {string}",
                i.number, i.class, i.subclass, i.protocol
            )?;
        }
        Ok(())
    }
}

/// Descriptor-level facts about one attached USB device, interfaces
/// included, as the OS device list reports them (no handle held).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbDescriptor {
    /// idVendor.
    pub vid: u16,
    /// idProduct.
    pub pid: u16,
    /// iSerialNumber string, if the OS reported one.
    pub serial: Option<String>,
    /// iProduct string, if the OS reported one.
    pub product: Option<String>,
    /// Bus identifier as reported by the OS.
    pub bus: String,
    /// Device address on the bus.
    pub address: u8,
    /// The interfaces, in descriptor order.
    pub interfaces: Vec<UsbInterface>,
}

impl UsbIdentity {
    /// The identity part of `d`: IDs, product string and every interface,
    /// in descriptor order.
    pub fn from_descriptor(d: &UsbDescriptor) -> UsbIdentity {
        UsbIdentity {
            vid: d.vid,
            pid: d.pid,
            product: d.product.clone(),
            interfaces: d.interfaces.clone(),
        }
    }
}

/// The descriptor in `devices` that is the meter `device`: the POWER-Z
/// vendor ID, the same product ID, bus and address, and the same serial
/// when `device` has one. `None` when no descriptor matches.
pub fn match_device<'a>(
    device: &DeviceEntry,
    devices: &'a [UsbDescriptor],
) -> Option<&'a UsbDescriptor> {
    devices.iter().find(|d| {
        d.vid == crate::VID
            && d.pid == device.pid
            && d.bus == device.bus
            && d.address == device.address
            && device
                .serial
                .as_ref()
                .is_none_or(|s| d.serial.as_ref() == Some(s))
    })
}

/// Two or more POWER-Z devices are attached and none has the flashed
/// meter's serial, so the flash cannot tell which one to talk to.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "several POWER-Z devices attached and none is the flashed meter; unplug the others: {}",
    .serials.join(", ")
)]
pub struct AmbiguousMeter {
    /// Each candidate's serial, `-` for one without.
    pub serials: Vec<String>,
}

/// The meter a flash talks to, from the device list as it is now: the one
/// with `serial`, else the only POWER-Z attached, whatever its serial
/// (`[inferred]` the bootloader may enumerate with another serial or none).
/// `None` while no meter is attached; two or more without that serial are
/// [`AmbiguousMeter`].
pub fn find_flashed_meter<'a>(
    devices: &'a [DeviceEntry],
    serial: Option<&str>,
) -> Result<Option<&'a DeviceEntry>, AmbiguousMeter> {
    if let Some(serial) = serial
        && let Some(d) = devices.iter().find(|d| d.serial.as_deref() == Some(serial))
    {
        return Ok(Some(d));
    }
    match devices {
        [] => Ok(None),
        [only] => Ok(Some(only)),
        several => Err(AmbiguousMeter {
            serials: several
                .iter()
                .map(|d| d.serial.clone().unwrap_or_else(|| "-".to_owned()))
                .collect(),
        }),
    }
}

/// The USB identity of the attached meter `device` (matched by
/// [`match_device`]), read from the OS device list; nothing is opened or
/// claimed. `None` when the meter is no longer listed.
pub fn usb_identity(device: &DeviceEntry) -> Result<Option<UsbIdentity>, Error> {
    use nusb::MaybeFuture;
    let descriptors: Vec<UsbDescriptor> = nusb::list_devices()
        .wait()
        .map_err(|e| Error::Io(e.to_string()))?
        .map(|d| UsbDescriptor {
            vid: d.vendor_id(),
            pid: d.product_id(),
            serial: d.serial_number().map(str::to_owned),
            product: d.product_string().map(str::to_owned),
            bus: d.bus_id().to_owned(),
            address: d.device_address(),
            interfaces: d
                .interfaces()
                .map(|i| UsbInterface {
                    number: i.interface_number(),
                    class: i.class(),
                    subclass: i.subclass(),
                    protocol: i.protocol(),
                    string: i.interface_string().map(str::to_owned),
                })
                .collect(),
        })
        .collect();
    Ok(match_device(device, &descriptors).map(UsbIdentity::from_descriptor))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLACEHOLDER_SERIAL: &str = "SERIAL-0001";

    fn dev(vid: u16, pid: u16, serial: Option<&str>, bus: &str, address: u8) -> UsbDeviceInfo {
        UsbDeviceInfo {
            vid,
            pid,
            serial: serial.map(str::to_owned),
            product: None,
            interface_string: None,
            bus: bus.to_owned(),
            address,
        }
    }

    // ---- AC-SJGK7E.1: discovery over injected descriptors ----

    #[test]
    fn model_from_pid_maps_known_and_unknown() {
        assert_eq!(Model::from_pid(0x0063), Model::Km003c);
        assert_eq!(Model::from_pid(0x0061), Model::Km002c);
        assert_eq!(Model::from_pid(0x0042), Model::Unknown(0x0042));
    }

    #[test]
    fn filter_devices_keeps_every_power_z_device_with_serial_and_bus_address() {
        let devices = [
            dev(0x05AC, 0x8262, Some("other"), "1", 2),
            dev(0x5FC9, 0x0063, Some(PLACEHOLDER_SERIAL), "1", 7),
            dev(0x5FC9, 0x0061, Some("SERIAL-0002"), "2", 3),
            dev(0x1234, 0x0063, None, "2", 4),
            dev(0x5FC9, 0x00AA, None, "3", 9),
        ];
        let found = filter_devices(&devices);
        assert_eq!(
            found,
            vec![
                DeviceEntry {
                    model: Model::Km003c,
                    pid: 0x0063,
                    serial: Some(PLACEHOLDER_SERIAL.to_owned()),
                    product: None,
                    interface_string: None,
                    bus: "1".to_owned(),
                    address: 7,
                },
                DeviceEntry {
                    model: Model::Km002c,
                    pid: 0x0061,
                    serial: Some("SERIAL-0002".to_owned()),
                    product: None,
                    interface_string: None,
                    bus: "2".to_owned(),
                    address: 3,
                },
                DeviceEntry {
                    model: Model::Unknown(0x00AA),
                    pid: 0x00AA,
                    serial: None,
                    product: None,
                    interface_string: None,
                    bus: "3".to_owned(),
                    address: 9,
                },
            ]
        );
    }

    #[test]
    fn filter_devices_with_no_power_z_device_is_empty() {
        let devices = [dev(0x05AC, 0x8262, None, "1", 2)];
        assert_eq!(filter_devices(&devices), Vec::new());
    }
}
