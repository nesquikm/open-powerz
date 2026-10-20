//! Interface selection by class, never by number.

/// One endpoint descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointDesc {
    /// bEndpointAddress (bit 7 set = IN).
    pub address: u8,
    /// bmAttributes (bits 0..1: 0 control, 1 isochronous, 2 bulk, 3 interrupt).
    pub attributes: u8,
    /// wMaxPacketSize.
    pub max_packet_size: u16,
}

/// One interface descriptor with its endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceDesc {
    /// bInterfaceNumber.
    pub number: u8,
    /// bAlternateSetting.
    pub alt_setting: u8,
    /// bInterfaceClass.
    pub class: u8,
    /// bInterfaceSubClass.
    pub subclass: u8,
    /// bInterfaceProtocol.
    pub protocol: u8,
    /// Endpoints of this interface.
    pub endpoints: Vec<EndpointDesc>,
}

/// An interface and its OUT / IN endpoint addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipePair {
    /// bInterfaceNumber.
    pub interface: u8,
    /// OUT endpoint address.
    pub out_ep: u8,
    /// IN endpoint address.
    pub in_ep: u8,
}

/// The interfaces picked from one configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    /// Vendor bulk interface (class 0xFF, one bulk IN + one bulk OUT).
    pub bulk: Option<PipePair>,
    /// HID interface (class 0x03).
    pub hid: Option<PipePair>,
    /// CDC data interface (class 0x0A).
    pub cdc_data: Option<PipePair>,
}

/// Pick the vendor bulk, HID and CDC data interfaces by class.
///
/// When several interfaces qualify for one role, the lowest interface number
/// wins, so the result does not depend on descriptor order.
pub fn select_interfaces(interfaces: &[InterfaceDesc]) -> Selection {
    Selection {
        bulk: pick(interfaces, CLASS_VENDOR, Some(TRANSFER_BULK)),
        hid: pick(interfaces, CLASS_HID, None),
        cdc_data: pick(interfaces, CLASS_CDC_DATA, Some(TRANSFER_BULK)),
    }
}

const CLASS_HID: u8 = 0x03;
const CLASS_CDC_DATA: u8 = 0x0A;
const CLASS_VENDOR: u8 = 0xFF;
const TRANSFER_BULK: u8 = 0x02;

/// Lowest-numbered interface of `class` with exactly one OUT and one IN
/// endpoint (of `transfer` type when given).
fn pick(interfaces: &[InterfaceDesc], class: u8, transfer: Option<u8>) -> Option<PipePair> {
    interfaces
        .iter()
        .filter(|i| i.class == class)
        .filter_map(|i| pipe_pair(i, transfer))
        .min_by_key(|p| p.interface)
}

fn pipe_pair(iface: &InterfaceDesc, transfer: Option<u8>) -> Option<PipePair> {
    let eps: Vec<&EndpointDesc> = iface
        .endpoints
        .iter()
        .filter(|e| transfer.is_none_or(|t| e.attributes & 0x03 == t))
        .collect();
    let mut ins = eps.iter().filter(|e| e.address & 0x80 != 0);
    let mut outs = eps.iter().filter(|e| e.address & 0x80 == 0);
    match (ins.next(), ins.next(), outs.next(), outs.next()) {
        (Some(i), None, Some(o), None) => Some(PipePair {
            interface: iface.number,
            out_ep: o.address,
            in_ep: i.address,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BULK: u8 = 0x02;
    const INTR: u8 = 0x03;

    fn ep(address: u8, attributes: u8, max_packet_size: u16) -> EndpointDesc {
        EndpointDesc {
            address,
            attributes,
            max_packet_size,
        }
    }

    fn iface(number: u8, class: u8, subclass: u8, endpoints: Vec<EndpointDesc>) -> InterfaceDesc {
        InterfaceDesc {
            number,
            alt_setting: 0,
            class,
            subclass,
            protocol: 0,
            endpoints,
        }
    }

    /// KM003C configuration 1 as captured in session 1 (firmware 2.0.6),
    /// descriptor structure only.
    fn km003c() -> Vec<InterfaceDesc> {
        vec![
            iface(0, 0xFF, 0x00, vec![ep(0x01, BULK, 64), ep(0x81, BULK, 64)]),
            iface(1, 0x02, 0x02, vec![ep(0x83, INTR, 8)]),
            iface(2, 0x0A, 0x00, vec![ep(0x02, BULK, 64), ep(0x82, BULK, 64)]),
            iface(3, 0x03, 0x00, vec![ep(0x85, INTR, 64), ep(0x05, INTR, 64)]),
        ]
    }

    /// KM002C-ordered fixture per prior art: bulk IF0, HID IF1, CDC IF2/IF3.
    fn km002c() -> Vec<InterfaceDesc> {
        vec![
            iface(0, 0xFF, 0x00, vec![ep(0x81, BULK, 64), ep(0x01, BULK, 64)]),
            iface(1, 0x03, 0x00, vec![ep(0x82, INTR, 64), ep(0x02, INTR, 64)]),
            iface(2, 0x02, 0x02, vec![ep(0x84, INTR, 8)]),
            iface(3, 0x0A, 0x00, vec![ep(0x03, BULK, 64), ep(0x83, BULK, 64)]),
        ]
    }

    // ---- AC-SJGK7E.2: interface selection by class ----

    #[test]
    fn selects_km003c_interfaces_and_endpoints() {
        let s = select_interfaces(&km003c());
        assert_eq!(
            s.bulk,
            Some(PipePair {
                interface: 0,
                out_ep: 0x01,
                in_ep: 0x81
            })
        );
        assert_eq!(
            s.hid,
            Some(PipePair {
                interface: 3,
                out_ep: 0x05,
                in_ep: 0x85
            })
        );
        assert_eq!(
            s.cdc_data,
            Some(PipePair {
                interface: 2,
                out_ep: 0x02,
                in_ep: 0x82
            })
        );
    }

    #[test]
    fn selects_km002c_ordered_interfaces_and_endpoints() {
        let s = select_interfaces(&km002c());
        assert_eq!(
            s.bulk,
            Some(PipePair {
                interface: 0,
                out_ep: 0x01,
                in_ep: 0x81
            })
        );
        assert_eq!(
            s.hid,
            Some(PipePair {
                interface: 1,
                out_ep: 0x02,
                in_ep: 0x82
            })
        );
        assert_eq!(
            s.cdc_data,
            Some(PipePair {
                interface: 3,
                out_ep: 0x03,
                in_ep: 0x83
            })
        );
    }

    #[test]
    fn selection_does_not_depend_on_interface_order() {
        let mut reversed = km003c();
        reversed.reverse();
        assert_eq!(select_interfaces(&reversed), select_interfaces(&km003c()));
    }

    #[test]
    fn vendor_interface_without_bulk_pair_is_not_bulk() {
        // Class 0xFF but interrupt endpoints only: must not be picked as bulk.
        let ifaces = vec![
            iface(0, 0xFF, 0x00, vec![ep(0x01, INTR, 64), ep(0x81, INTR, 64)]),
            iface(5, 0xFF, 0x00, vec![ep(0x06, BULK, 64), ep(0x86, BULK, 64)]),
        ];
        let s = select_interfaces(&ifaces);
        assert_eq!(
            s.bulk,
            Some(PipePair {
                interface: 5,
                out_ep: 0x06,
                in_ep: 0x86
            })
        );
        assert_eq!(s.hid, None);
        assert_eq!(s.cdc_data, None);
    }

    #[test]
    fn cdc_comm_interface_is_not_cdc_data() {
        let ifaces = vec![iface(1, 0x02, 0x02, vec![ep(0x83, INTR, 8)])];
        assert_eq!(select_interfaces(&ifaces), Selection::default());
    }
}
