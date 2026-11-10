//! The PD analyzer view's backend side (FR TGE83C). The capture itself lives
//! in `powerz_protocol::pd_capture`, shared with the web edition (FR EAKFEC);
//! this module adds the cable identity in the trigger panel's shape.

pub use powerz_protocol::pd_capture::*;

use crate::trigger::{CableView, cable_view};

/// The newest cable identity of a capture as the trigger panel shows it.
pub trait CableSeen {
    /// The newest cable Discover Identity ACK the capture keeps (FR AXHK2P).
    fn latest_cable_view(&self) -> Option<CableView>;
}

impl CableSeen for PdCapture {
    fn latest_cable_view(&self) -> Option<CableView> {
        self.latest_cable().map(|c| cable_view(&c))
    }
}
