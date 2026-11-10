//! The demo meter (AC-QFTCWY.3): the simulator's bulk persona with the PD
//! negotiation script, on a clock only the caller moves.

use std::time::Duration;

use powerz_protocol::transport::Transport;
use powerz_sim::{PdScenario, Persona, Scenario, SimDevice};
use wasm_bindgen::prelude::*;

use crate::error::BindError;

/// A simulated KM003C the web page drives: `transact` sends one request and
/// returns every reply transfer joined; `advance` moves simulated time.
#[wasm_bindgen]
#[derive(Debug)]
pub struct DemoMeter {
    sim: SimDevice,
}

#[wasm_bindgen]
impl DemoMeter {
    /// A freshly opened demo meter running `scenario` (`idle`, `load-5v-1a`,
    /// `pd-9v-2a`, `noisy-negative`) at uptime 0.
    #[wasm_bindgen(constructor)]
    pub fn new(scenario: &str) -> Result<DemoMeter, BindError> {
        let scenario = Scenario::from_name(scenario)
            .ok_or_else(|| BindError::invalid(format!("unknown demo scenario `{scenario}`")))?;
        let mut sim =
            SimDevice::manual(Persona::Bulk, scenario).with_pd_scenario(PdScenario::Negotiation);
        // The stale DISCONNECT that waits right after open is not a reply.
        while sim.read(Duration::ZERO).is_ok() {}
        Ok(DemoMeter { sim })
    }

    /// Send one request; returns every reply transfer joined (empty when the
    /// meter does not answer).
    pub fn transact(&mut self, request: &[u8]) -> Vec<u8> {
        transact_joined(&mut self.sim, request)
    }

    /// Move simulated time forward by `ms` milliseconds.
    pub fn advance(&mut self, ms: u32) {
        self.sim.advance(u64::from(ms));
    }

    /// This demo meter's serial port (FR W8SHX9): the simulator's CDC
    /// persona on the same clock, sharing the stored settings, so a
    /// settings write there is what this meter serves afterwards.
    #[wasm_bindgen(js_name = cdcTwin)]
    pub fn cdc_twin(&self) -> DemoCdc {
        let mut sim = self.sim.cdc_twin();
        // Whatever waits right after open is not a reply.
        while sim.read(Duration::ZERO).is_ok() {}
        DemoCdc { sim }
    }
}

/// The demo meter's serial port: `transact` and `advance` as on
/// [`DemoMeter`].
#[wasm_bindgen]
#[derive(Debug)]
pub struct DemoCdc {
    sim: SimDevice,
}

#[wasm_bindgen]
impl DemoCdc {
    /// Send one request; returns every reply transfer joined (empty when the
    /// meter does not answer).
    pub fn transact(&mut self, request: &[u8]) -> Vec<u8> {
        transact_joined(&mut self.sim, request)
    }

    /// Move simulated time forward by `ms` milliseconds.
    pub fn advance(&mut self, ms: u32) {
        self.sim.advance(u64::from(ms));
    }
}

fn transact_joined(sim: &mut SimDevice, request: &[u8]) -> Vec<u8> {
    let mut reply = sim.transact(request, Duration::ZERO).unwrap_or_default();
    while let Ok(more) = sim.read(Duration::ZERO) {
        reply.extend(more);
    }
    reply
}
