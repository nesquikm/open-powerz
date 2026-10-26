//! Simulated POWER-Z KM003C behind the `powerz-protocol`
//! [`Transport`] trait (FR G0W08Z).
//!
//! [`SimDevice`] is a stateful software meter with bulk, CDC and HID
//! [`Persona`]s, named [`Scenario`]s, a time source (real time or an injected
//! [`SimClock`]) and a [`FaultPlan`]. It is both the end-to-end test device
//! and the shipped "demo device" (`powerz --transport sim`).
//!
//! Every identifier it reports is synthetic: serial [`SIM_SERIAL`] and a
//! made-up UID; never a real meter's.

mod device;
mod flash;
mod memory;
mod pd;
mod pdm;
mod queue;

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub use flash::{FlashFault, SimMeter, SimMeterPipe};
pub use pd::PdScenario;
pub use pdm::{SimCable, SimCharger, SimPdm, SimPdmPort};
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::Clock;
use powerz_protocol::transport::discovery::{PID_KM003C, UsbIdentity, UsbInterface};
use powerz_protocol::transport::{Error, Transport};

/// Size of one HID report: longer replies "reboot" the HID persona.
const HID_REPORT_LEN: usize = 64;

/// Made-up serial number the simulator reports (OTP block, `list`).
pub const SIM_SERIAL: &str = "123456";

/// Which USB pipe of the meter the simulator imitates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persona {
    /// Vendor bulk interface: everything works; memory read needs CONNECT.
    Bulk,
    /// CDC virtual serial port: types 0x01..0x03 (ADCNEW) get no reply,
    /// otherwise like bulk.
    Cdc,
    /// HID: only single-report GET_DATA is answered; a reply longer than
    /// 64 bytes "reboots" the meter (`Error::Disconnected`).
    Hid,
}

/// One instant of a scenario, in SI units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// VBUS, volts.
    pub vbus_v: f64,
    /// IBUS, amperes (signed).
    pub ibus_a: f64,
    /// CC1, volts.
    pub cc1_v: f64,
    /// CC2, volts.
    pub cc2_v: f64,
    /// D+, volts.
    pub dp_v: f64,
    /// D−, volts.
    pub dm_v: f64,
    /// Chip temperature, °C.
    pub temp_c: f64,
}

/// Named demo scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// Nothing attached: VBUS ≈ 0 V, IBUS ≈ 0 A.
    Idle,
    /// 5 V source, 1 A load.
    Load5v1a,
    /// PD contract at 9 V, 2 A load.
    Pd9v2a,
    /// Noisy current that goes negative.
    NoisyNegative,
}

impl Scenario {
    /// Every scenario, in `--help` order.
    pub const ALL: [Scenario; 4] = [
        Scenario::Idle,
        Scenario::Load5v1a,
        Scenario::Pd9v2a,
        Scenario::NoisyNegative,
    ];

    /// CLI name (`idle`, `load-5v-1a`, `pd-9v-2a`, `noisy-negative`).
    pub fn name(self) -> &'static str {
        match self {
            Scenario::Idle => "idle",
            Scenario::Load5v1a => "load-5v-1a",
            Scenario::Pd9v2a => "pd-9v-2a",
            Scenario::NoisyNegative => "noisy-negative",
        }
    }

    /// Scenario by CLI name.
    pub fn from_name(name: &str) -> Option<Scenario> {
        Scenario::ALL.into_iter().find(|s| s.name() == name)
    }

    /// The scenario's values at simulated device uptime `t_ms` (ms since the
    /// [`SimDevice`] was created). Deterministic: same `t_ms`, same sample.
    pub fn sample(self, t_ms: u64) -> Sample {
        let t = t_ms as f64 / 1000.0;
        let temp_c = 30.0 + 2.0 * (t / 60.0).sin();
        let (vbus_v, ibus_a, cc1_v) = match self {
            Scenario::Idle => (0.0, 0.0, 0.0),
            Scenario::Load5v1a => (
                5.0 + 0.01 * (t * 0.7).sin(),
                1.0 + 0.005 * (t * 1.3).sin(),
                1.68,
            ),
            Scenario::Pd9v2a => (
                9.0 + 0.01 * (t * 0.7).sin(),
                2.0 + 0.01 * (t * 1.3).sin(),
                1.1,
            ),
            Scenario::NoisyNegative => (
                5.0 + 0.02 * (t * 3.1).sin(),
                -0.2 + 0.5 * (t * 7.0).sin() + 0.1 * (t * 23.0).sin(),
                1.68,
            ),
        };
        Sample {
            vbus_v,
            ibus_a,
            cc1_v,
            cc2_v: 0.0,
            dp_v: if vbus_v > 1.0 { 0.6 } else { 0.0 },
            dm_v: if vbus_v > 1.0 { 0.6 } else { 0.0 },
            temp_c,
        }
    }
}

/// Injected simulated time. Clones share one time line; time moves only by
/// [`advance`](SimClock::advance) or a session [`Clock::sleep`].
#[derive(Debug, Clone)]
pub struct SimClock {
    now: Arc<AtomicU64>,
    rng: u64,
}

impl SimClock {
    /// A clock reading `start_unix_ms` (ms since the Unix epoch).
    pub fn new(start_unix_ms: u64) -> SimClock {
        SimClock {
            now: Arc::new(AtomicU64::new(start_unix_ms)),
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Current simulated time, ms since the Unix epoch.
    pub fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }

    /// Move simulated time forward by `d`.
    pub fn advance(&self, d: Duration) {
        self.now.fetch_add(d.as_millis() as u64, Ordering::SeqCst);
    }
}

/// `SimClock` as the session clock: `now_ms` is the simulated time,
/// `sleep` advances it instantly, randomness is deterministic.
impl Clock for SimClock {
    fn now_ms(&mut self) -> u64 {
        SimClock::now_ms(self)
    }
    fn random_u64(&mut self) -> u64 {
        // splitmix64: deterministic per clock clone.
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn sleep(&mut self, duration: Duration) {
        self.advance(duration);
    }
}

/// Faults the simulator injects. Request numbers count `transact` calls
/// from 1 (follow-up `read`s are not counted).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FaultPlan {
    /// Request numbers that get no reply.
    timeout_at: Vec<usize>,
    /// Every request gets no reply.
    timeout_always: bool,
    /// Unplug after this many answered ADC-queue polls.
    disconnect_after_polls: Option<usize>,
    /// Request numbers first answered with a wrong id.
    wrong_id_at: Vec<usize>,
    /// Every request fails with a permission error.
    permission_denied: bool,
}

impl FaultPlan {
    /// No faults.
    pub fn new() -> FaultPlan {
        FaultPlan::default()
    }

    /// Request number `request` gets no reply (`Error::Timeout`).
    pub fn timeout_at(mut self, request: usize) -> FaultPlan {
        self.timeout_at.push(request);
        self
    }

    /// Every request gets no reply (`Error::Timeout`).
    pub fn timeout_always(mut self) -> FaultPlan {
        self.timeout_always = true;
        self
    }

    /// After `polls` answered ADC-queue polls the meter is unplugged: every
    /// later `transact` / `read` fails with `Error::Disconnected`.
    pub fn disconnect_after_polls(mut self, polls: usize) -> FaultPlan {
        self.disconnect_after_polls = Some(polls);
        self
    }

    /// Request number `request` is first answered with a wrong id; the
    /// correct reply follows on the next `read`.
    pub fn wrong_id_at(mut self, request: usize) -> FaultPlan {
        self.wrong_id_at.push(request);
        self
    }

    /// The OS refuses access: every request fails with `Error::Permission`.
    pub fn permission_denied(mut self) -> FaultPlan {
        self.permission_denied = true;
        self
    }

    /// Parse the `POWERZ_SIM_FAULT` syntax: comma-separated `timeout`,
    /// `timeout@N`, `disconnect@N` (after N queue polls), `wrong-id@N`,
    /// `permission`.
    pub fn parse(spec: &str) -> Result<FaultPlan, String> {
        let mut plan = FaultPlan::new();
        for item in spec.split(',').map(str::trim).filter(|i| !i.is_empty()) {
            let (name, arg) = match item.split_once('@') {
                Some((name, n)) => {
                    let n = n
                        .parse::<usize>()
                        .map_err(|_| format!("bad number in fault `{item}`"))?;
                    (name, Some(n))
                }
                None => (item, None),
            };
            plan = match (name, arg) {
                ("timeout", None) => plan.timeout_always(),
                ("timeout", Some(n)) => plan.timeout_at(n),
                ("disconnect", Some(n)) => plan.disconnect_after_polls(n),
                ("wrong-id", Some(n)) => plan.wrong_id_at(n),
                ("permission", None) => plan.permission_denied(),
                _ => {
                    return Err(format!(
                        "unknown fault `{item}` (expected timeout, timeout@N, \
                         disconnect@N, wrong-id@N or permission)"
                    ));
                }
            };
        }
        Ok(plan)
    }
}

/// Where the simulator's uptime comes from.
#[derive(Debug)]
enum TimeSource {
    /// Real time since this instant.
    Real(Instant),
    /// Injected time since `start` ms on `clock`.
    Sim { clock: SimClock, start: u64 },
}

/// The simulated meter.
#[derive(Debug)]
pub struct SimDevice {
    persona: Persona,
    scenario: Scenario,
    time: TimeSource,
    /// Reply transfers waiting on the IN pipe.
    pending: VecDeque<Vec<u8>>,
    /// Session auth level (0, 1 or 2), carried in reply header bits 17..18.
    auth_level: u8,
    /// CONNECT has been accepted.
    connected: bool,
    /// The ADC queue was started with 0x0E.
    streaming: bool,
    /// Meter rate code (SETTINGS); the queue falls back to it after 0x0F.
    rate_code: u8,
    /// The always-running ADC queue.
    queue: queue::AdcQueue,
    /// Injected faults.
    faults: FaultPlan,
    /// `transact` calls so far (request numbers count from 1).
    requests: usize,
    /// Answered ADC-queue polls so far.
    polls: usize,
    /// The meter was "unplugged" by a disconnect fault.
    unplugged: bool,
    /// The PD sniffer (0x10 / 0x11) and its capture scenario.
    pd: pd::PdSniffer,
    /// Application or bootloader mode (sets the USB interface string).
    mode: DeviceMode,
    /// Old-firmware field of the HEAD reply (0 = normal firmware).
    head_field: u32,
    /// Application firmware version in the 0x4420 info block.
    app_version: String,
    /// The simulated bootloader's flash state.
    boot: flash::Bootloader,
    /// Injected bootloader failure, if any.
    flash_fault: Option<FlashFault>,
    /// Jumps take effect at the next power-up, as on 2.0.6 `[device]`,
    /// instead of live.
    jump_at_power_up: bool,
    /// The mode the next power-up starts in, set by such a jump.
    power_up_mode: Option<DeviceMode>,
}

impl SimDevice {
    /// A freshly opened meter running in real time (uptime 0 now).
    pub fn new(persona: Persona, scenario: Scenario) -> SimDevice {
        SimDevice::open(persona, scenario, TimeSource::Real(Instant::now()))
    }

    /// A freshly opened meter on simulated time; uptime 0 is
    /// `clock.now_ms()` at this call.
    pub fn with_clock(persona: Persona, scenario: Scenario, clock: SimClock) -> SimDevice {
        let start = clock.now_ms();
        SimDevice::open(persona, scenario, TimeSource::Sim { clock, start })
    }

    /// A freshly opened meter: on bulk one stale DISCONNECT waits on the IN
    /// endpoint for the first read; CDC and HID start with nothing pending.
    fn open(persona: Persona, scenario: Scenario, time: TimeSource) -> SimDevice {
        let pending = match persona {
            Persona::Bulk => VecDeque::from([device::STALE_DISCONNECT.to_vec()]),
            Persona::Cdc | Persona::Hid => VecDeque::new(),
        };
        SimDevice {
            persona,
            scenario,
            time,
            pending,
            auth_level: 0,
            connected: false,
            streaming: false,
            rate_code: 2,
            queue: queue::AdcQueue::new(2),
            faults: FaultPlan::new(),
            requests: 0,
            polls: 0,
            unplugged: false,
            pd: pd::PdSniffer::default(),
            mode: DeviceMode::Application,
            head_field: 0,
            app_version: memory::SIM_APP_VERSION.to_string(),
            boot: flash::Bootloader::default(),
            flash_fault: None,
            jump_at_power_up: false,
            power_up_mode: None,
        }
    }

    /// Simulated device uptime in ms.
    fn uptime_ms(&self) -> u64 {
        match &self.time {
            TimeSource::Real(t0) => t0.elapsed().as_millis() as u64,
            TimeSource::Sim { clock, start } => clock.now_ms().saturating_sub(*start),
        }
    }

    /// Play `scenario` on the PD sniffer: from each sniffer start (0x10,
    /// mode 1) PD_PACKET polls carry its records. Without a PD scenario the
    /// sniffer captures nothing (status block only).
    pub fn with_pd_scenario(mut self, scenario: PdScenario) -> SimDevice {
        self.pd.set_scenario(scenario);
        self
    }

    /// Run in `mode` (application by default): changes the USB interface
    /// string the simulated meter reports.
    pub fn with_mode(mut self, mode: DeviceMode) -> SimDevice {
        self.mode = mode;
        self
    }

    /// Answer HEAD with old-firmware field `field` (default 0).
    pub fn with_head_field(mut self, field: u32) -> SimDevice {
        self.head_field = field;
        self
    }

    /// Report application firmware `version` in the 0x4420 info block.
    /// The version field is 12 bytes; a longer string is cut to fit.
    pub fn with_app_version(mut self, version: &str) -> SimDevice {
        let mut end = version.len().min(12);
        while !version.is_char_boundary(end) {
            end -= 1;
        }
        self.app_version = version[..end].to_string();
        self
    }

    /// The USB iProduct string: the model name `[device]`.
    pub fn product_string(&self) -> &'static str {
        "POWER-Z KM003C"
    }

    /// The interface string of the vendor interface (string 2): `APP` in
    /// application mode `[device]`, `BOOT` in bootloader mode `[inferred]`.
    pub fn interface_string(&self) -> &'static str {
        match self.mode {
            DeviceMode::Application => "APP",
            DeviceMode::Bootloader => "BOOT",
            DeviceMode::Unknown => "POWER-Z",
        }
    }

    /// The USB identity the simulated meter reports `[inferred]`: in
    /// application mode the KM003C's four interfaces (vendor 0xFF and CDC
    /// 0x02 named `APP`, CDC data 0x0A, HID 0x03) `[device]`; in bootloader
    /// mode a single vendor interface named `BOOT`.
    pub fn usb_identity(&self) -> UsbIdentity {
        let iface = |number: u8, class: u8, string: Option<&str>| UsbInterface {
            number,
            class,
            subclass: 0,
            protocol: 0,
            string: string.map(str::to_owned),
        };
        let vendor = self.interface_string();
        let interfaces = match self.mode {
            DeviceMode::Bootloader => vec![iface(0, 0xFF, Some(vendor))],
            DeviceMode::Application | DeviceMode::Unknown => vec![
                iface(0, 0xFF, Some(vendor)),
                iface(1, 0x02, Some(vendor)),
                iface(2, 0x0A, None),
                iface(3, 0x03, None),
            ],
        };
        UsbIdentity {
            vid: powerz_protocol::VID,
            pid: PID_KM003C,
            product: Some(self.product_string().to_owned()),
            interfaces,
        }
    }

    /// Inject `faults`.
    pub fn with_faults(mut self, faults: FaultPlan) -> SimDevice {
        self.faults = faults;
        self
    }

    /// The error every transfer fails with, if any (permission, unplugged).
    fn blocked(&self) -> Option<Error> {
        if self.faults.permission_denied {
            Some(Error::Permission(
                "access to the simulated meter denied".into(),
            ))
        } else if self.unplugged {
            Some(Error::Disconnected)
        } else {
            None
        }
    }
}

impl Transport for SimDevice {
    fn transact(&mut self, request: &[u8], _timeout: Duration) -> Result<Vec<u8>, Error> {
        if let Some(e) = self.blocked() {
            return Err(e);
        }
        self.requests += 1;
        let n = self.requests;
        if self.faults.timeout_always || self.faults.timeout_at.contains(&n) {
            return Err(Error::Timeout);
        }
        let is_poll = device::is_queue_poll(request);
        if self.persona == Persona::Hid && !powerz_protocol::frame::is_get_data(request) {
            // Over HID the meter neither answers nor acts on anything but
            // GET_DATA.
            return Err(Error::Timeout);
        }
        let mut replies = self.handle(request);
        if self.persona == Persona::Hid {
            // A reply longer than one report crashes the meter's HID stack
            // ("reboot").
            if replies.iter().any(|r| r.len() > HID_REPORT_LEN) {
                self.unplugged = true;
                self.pending.clear();
                return Err(Error::Disconnected);
            }
        }
        if is_poll {
            self.polls += 1;
            if self.faults.disconnect_after_polls == Some(self.polls) {
                self.unplugged = true;
            }
        }
        if self.faults.wrong_id_at.contains(&n)
            && let Some(first) = replies.first()
        {
            let mut wrong = first.clone();
            if let Some(id) = wrong.get_mut(1) {
                *id ^= 0x80;
            }
            replies.insert(0, wrong);
        }
        self.pending.extend(replies);
        // The reply to this request is delivered even if it was the last
        // poll before an unplug.
        self.pending.pop_front().ok_or(Error::Timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, Error> {
        if let Some(e) = self.blocked() {
            return Err(e);
        }
        match self.pending.pop_front() {
            Some(reply) => Ok(reply),
            None if self.mode == DeviceMode::Bootloader => self.bootloader_read(timeout),
            None => Err(Error::Timeout),
        }
    }
}

/// The HID persona as a raw HID device, so the library's `HidTransport`
/// (and its guard) can sit on top of the simulator.
impl powerz_protocol::transport::hid::HidIo for SimDevice {
    /// One output report (report ID first): handled as one request; its
    /// reply, if any, waits for the next input-report read.
    fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
        let frame = data.get(1..).unwrap_or_default();
        match Transport::transact(self, frame, Duration::ZERO) {
            Ok(reply) => self.pending.push_front(reply),
            Err(Error::Timeout) => {}
            Err(e) => return Err(e),
        }
        Ok(data.len())
    }

    /// One input report; `Ok(0)` when nothing is pending.
    fn read_timeout(&mut self, buf: &mut [u8], _timeout: Duration) -> Result<usize, Error> {
        if let Some(e) = self.blocked() {
            return Err(e);
        }
        let Some(report) = self.pending.pop_front() else {
            return Ok(0);
        };
        let n = report.len().min(buf.len());
        buf[..n].copy_from_slice(&report[..n]);
        Ok(n)
    }
}

#[cfg(test)]
mod persona_tests {
    use super::*;

    #[test]
    fn hid_control_commands_have_no_effect_on_state() {
        // The real meter ignores control commands over HID (no reply, no
        // effect); the HID persona must not act on them either.
        let mut dev = SimDevice::new(Persona::Hid, Scenario::Idle);
        let _ = dev.transact(
            &powerz_protocol::frame::connect(1),
            Duration::from_millis(20),
        );
        assert!(!dev.connected);
    }
}
