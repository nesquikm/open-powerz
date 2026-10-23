//! Simulated PDM text session on the meter's virtual serial port
//! (FR PR41FT, AC .6).
//!
//! [`SimPdm`] is the simulated meter plus a scripted charger; each
//! [`SimPdm::open_port`] returns a [`SimPdmPort`] (`Read + Write`) the way
//! opening the tty does. Replies follow the device-confirmed session-2
//! transcript: `\n`-terminated lines, unprompted `pdo:` blocks after
//! `entry pd`, `ok\ntimeout\n` for protocols the charger lacks, no reply to
//! unknown text or to anything before `pdm open`. Opening the port again
//! drops the text session (the device needs `pdm open` again) and makes
//! older port handles fail.

use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use powerz_protocol::frame::{Attribute, DataHeader, SectionHeader};

use crate::SimClock;
use crate::device::{msg, put_data_obj};

/// Scripted chargers behind the simulated meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimCharger {
    /// 140 W PD 3.1 EPR charger (session 2): SPR 5/9/15 V 3 A + 20 V 4.7 A;
    /// EPR list 5/9/15 V 3 A, 20 V 5 A, 28 V 5 A (slot 8), AVS 15-28 V
    /// 140 W (slot 9). PD only: every other protocol entry times out.
    Epr140W,
    /// 65 W PD 3.0 SPR-only charger (no EPR): 5/9/15 V 3 A, 20 V 3.25 A,
    /// PPS 3.3-21 V 3.25 A (slot 5). Only the SPR `pdo:` block follows
    /// `entry pd` / `pd hrst`. [inferred]
    Pps65W,
    /// Synthetic multi-protocol charger (FR DGSQ6Q): the 65 W PD offers,
    /// plus QC 2.0 / 3.0, FCP, AFC, SFCP, SCP, VFCP, TFCP and UFCS. Entries
    /// answer `ok\nready\n`, followed by a capability block for SCP (40 W,
    /// 3.4-20 V, 0.1-4 A), VFCP (18 W, 9 V 2 A / 12 V 1.5 A), TFCP (60 W,
    /// 5-20 V, 0.1-3 A) and UFCS (#1 3.4-11 V 5 A, #2 3.4-21 V 3 A); a
    /// request answers `ready\n` and moves VBUS to the requested voltage.
    /// \[static\] reply forms; no such charger has been seen on the device.
    MultiProtocol,
}

/// E-marked cables between the simulated meter and the charger (FR DGSQ6T).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimCable {
    /// The session-2 240 W passive cable: VID 0x050D, PID 0x8254, 50 V,
    /// 5 A, EPR capable. It answers a SOP' Discover Identity with its ACK.
    /// \[device\] (the ACK seen in the PD capture)
    Epr240W,
}

/// The session-2 240 W cable's Discover Identity ACK data objects. [device]
const CABLE_EPR240_ACK: [u32; 5] = [
    0xFF00_A841,
    0x1860_050D,
    0x0000_404D,
    0x8254_0000,
    0x000A_2640,
];

/// PD CRC-32 (IEEE 802.3, reflected) over the header and data objects.
fn pd_crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

impl SimCable {
    /// The ACK as a received-message line in the vendor doc's receive form:
    /// `pd data=` + SOP byte, header LE, header LE again, data objects LE,
    /// CRC-32 LE. [inferred] (receive form from the doc, not yet seen on a
    /// device)
    fn discover_identity_ack(self) -> Vec<u8> {
        let objects = match self {
            SimCable::Epr240W => CABLE_EPR240_ACK,
        };
        // Vendor_Defined, NumDO 5, Spec Rev 3.0, Cable Plug. [device]
        let header: u16 = 0x518F;
        let mut msg = header.to_le_bytes().to_vec();
        for o in objects {
            msg.extend(o.to_le_bytes());
        }
        let crc = pd_crc32(&msg);
        let mut raw = vec![1u8];
        raw.extend(header.to_le_bytes());
        raw.extend(&msg);
        raw.extend(crc.to_le_bytes());
        let hex: String = raw.iter().map(|b| format!("{b:02X}")).collect();
        format!("pd data={hex}\n").into_bytes()
    }
}

/// Synthetic capability block (with its `\n`) for `scp` / `vfcp` / `tfcp` /
/// `ufcs`,
/// in the \[static\] layouts: SCP 40 W, 3.4-20 V, 0.1-4 A, 10 mV / 50 mA
/// steps, bound 11 V / 5 A; VFCP 18 W, 9 V 2 A / 12 V 1.5 A and a short
/// tail; TFCP 60 W, 5-20 V, 0.1-3 A; UFCS #1 3.4-11 V 0.1-5 A, #2
/// 3.4-21 V 0.1-3 A.
fn legacy_block(protocol: &str) -> Vec<u8> {
    let mut b = format!("{protocol}:").into_bytes();
    match protocol {
        "scp" => {
            for v in [400u16, 3400, 20000, 100, 4000, 10, 50, 11000, 5000] {
                b.extend(v.to_le_bytes());
            }
        }
        "vfcp" => {
            b.push(0);
            for v in [7000u16, 9000, 12000, 2000, 1500, 180] {
                b.extend(v.to_le_bytes());
            }
            b.extend([0x04, 0x00, 0x01, 0x00, 0x0D, 0x20, 0xA6, 0x00]);
        }
        "tfcp" => {
            b.push(0);
            b.extend(600u32.to_le_bytes());
            for v in [5000u16, 100, 20000, 3000] {
                b.extend(v.to_le_bytes());
            }
        }
        "ufcs" => {
            // Count, then u64 LE entries: index (63..60), current step code
            // (59..57, 50 mA), max V (55..40) and min V (39..24) in 10 mV,
            // max I (23..8) and min I (7..0) in 10 mA.
            let entry = |index: u64, max_10mv: u64, max_10ma: u64| -> u64 {
                (index << 60) | (4 << 57) | (max_10mv << 40) | (340 << 24) | (max_10ma << 8) | 10
            };
            b.push(2);
            b.extend(entry(1, 1100, 500).to_le_bytes());
            b.extend(entry(2, 2100, 300).to_le_bytes());
        }
        _ => return Vec::new(),
    }
    b.push(b'\n');
    b
}

/// Slots per `pdo:` block. [device]
const PDO_SLOTS: usize = 11;

/// 140 W charger, SPR Source_Capabilities (u32 PDOs). [device]
const EPR140_SPR: [u32; PDO_SLOTS] = [
    0x0981_912C,
    0x0002_D12C,
    0x0004_B12C,
    0x0006_41D6,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
];

/// 140 W charger, EPR_Source_Capabilities (slot 8 = 28 V 5 A, slot 9 =
/// EPR AVS 15-28 V 140 W). [device]
const EPR140_EPR: [u32; PDO_SLOTS] = [
    0x0981_912C,
    0x0002_D12C,
    0x0004_B12C,
    0x0006_41F4,
    0,
    0,
    0,
    0x0008_C1F4,
    0xD230_968C,
    0,
    0,
];

/// 65 W PPS charger, SPR Source_Capabilities: 5/9/15 V 3 A, 20 V 3.25 A,
/// PPS 3.3-21 V 3.25 A (slot 5). [inferred]
const PPS65_SPR: [u32; PDO_SLOTS] = [
    0x0001_912C,
    0x0002_D12C,
    0x0004_B12C,
    0x0006_4145,
    0xC1A4_2141,
    0,
    0,
    0,
    0,
    0,
    0,
];

/// VBUS (mV) while the meter presents Rd on the charger's 5 V. [device]
const VBUS_5V_MV: u32 = 5180;
/// VBUS (mV) once PD is entered (and after a hard reset). [device]
const VBUS_PD_MV: u32 = 5162;
/// VBUS (mV) with Rd removed (`pdm close`). [device]
const VBUS_OFF_MV: u32 = 7;

impl SimCharger {
    /// The SPR PDO list and, for an EPR charger, the EPR list.
    fn pdos(self) -> (&'static [u32; PDO_SLOTS], Option<&'static [u32; PDO_SLOTS]>) {
        match self {
            SimCharger::Epr140W => (&EPR140_SPR, Some(&EPR140_EPR)),
            SimCharger::Pps65W | SimCharger::MultiProtocol => (&PPS65_SPR, None),
        }
    }

    /// The list requests are made against: the EPR list when there is one.
    fn active_pdos(self) -> &'static [u32; PDO_SLOTS] {
        let (spr, epr) = self.pdos();
        epr.unwrap_or(spr)
    }

    /// `pdo:` block: tag, ASCII digit of the highest used slot, slot
    /// count, slots as u32 LE, `\n`. [device]
    fn block(list: &[u32; PDO_SLOTS]) -> Vec<u8> {
        let highest = list.iter().rposition(|&p| p != 0).map_or(0, |i| i + 1);
        let mut b = b"pdo:".to_vec();
        b.push(b'0' + highest as u8);
        b.push(PDO_SLOTS as u8);
        for p in list {
            b.extend(p.to_le_bytes());
        }
        b.push(b'\n');
        b
    }

    /// The unprompted offers (SPR, then EPR when the charger has it) with
    /// their `ready:` lines, as sent after `entry pd` and after a hard
    /// reset. [device]
    fn offers(self) -> Vec<u8> {
        let (spr, epr) = self.pdos();
        let mut out = SimCharger::block(spr);
        match epr {
            Some(epr) => {
                out.extend(b"ready:5191mV,0mA\n");
                out.extend(SimCharger::block(epr));
                out.extend(format!("ready:{VBUS_PD_MV}mV,0mA\n").bytes());
            }
            None => out.extend(format!("ready:{VBUS_PD_MV}mV,0mA\n").bytes()),
        }
        out
    }

    /// Whether the charger speaks `protocol` (an `entry` name).
    fn supports(self, protocol: &str) -> bool {
        match self {
            SimCharger::Epr140W => protocol == "pd",
            SimCharger::Pps65W => protocol == "pd",
            SimCharger::MultiProtocol => true,
        }
    }
}

/// The meter's `pdm parm` reply out of the box. \[device\] (session 2)
const DEFAULT_PARM: &str = "type=0,em=2,sink=0";

#[derive(Debug)]
struct State {
    charger: SimCharger,
    clock: SimClock,
    busy_open: bool,
    interleave: bool,
    generation: u64,
    port_open: bool,
    pdm_active: bool,
    pd_entered: bool,
    /// The legacy protocol entered (`entry qc`, ...), until `reset`.
    legacy: Option<String>,
    /// Last legacy request, mV (QC 3.0 steps move from here).
    legacy_mv: u32,
    /// `entry list` / `entry list+` stalled: silent until `reset`. [device]
    listing: bool,
    vbus_mv: u32,
    /// What `pdm parm` reports; `pdm set` replaces it. \[device\] default
    parm: String,
    writes: Vec<(u64, Vec<u8>)>,
    pending: Vec<u8>,
    /// The e-marked cable on the input, if any.
    cable: Option<SimCable>,
}

impl State {
    /// Handle one written command; return the reply bytes.
    fn handle(&mut self, cmd: &[u8]) -> Vec<u8> {
        let text = String::from_utf8_lossy(cmd);
        let text = text.trim_end_matches(['\r', '\n']);
        if !self.pdm_active {
            if text.starts_with("pdm open") {
                return self.pdm_open();
            }
            return Vec::new();
        }
        if self.listing && !text.starts_with("reset") && !text.starts_with("pdm close") {
            // Stalled after the list: nothing answers but `reset`. [device]
            return Vec::new();
        }
        if text.starts_with("pdm open") {
            self.pdm_open()
        } else if text.starts_with("pdm close") {
            self.listing = false;
            self.pdm_active = false;
            self.pd_entered = false;
            self.legacy = None;
            self.vbus_mv = VBUS_OFF_MV;
            b"ok\n".to_vec()
        } else if text.starts_with("pdm parm") {
            format!("{}\n", self.parm).into_bytes()
        } else if let Some(rest) = text.strip_prefix("pdm set ") {
            // No reply. [static]
            self.parm = rest.trim().to_string();
            Vec::new()
        } else if text.starts_with("reset") {
            // Detach; Rd kept, VBUS back to 5 V.
            self.pd_entered = false;
            self.legacy = None;
            self.listing = false;
            self.vbus_mv = VBUS_5V_MV;
            b"ok\n".to_vec()
        } else if let Some(rest) = text.strip_prefix("entry ") {
            self.entry(rest.split_whitespace().next().unwrap_or_default())
        } else if let Some(rest) = text.strip_prefix("pd req=") {
            self.pd_request(rest)
        } else if let Some(mv) = self.legacy_target(text) {
            self.legacy_mv = mv;
            self.vbus_mv = if mv <= 5000 {
                VBUS_5V_MV
            } else {
                mv * 985 / 1000
            };
            b"ready\n".to_vec()
        } else if let Some(hex) = text.strip_prefix("pd data=") {
            self.pd_data(hex)
        } else if text.starts_with("pd hrst") && self.pd_entered {
            self.vbus_mv = VBUS_PD_MV;
            self.charger.offers()
        } else {
            // Unknown text: silently ignored, case-sensitive. [device]
            Vec::new()
        }
    }

    /// `pd data=<hex>`: the e-marked cable answers a SOP' Discover Identity
    /// REQ with its ACK; anything else is sent silently.
    fn pd_data(&self, hex: &str) -> Vec<u8> {
        let Some(cable) = self.cable else {
            return Vec::new();
        };
        let bytes: Option<Vec<u8>> = (0..hex.len() / 2)
            .map(|i| {
                hex.get(2 * i..2 * i + 2)
                    .and_then(|p| u8::from_str_radix(p, 16).ok())
            })
            .collect();
        let Some([sop, lo, hi, o0, o1, o2, o3, ..]) = bytes.as_deref() else {
            return Vec::new();
        };
        let header = u16::from_le_bytes([*lo, *hi]);
        let vdm = u32::from_le_bytes([*o0, *o1, *o2, *o3]);
        // SOP', Vendor_Defined, structured VDM (SVID 0xFF00) Discover
        // Identity REQ.
        let discover_req =
            header & 0x1F == 0x0F && vdm & 0xFFFF_80DF == 0xFF00_8001 && vdm & 0xC0 == 0;
        if *sop != 1 || !discover_req {
            return Vec::new();
        }
        cable.discover_identity_ack()
    }

    fn pdm_open(&mut self) -> Vec<u8> {
        self.pdm_active = true;
        if self.vbus_mv < VBUS_5V_MV {
            self.vbus_mv = VBUS_5V_MV;
        }
        let mut out = b"pdm mode entry\nver1.0\n".to_vec();
        if self.busy_open {
            self.busy_open = false;
            out.extend(b"busy\n");
        }
        out
    }

    fn entry(&mut self, protocol: &str) -> Vec<u8> {
        match protocol {
            "pd" if self.charger.supports("pd") => {
                self.pd_entered = true;
                self.vbus_mv = VBUS_PD_MV;
                let mut out = b"ok\ncc1 attach\n".to_vec();
                out.extend(self.charger.offers());
                out
            }
            // SCP / VFCP / TFCP / UFCS follow `ready` with their block;
            // `legacy_block` is empty for the others.
            "qc" | "fcp" | "afc" | "sfcp" | "scp" | "vfcp" | "tfcp" | "ufcs"
                if self.charger.supports(protocol) =>
            {
                self.legacy = Some(protocol.to_string());
                self.legacy_mv = 5000;
                let mut out = b"ok\nready\n".to_vec();
                out.extend(legacy_block(protocol));
                out
            }
            "ufcs" if !self.charger.supports("ufcs") => b"ok\ntimeout\ntimeout\ntimeout\n".to_vec(),
            "qc" | "fcp" | "scp" | "afc" | "sfcp" | "tfcp" if !self.charger.supports(protocol) => {
                b"ok\ntimeout\n".to_vec()
            }
            "vfcp" => b"ok\nready\n".to_vec(),
            "apple" => b"fail\n".to_vec(),
            "bc" => b"ok\n".to_vec(),
            // Session 2 on the 140 W charger, then silence. [device]
            "list" | "list+" => {
                self.listing = true;
                b"starting\nbc1.2:true 5203mV,0mA\nufcs:null\nvfcp:null\n".to_vec()
            }
            _ => Vec::new(),
        }
    }

    /// The voltage (mV) a legacy request asks for, when the protocol it
    /// belongs to is entered: `qc|fcp|afc|sfcp <n>V`, `qc3 volt=<mV>`,
    /// `qc3 inc=<n>` / `qc3 dec=<n>` (200 mV per step, kept in 3.6-20 V).
    fn legacy_target(&self, text: &str) -> Option<u32> {
        let entered = self.legacy.as_deref()?;
        if let Some(v) = text
            .strip_prefix(entered)
            .and_then(|r| r.strip_prefix(' '))
            .and_then(|r| r.strip_suffix('V'))
        {
            return v.parse::<u32>().ok().map(|v| v * 1000);
        }
        if let Some(args) = text
            .strip_prefix(entered)
            .and_then(|r| r.strip_prefix(" volt="))
        {
            // `scp|vfcp|tfcp volt=<mV>,cur=<mA>`
            return args.split(',').next()?.parse::<u32>().ok();
        }
        if entered == "ufcs" {
            // `ufcs req=<n>,volt=<mV>,cur=<mA>` moves VBUS; `ufcs cmd=<n>`
            // answers `ready` and leaves it where it is.
            if let Some(args) = text.strip_prefix("ufcs req=") {
                return args
                    .split(',')
                    .find_map(|kv| kv.strip_prefix("volt="))?
                    .parse::<u32>()
                    .ok();
            }
            if text.starts_with("ufcs cmd=") {
                return Some(self.legacy_mv);
            }
            return None;
        }
        if entered != "qc" {
            return None;
        }
        let (min, max, step) = (3600u32, 20_000u32, 200u32);
        if let Some(v) = text.strip_prefix("qc3 volt=") {
            return v.parse::<u32>().ok().map(|v| v.clamp(min, max));
        }
        let (n, up) = if let Some(n) = text.strip_prefix("qc3 inc=") {
            (n, true)
        } else {
            (text.strip_prefix("qc3 dec=")?, false)
        };
        let delta = n.parse::<u32>().ok()?.saturating_mul(step);
        let mv = if up {
            self.legacy_mv.saturating_add(delta)
        } else {
            self.legacy_mv.saturating_sub(delta)
        };
        Some(mv.clamp(min, max))
    }

    /// `pd req=<pos>[,volt=<mV>][,cur=<mA>]` against the active list (EPR
    /// when the charger has one): fixed, PPS and EPR AVS offers.
    fn pd_request(&mut self, args: &str) -> Vec<u8> {
        if !self.pd_entered {
            return Vec::new();
        }
        let mut parts = args.split(',');
        let Some(pos) = parts.next().and_then(|p| p.trim().parse::<usize>().ok()) else {
            return Vec::new();
        };
        let (mut volt, mut cur) = (None, None);
        for kv in parts {
            if let Some(v) = kv.strip_prefix("volt=") {
                volt = v.trim().parse::<u32>().ok();
            } else if let Some(c) = kv.strip_prefix("cur=") {
                cur = c.trim().parse::<u32>().ok();
            }
        }
        let list = self.charger.active_pdos();
        let Some(&pdo) = pos.checked_sub(1).and_then(|i| list.get(i)) else {
            return Vec::new();
        };
        let (v, i) = match pdo >> 30 {
            0 if pdo != 0 => ((pdo >> 10 & 0x3FF) * 50, cur.unwrap_or((pdo & 0x3FF) * 10)),
            // SPR PPS APDO: 3.3-21 V style range, current in 50 mA units.
            3 if pdo >> 28 & 0x3 == 0 => {
                let min = (pdo >> 8 & 0xFF) * 100;
                let max = (pdo >> 17 & 0xFF) * 100;
                let v = volt.unwrap_or(min).clamp(min, max);
                (v, cur.unwrap_or((pdo & 0x7F) * 50))
            }
            // EPR AVS APDO: range in 100 mV units, PDP in watts.
            3 if pdo >> 28 & 0x3 == 1 => {
                let min = (pdo >> 8 & 0xFF) * 100;
                let max = (pdo >> 17 & 0x1FF) * 100;
                let v = volt.unwrap_or(min).clamp(min, max);
                let pdp_mw = (pdo & 0xFF) * 1000;
                (v, cur.unwrap_or(pdp_mw * 1000 / v.max(1)))
            }
            _ => return Vec::new(),
        };
        self.vbus_mv = if v <= 5000 {
            VBUS_5V_MV
        } else {
            v * 985 / 1000
        };
        format!("req:{v}mV,{i}mA\nready:{}mV,0mA\n", self.vbus_mv).into_bytes()
    }

    /// An ADCNEW frame and a PUT_DATA ADC frame, as the meter interleaves
    /// them on the tty. [device]
    fn binary_frames(&self) -> Vec<u8> {
        // ADCNEW: `02 att words crc` + words × 4 bytes.
        let mut out = vec![0x02, 0x12, 0x05, 0x00];
        out.extend([0u8; 20]);
        // PUT_DATA with one ADC section (44 bytes, VBUS in µV).
        let mut payload = vec![0u8; 44];
        if let Some(v) = payload.get_mut(0..4) {
            v.copy_from_slice(&(self.vbus_mv as i32 * 1000).to_le_bytes());
        }
        let section = SectionHeader {
            att: Attribute::ADC.bits(),
            next: false,
            chunk: 0,
            size: payload.len() as u16,
        };
        let header = DataHeader {
            ty: msg::PUT_DATA,
            extend: false,
            id: 0,
            encode: false,
            permi: 0,
            obj: put_data_obj(4 + 4 + payload.len()),
        };
        out.extend(header.to_header().encode());
        out.extend(section.encode());
        out.extend(payload);
        out
    }
}

/// The simulated meter + charger. Clones share one simulation.
#[derive(Debug, Clone)]
pub struct SimPdm {
    shared: Arc<Mutex<State>>,
}

/// Lock the shared state, recovering from a poisoned lock.
fn lock(shared: &Mutex<State>) -> MutexGuard<'_, State> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

impl SimPdm {
    /// A meter with `charger` on its input and nothing on its output.
    pub fn new(charger: SimCharger, clock: SimClock) -> SimPdm {
        SimPdm {
            shared: Arc::new(Mutex::new(State {
                charger,
                clock,
                busy_open: false,
                interleave: false,
                generation: 0,
                port_open: false,
                pdm_active: false,
                pd_entered: false,
                legacy: None,
                legacy_mv: 5000,
                listing: false,
                vbus_mv: 0,
                parm: DEFAULT_PARM.to_string(),
                writes: Vec::new(),
                pending: Vec::new(),
                cable: None,
            })),
        }
    }

    /// The first `pdm open` answers `pdm mode entry` plus `busy`; `reset`
    /// then answers `ok`.
    pub fn with_busy_open(self) -> SimPdm {
        lock(&self.shared).busy_open = true;
        self
    }

    /// Put an e-marked `cable` on the input: it answers SOP' Discover
    /// Identity (`pd data=01...`) once the PDM session is open.
    pub fn with_cable(self, cable: SimCable) -> SimPdm {
        lock(&self.shared).cable = Some(cable);
        self
    }

    /// Put an ADCNEW frame and a PUT_DATA ADC frame before every reply.
    pub fn with_interleaved_binary(self) -> SimPdm {
        lock(&self.shared).interleave = true;
        self
    }

    /// Open the tty: a new port handle. Drops the text session and makes
    /// earlier handles fail with a broken pipe.
    pub fn open_port(&self) -> SimPdmPort {
        let mut s = lock(&self.shared);
        s.generation += 1;
        s.port_open = true;
        s.pdm_active = false;
        s.pd_entered = false;
        s.legacy = None;
        s.pending.clear();
        SimPdmPort {
            shared: Arc::clone(&self.shared),
            generation: s.generation,
        }
    }

    /// Every write so far, one entry per `write` call, as text.
    pub fn writes(&self) -> Vec<String> {
        lock(&self.shared)
            .writes
            .iter()
            .map(|(_, w)| String::from_utf8_lossy(w).into_owned())
            .collect()
    }

    /// Simulated time (ms) of every write so far.
    pub fn write_times_ms(&self) -> Vec<u64> {
        lock(&self.shared).writes.iter().map(|(t, _)| *t).collect()
    }

    /// Whether the PDM text session is open.
    pub fn pdm_active(&self) -> bool {
        lock(&self.shared).pdm_active
    }

    /// Simulated VBUS on the input, mV (0 until `pdm open` presents Rd).
    pub fn vbus_mv(&self) -> u32 {
        lock(&self.shared).vbus_mv
    }

    /// Whether the newest port handle is still open (not dropped).
    pub fn port_open(&self) -> bool {
        lock(&self.shared).port_open
    }
}

/// One open handle to the simulated tty.
#[derive(Debug)]
pub struct SimPdmPort {
    shared: Arc<Mutex<State>>,
    generation: u64,
}

fn broken_pipe() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "simulated tty was reopened")
}

impl Read for SimPdmPort {
    /// Pending reply bytes; `TimedOut` when none; `BrokenPipe` on a stale handle.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut s = lock(&self.shared);
        if s.generation != self.generation {
            return Err(broken_pipe());
        }
        if s.pending.is_empty() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "no reply pending"));
        }
        let n = buf.len().min(s.pending.len());
        buf[..n].copy_from_slice(&s.pending[..n]);
        s.pending.drain(..n);
        Ok(n)
    }
}

impl Write for SimPdmPort {
    /// One command per write; `BrokenPipe` on a stale handle.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut s = lock(&self.shared);
        if s.generation != self.generation {
            return Err(broken_pipe());
        }
        let now = s.clock.now_ms();
        s.writes.push((now, buf.to_vec()));
        let reply = s.handle(buf);
        if !reply.is_empty() {
            if s.interleave {
                let frames = s.binary_frames();
                s.pending.extend(frames);
            }
            s.pending.extend(reply);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for SimPdmPort {
    fn drop(&mut self) {
        let mut s = lock(&self.shared);
        if s.generation == self.generation {
            s.port_open = false;
        }
    }
}
