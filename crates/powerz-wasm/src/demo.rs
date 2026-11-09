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
        let mut reply = self
            .sim
            .transact(request, Duration::ZERO)
            .unwrap_or_default();
        while let Ok(more) = self.sim.read(Duration::ZERO) {
            reply.extend(more);
        }
        reply
    }

    /// Move simulated time forward by `ms` milliseconds.
    pub fn advance(&mut self, ms: u32) {
        self.sim.advance(u64::from(ms));
    }
}
