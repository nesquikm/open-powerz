//! The simulated PD sniffer (FR TGE83A): a scripted PD capture played from
//! the moment the sniffer is started (0x10, mode 1), served as PD_PACKET
//! chunks (12-byte status block plus records).
//!
//! Record layout (`docs/protocol.md`): `b0 | u32 ts ms | u8 sop/event | ...`.
//! PD message records have `b0 = 0x80 | (len - 1)`, the 16-bit PD header at
//! +6 and the data objects after it; event records are 6 bytes with
//! `b0 = 0x05`. Every message is a valid USB PD r3.2 message (header,
//! message ID, data object count); all values are synthetic.

use std::collections::VecDeque;

use powerz_protocol::decode::pd_packet::STATUS_LEN;

/// Named PD capture scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdScenario {
    /// Attach, SPR negotiation at 5 V, EPR entry with chunked
    /// EPR_Source_Capabilities and a 28 V EPR contract, EPR keep-alives,
    /// a Hard Reset and a fresh SPR negotiation.
    Negotiation,
}

/// Cap on a PD_PACKET chunk (status block plus records); older records
/// wait for the next poll.
const CHUNK_CAP: usize = 320;
/// CC1 with the sink's Rd attached, mV.
const CC1_ATTACHED_MV: u16 = 1528;

// ---- USB PD r3.2 message building ----

/// Control message types.
const ACCEPT: u8 = 3;
const PS_RDY: u8 = 6;
/// Data message types.
const SOURCE_CAPABILITIES: u8 = 1;
const REQUEST: u8 = 2;
const EPR_REQUEST: u8 = 9;
const EPR_MODE: u8 = 10;
/// Extended message types.
const EXTENDED_CONTROL: u8 = 16;
const EPR_SOURCE_CAPABILITIES: u8 = 17;

/// Extended_Control types.
const EPR_KEEPALIVE: u8 = 3;
const EPR_KEEPALIVE_ACK: u8 = 4;

/// EPR_Mode actions.
const EPR_ENTER: u32 = 1;
const EPR_ENTER_ACK: u32 = 2;
const EPR_ENTER_SUCCEEDED: u32 = 3;

/// Event codes (low nibble) of event records.
const EV_ATTACH_CC1: u8 = 0x11;
const EV_HARD_RESET: u8 = 0x04;

/// Fixed supply PDO: voltage in mV, maximum current in mA, flag bits.
const fn fixed_pdo(mv: u32, ma: u32, flags: u32) -> u32 {
    flags | ((mv / 50) << 10) | (ma / 10)
}

/// PDO1 flags: unconstrained power (bit 27), EPR mode capable (bit 23).
const PDO1_FLAGS: u32 = (1 << 27) | (1 << 23);

/// SPR source PDOs: 5 V 3 A, 9 V 3 A, 15 V 3 A, 20 V 5 A.
const SPR_PDOS: [u32; 4] = [
    fixed_pdo(5_000, 3_000, PDO1_FLAGS),
    fixed_pdo(9_000, 3_000, 0),
    fixed_pdo(15_000, 3_000, 0),
    fixed_pdo(20_000, 5_000, 0),
];

/// EPR PDO 8: fixed 28 V 5 A.
const PDO_28V: u32 = fixed_pdo(28_000, 5_000, 0);

/// EPR PDO 9: EPR AVS 15–28 V, 140 W (bits 31:28 = 1101b, max V in
/// 100 mV at 25:17, min V in 100 mV at 15:8, PDP in W at 7:0).
const PDO_EPR_AVS: u32 = (0b1101 << 28) | (280 << 17) | (150 << 8) | 140;

/// Fixed RDO: object position, operating / maximum current in mA, EPR mode
/// capable (bit 22).
const fn fixed_rdo(position: u32, ma: u32) -> u32 {
    (position << 28) | (1 << 22) | ((ma / 10) << 10) | (ma / 10)
}

/// Who sent a message: the source (DFP) or the sink (UFP).
#[derive(Clone, Copy)]
enum Role {
    Source,
    Sink,
}

/// 16-bit PD message header for SOP: type, data roles and power roles of
/// `role`, spec revision 3.x, message ID, data object count, extended.
fn pd_header(ty: u8, role: Role, id: u8, ndo: usize, extended: bool) -> u16 {
    let src = u16::from(matches!(role, Role::Source));
    u16::from(ty & 0x1F)
        | (src << 5) // port data role: DFP
        | (0b10 << 6) // spec revision 3.x
        | (src << 8) // port power role: source
        | (u16::from(id & 7) << 9)
        | (((ndo as u16) & 7) << 12)
        | (u16::from(extended) << 15)
}

/// The scripted capture: message records with their per-port message ID
/// counters, built in time order.
struct Script {
    records: Vec<(u64, Vec<u8>)>,
    source_id: u8,
    sink_id: u8,
}

impl Script {
    fn new() -> Script {
        Script {
            records: Vec::new(),
            source_id: 0,
            sink_id: 0,
        }
    }

    /// Next message ID of `role`'s port (3 bits, wraps).
    fn next_id(&mut self, role: Role) -> u8 {
        let c = match role {
            Role::Source => &mut self.source_id,
            Role::Sink => &mut self.sink_id,
        };
        let id = *c;
        *c = (*c + 1) & 7;
        id
    }

    /// A PD message record on SOP at `t` ms: header plus `body` (already a
    /// whole number of 4-byte data objects).
    fn message(&mut self, t: u64, ty: u8, role: Role, body: &[u8], extended: bool) {
        let id = self.next_id(role);
        let ndo = body.len() / 4;
        let header = pd_header(ty, role, id, ndo, extended);
        let len = 8 + body.len();
        let mut r = Vec::with_capacity(len);
        r.push(0x80 | (len as u8 - 1));
        r.extend([0; 4]); // time, filled in when the record is played
        r.push(0); // SOP
        r.extend(header.to_le_bytes());
        r.extend(body);
        self.records.push((t, r));
    }

    fn control(&mut self, t: u64, ty: u8, role: Role) {
        self.message(t, ty, role, &[], false);
    }

    fn data(&mut self, t: u64, ty: u8, role: Role, objects: &[u32]) {
        let body: Vec<u8> = objects.iter().flat_map(|o| o.to_le_bytes()).collect();
        self.message(t, ty, role, &body, false);
    }

    /// Extended message: 16-bit extended header plus `data`, padded to a
    /// whole number of data objects.
    fn extended(&mut self, t: u64, ty: u8, role: Role, ext_header: u16, data: &[u8]) {
        let mut body = ext_header.to_le_bytes().to_vec();
        body.extend(data);
        body.resize(body.len().div_ceil(4) * 4, 0);
        self.message(t, ty, role, &body, true);
    }

    /// Event record at `t` ms.
    fn event(&mut self, t: u64, code: u8) {
        self.records.push((t, vec![0x05, 0, 0, 0, 0, code]));
    }

    /// Hard Reset: both ports' message ID counters restart.
    fn hard_reset(&mut self, t: u64) {
        self.event(t, EV_HARD_RESET);
        self.source_id = 0;
        self.sink_id = 0;
    }

    /// SPR negotiation at 5 V starting at `t`: Source_Capabilities,
    /// Request (PDO 1, 3 A), Accept, PS_RDY.
    fn spr_negotiation(&mut self, t: u64) {
        self.data(t, SOURCE_CAPABILITIES, Role::Source, &SPR_PDOS);
        self.data(t + 2, REQUEST, Role::Sink, &[fixed_rdo(1, 3_000)]);
        self.control(t + 4, ACCEPT, Role::Source);
        self.control(t + 30, PS_RDY, Role::Source);
    }
}

/// Extended header: data size, chunked (bit 15), chunk number (14:11),
/// request chunk (bit 10).
fn ext_header(size: u16, chunk: u16, request: bool) -> u16 {
    (1 << 15) | (chunk << 11) | (u16::from(request) << 10) | (size & 0x1FF)
}

/// One step of the VBUS contract: from `t` ms on, VBUS is `mv`.
const VBUS_STEPS: [(u64, u16); 4] = [
    (0, 5_000),
    (T_EPR_RDY, 28_000),
    (T_RESET, 0),
    (T_RESET + 700, 5_000),
];

/// Script times, ms since the sniffer start.
const T_ATTACH: u64 = 20;
const T_SPR: u64 = 300;
const T_EPR: u64 = 600;
const T_EPR_RDY: u64 = 1_100;
const T_RESET: u64 = 2_400;

/// The records of `scenario`, in time order, with times in ms since the
/// sniffer start.
fn script(scenario: PdScenario) -> Vec<(u64, Vec<u8>)> {
    match scenario {
        PdScenario::Negotiation => negotiation(),
    }
}

fn negotiation() -> Vec<(u64, Vec<u8>)> {
    let mut s = Script::new();
    s.event(T_ATTACH, EV_ATTACH_CC1);
    s.spr_negotiation(T_SPR);

    // EPR entry.
    let pdp_w = 140;
    s.data(
        T_EPR,
        EPR_MODE,
        Role::Sink,
        &[(EPR_ENTER << 24) | (pdp_w << 16)],
    );
    s.data(T_EPR + 2, EPR_MODE, Role::Source, &[EPR_ENTER_ACK << 24]);
    s.data(
        T_EPR + 100,
        EPR_MODE,
        Role::Source,
        &[EPR_ENTER_SUCCEEDED << 24],
    );

    // EPR_Source_Capabilities, chunked (26 bytes per chunk): SPR PDOs in
    // positions 1..7, EPR PDOs from position 8.
    let mut pdos = SPR_PDOS.to_vec();
    pdos.resize(7, 0);
    pdos.extend([PDO_28V, PDO_EPR_AVS]);
    let caps: Vec<u8> = pdos.iter().flat_map(|p| p.to_le_bytes()).collect();
    let size = caps.len() as u16;
    let t = T_EPR + 103;
    s.extended(
        t,
        EPR_SOURCE_CAPABILITIES,
        Role::Source,
        ext_header(size, 0, false),
        &caps[..26],
    );
    s.extended(
        t + 2,
        EPR_SOURCE_CAPABILITIES,
        Role::Sink,
        ext_header(0, 1, true),
        &[],
    );
    s.extended(
        t + 4,
        EPR_SOURCE_CAPABILITIES,
        Role::Source,
        ext_header(size, 1, false),
        &caps[26..],
    );

    // EPR_Request for PDO 8 (28 V 5 A): RDO plus a copy of the PDO.
    s.data(
        t + 6,
        EPR_REQUEST,
        Role::Sink,
        &[fixed_rdo(8, 5_000), PDO_28V],
    );
    s.control(t + 9, ACCEPT, Role::Source);
    s.control(T_EPR_RDY, PS_RDY, Role::Source);

    // EPR keep-alives while in the EPR contract.
    for k in 1..=3 {
        let t = T_EPR_RDY + 350 * k;
        s.extended(
            t,
            EXTENDED_CONTROL,
            Role::Sink,
            ext_header(2, 0, false),
            &[EPR_KEEPALIVE, 0],
        );
        s.extended(
            t + 2,
            EXTENDED_CONTROL,
            Role::Source,
            ext_header(2, 0, false),
            &[EPR_KEEPALIVE_ACK, 0],
        );
    }

    // Hard Reset, then a fresh SPR negotiation.
    s.hard_reset(T_RESET);
    s.spr_negotiation(T_RESET + 800);
    s.records
}

/// VBUS of the scripted contract at `t` ms since the sniffer start, mV.
fn vbus_at(t: u64) -> u16 {
    VBUS_STEPS
        .iter()
        .rev()
        .find(|(from, _)| *from <= t)
        .map_or(0, |(_, mv)| *mv)
}

/// The sniffer state of the simulated meter.
#[derive(Debug, Default)]
pub(crate) struct PdSniffer {
    /// Scenario played while sniffing, if any.
    scenario: Option<PdScenario>,
    /// Uptime the sniffer was started at, `None` when stopped.
    started: Option<u64>,
    /// Records still to be played: (ms since start, record).
    script: VecDeque<(u64, Vec<u8>)>,
    /// Played records waiting to be polled.
    ready: VecDeque<Vec<u8>>,
}

impl PdSniffer {
    pub(crate) fn set_scenario(&mut self, scenario: PdScenario) {
        self.scenario = Some(scenario);
    }

    /// Start sniffing at uptime `now`: the script starts over.
    pub(crate) fn start(&mut self, now: u64) {
        self.started = Some(now);
        self.ready.clear();
        self.script = self.scenario.map(script).unwrap_or_default().into();
    }

    /// Stop sniffing; nothing more is captured.
    pub(crate) fn stop(&mut self) {
        self.started = None;
        self.script.clear();
        self.ready.clear();
    }

    /// One PD_PACKET payload at uptime `now`: the status block, then the
    /// records due so far, up to [`CHUNK_CAP`] bytes. `fallback` gives
    /// `(vbus mV, ibus mA, cc1 mV, cc2 mV)` when no scenario is playing.
    pub(crate) fn chunk(&mut self, now: u64, fallback: (u16, i16, u16, u16)) -> Vec<u8> {
        let (vbus, ibus, cc1, cc2) = match (self.started, self.scenario) {
            (Some(start), Some(_)) => {
                let t = now - start;
                let cc1 = if t >= T_ATTACH { CC1_ATTACHED_MV } else { 0 };
                (vbus_at(t), 0, cc1, 0)
            }
            _ => fallback,
        };
        let mut p = Vec::with_capacity(CHUNK_CAP);
        p.extend((now as u32).to_le_bytes());
        p.extend(vbus.to_le_bytes());
        p.extend(ibus.to_le_bytes());
        p.extend(cc1.to_le_bytes());
        p.extend(cc2.to_le_bytes());
        debug_assert_eq!(p.len(), STATUS_LEN);
        let Some(start) = self.started else {
            return p;
        };
        while let Some((t, _)) = self.script.front()
            && start + *t <= now
        {
            let Some((t, mut r)) = self.script.pop_front() else {
                break;
            };
            r[1..5].copy_from_slice(&((start + t) as u32).to_le_bytes());
            self.ready.push_back(r);
        }
        while let Some(r) = self.ready.front()
            && p.len() + r.len() <= CHUNK_CAP
        {
            let Some(r) = self.ready.pop_front() else {
                break;
            };
            p.extend(r);
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_records_have_consistent_lengths_and_headers() {
        for (_, r) in negotiation() {
            if r[0] & 0x80 != 0 {
                let len = usize::from(r[0] & 0x3F) + 1;
                assert_eq!(len, r.len());
                let h = u16::from_le_bytes([r[6], r[7]]);
                let ndo = usize::from((h >> 12) & 7);
                assert_eq!(8 + 4 * ndo, r.len(), "{r:02x?}");
                assert_eq!((h >> 6) & 3, 0b10, "spec revision 3.x");
            } else {
                assert_eq!(usize::from(r[0] & 0x1F) + 1, r.len());
            }
        }
    }

    #[test]
    fn epr_caps_chunks_carry_the_whole_list() {
        let recs = negotiation();
        let chunks: Vec<_> = recs
            .iter()
            .filter(|(_, r)| {
                if r[0] & 0x80 == 0 {
                    return false;
                }
                let h = u16::from_le_bytes([r[6], r[7]]);
                h & 0x8000 != 0 && h & 0x1F == 17 && (h >> 8) & 1 == 1
            })
            .collect();
        assert_eq!(chunks.len(), 2);
        let mut data: Vec<u8> = Vec::new();
        for (_, r) in chunks {
            let eh = u16::from_le_bytes([r[8], r[9]]);
            let size = usize::from(eh & 0x1FF);
            assert_eq!(size, 36);
            data.extend(&r[10..]);
        }
        data.truncate(36);
        assert_eq!(&data[28..32], &PDO_28V.to_le_bytes());
        assert_eq!(&data[32..36], &PDO_EPR_AVS.to_le_bytes());
    }
}
