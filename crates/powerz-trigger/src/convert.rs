//! Mapping between the UI's shapes and the protocol library's.

use powerz_protocol::pdm::legacy::{self, LegacyProtocol};
use powerz_protocol::pdm::message::{CableIdentity, PdMessage, SopTarget};
use powerz_protocol::pdm::{self, console, pd};

use crate::error::TriggerError;
use crate::types::{
    CableView, ConsoleDirection, ConsoleLine, LimitsView, MessageSop, OfferView, PdMessageRequest,
    ProtocolSection, RiskClass, TriggerProtocol, TriggerRequest,
};

/// A library console entry as the UI's line; a block is described in words.
pub(crate) fn console_line(e: &console::ConsoleEntry) -> ConsoleLine {
    ConsoleLine {
        at_ms: e.at_ms,
        direction: match e.direction {
            console::Direction::Tx => ConsoleDirection::Tx,
            console::Direction::Rx => ConsoleDirection::Rx,
        },
        text: e.text.clone(),
        block: e.block.as_ref().map(console::describe_block),
    }
}

/// The one message the app may send a cable with no dialog: SOP', header
/// 0x108F, object 0xFF00A801 (both little endian). It cannot raise VBUS.
pub(crate) const CABLE_READ: [u8; 7] = [0x01, 0x8F, 0x10, 0x01, 0xA8, 0x00, 0xFF];

/// Exactly that message, parsed. Raw console text never matches, so the
/// console keeps asking for the same bytes.
pub fn is_cable_read(command: &pdm::Command) -> bool {
    matches!(command, pdm::Command::PdData(bytes) if bytes.as_slice() == CABLE_READ)
}

/// The library's cable identity as the UI's.
pub fn cable_view(c: &CableIdentity) -> CableView {
    CableView {
        vid: c.vid,
        pid: c.pid,
        active: c.active,
        max_voltage_v: c.max_voltage_v,
        max_current_ma: c.max_current_ma,
        epr_capable: c.epr_capable,
        speed: c.speed.map(|s| s.name()),
        vendor: pdm::vendors::vendor_name(c.vid).map(str::to_owned),
    }
}

/// A UI message as the library's.
pub fn lib_message(m: &PdMessageRequest) -> PdMessage {
    let sop = match m.sop {
        MessageSop::Sop => SopTarget::Sop,
        MessageSop::SopPrime => SopTarget::SopPrime,
        MessageSop::SopDoublePrime => SopTarget::SopDoublePrime,
    };
    PdMessage {
        sop,
        msg_type: m.msg_type,
        data_objects: m.data_objects.clone(),
    }
}

/// A UI request as a library request.
pub(crate) enum LibRequest {
    Pd(pd::Request),
    Legacy(legacy::Request),
}

/// Map a UI request to the library's; a PPS / AVS request needs both a
/// voltage and a current.
pub(crate) fn lib_request(request: TriggerRequest) -> Result<LibRequest, TriggerError> {
    use legacy::Request as L;
    use pdm::FixedProtocol as F;
    let fixed = |protocol, volts| LibRequest::Legacy(L::Fixed { protocol, volts });
    Ok(match request {
        TriggerRequest::Pd {
            position,
            voltage_mv: None,
            current_ma,
        } => LibRequest::Pd(pd::Request::Fixed {
            position,
            current_ma,
        }),
        TriggerRequest::Pd {
            position,
            voltage_mv: Some(voltage_mv),
            current_ma: Some(current_ma),
        } => LibRequest::Pd(pd::Request::Programmable {
            position,
            voltage_mv,
            current_ma,
        }),
        TriggerRequest::Pd {
            voltage_mv: Some(_),
            current_ma: None,
            ..
        } => {
            return Err(TriggerError::usage(
                "A PPS or AVS request needs a voltage and a current.",
            ));
        }
        TriggerRequest::Qc { volts } => fixed(F::Qc, volts),
        TriggerRequest::Fcp { volts } => fixed(F::Fcp, volts),
        TriggerRequest::Afc { volts } => fixed(F::Afc, volts),
        TriggerRequest::Sfcp { volts } => fixed(F::Sfcp, volts),
        TriggerRequest::Qc3 { voltage_mv } => LibRequest::Legacy(L::Qc3Volt { voltage_mv }),
        TriggerRequest::Scp {
            voltage_mv,
            current_ma,
        } => LibRequest::Legacy(L::Scp {
            voltage_mv,
            current_ma,
        }),
        TriggerRequest::Vfcp {
            voltage_mv,
            current_ma,
        } => LibRequest::Legacy(L::Vfcp {
            voltage_mv,
            current_ma,
        }),
        TriggerRequest::Tfcp {
            voltage_mv,
            current_ma,
        } => LibRequest::Legacy(L::Tfcp {
            voltage_mv,
            current_ma,
        }),
        TriggerRequest::Ufcs {
            index,
            voltage_mv,
            current_ma,
        } => LibRequest::Legacy(L::Ufcs {
            index,
            voltage_mv,
            current_ma,
        }),
    })
}

/// The library risk class as the UI's.
pub fn risk_class(risk: pdm::Risk) -> RiskClass {
    match risk {
        pdm::Risk::ReadOnly => RiskClass::ReadOnly,
        pdm::Risk::AtMost5V => RiskClass::AtMost5V,
        pdm::Risk::Above5V => RiskClass::Above5V,
    }
}

/// A command as the meter receives it, without the line end.
pub fn command_text(command: &pdm::Command) -> String {
    String::from_utf8_lossy(&command.render()).trim().to_owned()
}

/// The library protocol of a non-PD section.
pub(crate) fn legacy_of(protocol: TriggerProtocol) -> Option<LegacyProtocol> {
    match protocol {
        TriggerProtocol::Pd => None,
        TriggerProtocol::Qc => Some(LegacyProtocol::Qc),
        TriggerProtocol::Fcp => Some(LegacyProtocol::Fcp),
        TriggerProtocol::Afc => Some(LegacyProtocol::Afc),
        TriggerProtocol::Sfcp => Some(LegacyProtocol::Sfcp),
        TriggerProtocol::Scp => Some(LegacyProtocol::Scp),
        TriggerProtocol::Vfcp => Some(LegacyProtocol::Vfcp),
        TriggerProtocol::Tfcp => Some(LegacyProtocol::Tfcp),
        TriggerProtocol::Ufcs => Some(LegacyProtocol::Ufcs),
    }
}

pub(crate) fn unsupported(protocol: TriggerProtocol) -> ProtocolSection {
    ProtocolSection {
        protocol,
        supported: false,
        offers: Vec::new(),
        limits: Vec::new(),
        settings: None,
    }
}

pub(crate) fn offer_view(o: &pd::Offer) -> OfferView {
    OfferView {
        position: o.position,
        kind: o.kind.name().to_owned(),
        min_voltage_mv: o.min_voltage_mv,
        max_voltage_mv: o.max_voltage_mv,
        max_current_ma: o.max_current_ma,
        max_power_mw: o.max_power_mw,
        epr: o.is_epr(),
    }
}

/// The log line for a PD listing: position, kind and voltage of each offer.
pub(crate) fn offers_line(offers: &[pd::Offer]) -> String {
    let volts = |mv: u32| f64::from(mv) / 1000.0;
    let list: Vec<String> = offers
        .iter()
        .map(|o| {
            let (min, max) = (volts(o.min_voltage_mv), volts(o.max_voltage_mv));
            let range = if o.min_voltage_mv == o.max_voltage_mv {
                format!("{max:.2}")
            } else {
                format!("{min:.2}-{max:.2}")
            };
            format!("#{} {} {range} V", o.position, o.kind.name())
        })
        .collect();
    format!("trigger offers: {}", list.join(", "))
}

pub(crate) fn limits_view(l: &legacy::Limits) -> LimitsView {
    LimitsView {
        index: l.index,
        min_voltage_mv: l.min_voltage_mv,
        max_voltage_mv: l.max_voltage_mv,
        min_current_ma: l.min_current_ma,
        max_current_ma: l.max_current_ma,
        max_power_mw: l.max_power_mw,
    }
}
