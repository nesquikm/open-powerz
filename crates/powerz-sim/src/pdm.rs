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

/// VBUS (mV) while the meter presents Rd on the charger's 5 V. [device]
const VBUS_5V_MV: u32 = 5180;
/// VBUS (mV) once PD is entered (and after a hard reset). [device]
const VBUS_PD_MV: u32 = 5162;
/// VBUS (mV) with Rd removed (`pdm close`). [device]
const VBUS_OFF_MV: u32 = 7;

impl SimCharger {
    /// The SPR and EPR PDO lists the charger offers.
    fn pdos(self) -> (&'static [u32; PDO_SLOTS], &'static [u32; PDO_SLOTS]) {
        match self {
            SimCharger::Epr140W => (&EPR140_SPR, &EPR140_EPR),
        }
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

    /// Both unprompted offers (SPR then EPR) with their `ready:` lines,
    /// as sent after `entry pd` and after a hard reset. [device]
    fn offers(self) -> Vec<u8> {
        let (spr, epr) = self.pdos();
        let mut out = SimCharger::block(spr);
        out.extend(b"ready:5191mV,0mA\n");
        out.extend(SimCharger::block(epr));
        out.extend(format!("ready:{VBUS_PD_MV}mV,0mA\n").bytes());
        out
    }

    /// Whether the charger speaks `protocol` (an `entry` name).
    fn supports(self, protocol: &str) -> bool {
        match self {
            SimCharger::Epr140W => protocol == "pd",
        }
    }
}

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
    vbus_mv: u32,
    writes: Vec<(u64, Vec<u8>)>,
    pending: Vec<u8>,
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
        if text.starts_with("pdm open") {
            self.pdm_open()
        } else if text.starts_with("pdm close") {
            self.pdm_active = false;
            self.pd_entered = false;
            self.vbus_mv = VBUS_OFF_MV;
            b"ok\n".to_vec()
        } else if text.starts_with("pdm parm") {
            b"type=0,em=2,sink=0\n".to_vec()
        } else if text.starts_with("reset") {
            // Detach; Rd kept, VBUS back to 5 V.
            self.pd_entered = false;
            self.vbus_mv = VBUS_5V_MV;
            b"ok\n".to_vec()
        } else if let Some(rest) = text.strip_prefix("entry ") {
            self.entry(rest.split_whitespace().next().unwrap_or_default())
        } else if let Some(rest) = text.strip_prefix("pd req=") {
            self.pd_request(rest)
        } else if text.starts_with("pd hrst") && self.pd_entered {
            self.vbus_mv = VBUS_PD_MV;
            self.charger.offers()
        } else {
            // Unknown text: silently ignored, case-sensitive. [device]
            Vec::new()
        }
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
            "ufcs" if !self.charger.supports("ufcs") => b"ok\ntimeout\ntimeout\ntimeout\n".to_vec(),
            "qc" | "fcp" | "scp" | "afc" | "sfcp" | "tfcp" if !self.charger.supports(protocol) => {
                b"ok\ntimeout\n".to_vec()
            }
            "vfcp" => b"ok\nready\n".to_vec(),
            "apple" => b"fail\n".to_vec(),
            "bc" => b"ok\n".to_vec(),
            _ => Vec::new(),
        }
    }

    /// `pd req=<pos>[,volt=<mV>][,cur=<mA>]` against the EPR list.
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
        let (_, epr) = self.charger.pdos();
        let Some(&pdo) = pos.checked_sub(1).and_then(|i| epr.get(i)) else {
            return Vec::new();
        };
        let (v, i) = match pdo >> 30 {
            0 if pdo != 0 => ((pdo >> 10 & 0x3FF) * 50, cur.unwrap_or((pdo & 0x3FF) * 10)),
            3 => {
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
                vbus_mv: 0,
                writes: Vec::new(),
                pending: Vec::new(),
            })),
        }
    }

    /// The first `pdm open` answers `pdm mode entry` plus `busy`; `reset`
    /// then answers `ok`.
    pub fn with_busy_open(self) -> SimPdm {
        lock(&self.shared).busy_open = true;
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
