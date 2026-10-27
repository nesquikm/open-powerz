//! `powerz read`.

use std::io::Write;

use powerz_protocol::session::{Clock, Session};
use powerz_protocol::transport::Transport;

use crate::error::CliError;
use crate::output::{Reading, adc_json, write_adc_lines, write_json};

/// `powerz read`: one ADC snapshot. Human output has one labelled value per
/// line (VBUS, IBUS, power, temperature, D+, D−, CC1, CC2) with at least 3
/// decimals; JSON has `vbus_v`, `ibus_a`, `power_w`, `temp_c` (null when
/// invalid), `dp_v`, `dm_v`, `cc1_v`, `cc2_v`.
pub fn read<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let a = session.read_adc()?;
    if json {
        write_json(out, &adc_json(&a))
    } else {
        write_adc_lines(out, &Reading::from(&a))
    }
}
