//! Legacy and vendor protocol triggering over a scripted PDM port
//! (FR DGSQ6Q).
//!
//! Unsupported-charger replies are rewritten from the device-confirmed
//! session-2 transcript (`entry qc` → `ok\ntimeout\n`, `entry ufcs` → three
//! `timeout`s, `entry vfcp` → `ok\nready\n` with no block). Replies of a
//! supported protocol (`ready`, the `scp:` / `tfcp:` / `ufcs:` blocks) are
//! synthetic, built from the \[static\] layouts: no such charger has been
//! seen on the device.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::time::Duration;

use powerz_protocol::pdm::legacy::{self, Error as LegacyError, LegacyProtocol, Limits, Request};
use powerz_protocol::pdm::{Error, FixedProtocol, HighVoltageConfirmation, PdmSession};
use powerz_protocol::session::Clock;

const T0: u64 = 1_790_000_000_000;
const OPEN_REPLY: &[u8] = b"pdm mode entry\nver1.0\n";

#[derive(Clone, Debug)]
struct FakeClock(Rc<Cell<u64>>);

impl Clock for FakeClock {
    fn now_ms(&mut self) -> u64 {
        self.0.get()
    }
    fn random_u64(&mut self) -> u64 {
        4
    }
    fn sleep(&mut self, d: Duration) {
        self.0.set(self.0.get() + d.as_millis() as u64);
    }
}

#[derive(Default)]
struct State {
    script: HashMap<Vec<u8>, VecDeque<Vec<u8>>>,
    writes: Vec<Vec<u8>>,
    pending: VecDeque<u8>,
}

/// Scripted serial port; clones share state.
#[derive(Clone, Default)]
struct FakePort {
    state: Rc<RefCell<State>>,
}

impl FakePort {
    fn on(self, cmd: &str, reply: &[u8]) -> FakePort {
        self.state
            .borrow_mut()
            .script
            .entry(cmd.as_bytes().to_vec())
            .or_default()
            .push_back(reply.to_vec());
        self
    }
    fn writes(&self) -> Vec<String> {
        self.state
            .borrow()
            .writes
            .iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect()
    }
    /// Writes after `pdm open`.
    fn sent(&self) -> Vec<String> {
        self.writes().into_iter().skip(1).collect()
    }
}

impl std::fmt::Debug for FakePort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FakePort")
    }
}

impl Read for FakePort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut s = self.state.borrow_mut();
        if s.pending.is_empty() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "fake timeout"));
        }
        // Small chunks: blocks and lines arrive split.
        let n = buf.len().min(s.pending.len()).min(7);
        for b in buf.iter_mut().take(n) {
            *b = s.pending.pop_front().unwrap();
        }
        Ok(n)
    }
}

impl Write for FakePort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut s = self.state.borrow_mut();
        s.writes.push(buf.to_vec());
        let reply = s.script.get_mut(buf).and_then(|q| {
            if q.len() > 1 {
                q.pop_front()
            } else {
                q.front().cloned()
            }
        });
        if let Some(r) = reply {
            s.pending.extend(r);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn open(port: &FakePort) -> PdmSession<FakePort, FakeClock> {
    PdmSession::open_with_clock(port.clone(), FakeClock(Rc::new(Cell::new(T0)))).expect("pdm open")
}

fn base() -> FakePort {
    FakePort::default()
        .on("pdm open", OPEN_REPLY)
        .on("reset", b"ok\n")
        .on("pdm close", b"ok\n")
}

/// Synthetic `scp:` block: 40 W, 3.4-20 V, 0.1-4 A, 10 mV / 50 mA steps,
/// bound 11 V / 5 A, with its `\n`.
fn scp_block() -> Vec<u8> {
    let mut b = b"scp:".to_vec();
    for v in [400u16, 3400, 20000, 100, 4000, 10, 50, 11000, 5000] {
        b.extend(v.to_le_bytes());
    }
    b.push(b'\n');
    b
}

/// Synthetic `tfcp:` block: 60 W, 5-20 V, 0.1-3 A, with its `\n`.
fn tfcp_block() -> Vec<u8> {
    let mut b = b"tfcp:".to_vec();
    b.push(0);
    b.extend(600u32.to_le_bytes());
    for v in [5000u16, 100, 20000, 3000] {
        b.extend(v.to_le_bytes());
    }
    b.push(b'\n');
    b
}

/// Synthetic `vfcp:` block: 18 W, 9 V 2 A / 12 V 1.5 A, a short tail, `\n`.
fn vfcp_block() -> Vec<u8> {
    let mut b = b"vfcp:".to_vec();
    b.push(0);
    for v in [7000u16, 9000, 12000, 2000, 1500, 180] {
        b.extend(v.to_le_bytes());
    }
    b.extend([0x04, 0x00, 0x01, 0x00, 0x0D, 0x20, 0xA6, 0x00]);
    b.push(b'\n');
    b
}

/// Synthetic UFCS entry: `index`, `min`-`max` mV (10 mV units), 0.1-`max_ma`
/// mA, 10 mV / 50 mA steps.
fn ufcs_entry(index: u64, min_mv: u64, max_mv: u64, max_ma: u64) -> u64 {
    (index << 60)
        | (4 << 57)
        | ((max_mv / 10) << 40)
        | ((min_mv / 10) << 24)
        | ((max_ma / 10) << 8)
        | 10
}

/// Synthetic `ufcs:` block: #1 3.4-11 V 5 A, #2 3.4-21 V 3 A, with its `\n`.
fn ufcs_block() -> Vec<u8> {
    let mut b = b"ufcs:".to_vec();
    b.push(2);
    b.extend(ufcs_entry(1, 3400, 11000, 5000).to_le_bytes());
    b.extend(ufcs_entry(2, 3400, 21000, 3000).to_le_bytes());
    b.push(b'\n');
    b
}

fn reply(prefix: &[u8], block: Vec<u8>) -> Vec<u8> {
    let mut b = prefix.to_vec();
    b.extend(block);
    b
}

/// A charger that speaks every protocol here. [static] reply forms.
fn supporting_port() -> FakePort {
    base()
        .on("entry qc", b"ok\nready\n")
        .on("entry fcp", b"ok\nready\n")
        .on("entry afc", b"ok\nready\n")
        .on("entry sfcp", b"ok\nready\n")
        .on("entry scp", &reply(b"ok\nready\n", scp_block()))
        .on("entry vfcp", &reply(b"ok\nready\n", vfcp_block()))
        .on("entry tfcp", &reply(b"ok\nready\n", tfcp_block()))
        .on("entry ufcs", &reply(b"ok\nready\n", ufcs_block()))
        .on("qc 5V", b"ready\n")
        .on("qc 9V", b"ready\n")
        .on("qc 20V", b"ready\n")
        .on("qc3 volt=9000", b"ready\n")
        .on("qc3 inc=2", b"ready\n")
        .on("qc3 dec=1", b"ready\n")
        .on("fcp 9V", b"ready\n")
        .on("afc 12V", b"ready\n")
        .on("sfcp 9V", b"ready\n")
        .on("scp volt=9000,cur=2000", b"ready\n")
        .on("vfcp volt=9000,cur=1000", b"ready\n")
        .on("tfcp volt=12000,cur=2000", b"ready\n")
        .on("ufcs req=2,volt=15000,cur=2000", b"ready\n")
        .on("ufcs cmd=6", b"ready\n")
}

/// The session-2 charger: every entry here times out (VFCP answers
/// `ready` without a block). [device]
fn unsupporting_port() -> FakePort {
    base()
        .on("entry qc", b"ok\ntimeout\n")
        .on("entry fcp", b"ok\ntimeout\n")
        .on("entry scp", b"ok\ntimeout\n")
        .on("entry afc", b"ok\ntimeout\n")
        .on("entry sfcp", b"ok\ntimeout\n")
        .on("entry tfcp", b"ok\ntimeout\n")
        .on("entry ufcs", b"ok\ntimeout\ntimeout\ntimeout\n")
        .on("entry vfcp", b"ok\nready\n")
        .on("ufcs pdo", b"maw power 0.0W\n\n")
}

/// Enter `p` on `port`, then send `req` with a confirmation made for its
/// rendered command.
fn enter_and_confirm(
    port: &FakePort,
    p: LegacyProtocol,
    req: Request,
) -> Result<legacy::Outcome, LegacyError> {
    let mut s = open(port);
    legacy::enter(&mut s, p)?;
    let cmd = legacy::validate(s.capabilities(), &req)?;
    legacy::request(
        &mut s,
        &req,
        Some(HighVoltageConfirmation::for_command(&cmd)),
    )
}

mod pdm_legacy {
    use super::*;

    // ---- AC-DGSQ6Q.1: QC 2.0 and QC 3.0 ----

    #[test]
    fn qc2_requests_go_out_after_entry_qc() {
        for (v, cmd) in [(5u8, "qc 5V"), (9, "qc 9V"), (20, "qc 20V")] {
            let port = supporting_port();
            let req = Request::Fixed {
                protocol: FixedProtocol::Qc,
                volts: v,
            };
            let out = enter_and_confirm(&port, LegacyProtocol::Qc, req).expect(cmd);
            assert_eq!(out.command.render(), cmd.as_bytes());
            assert!(
                out.reply.as_deref().is_some_and(|r| r.starts_with("ready")),
                "{out:?}"
            );
            // The helper drops the session, which closes it (AC-PR41FT.1).
            assert_eq!(port.sent(), ["entry qc", cmd, "pdm close"], "{v} V");
        }
    }

    #[test]
    fn qc3_voltage_and_steps() {
        let port = supporting_port();
        let mut s = open(&port);
        let e = legacy::enter(&mut s, LegacyProtocol::Qc).unwrap();
        assert!(e.limits.is_empty());
        let volt = Request::Qc3Volt { voltage_mv: 9000 };
        let cmd = legacy::validate(s.capabilities(), &volt).unwrap();
        legacy::request(
            &mut s,
            &volt,
            Some(HighVoltageConfirmation::for_command(&cmd)),
        )
        .unwrap();
        let inc = Request::Qc3Inc(2);
        let cmd = legacy::validate(s.capabilities(), &inc).unwrap();
        legacy::request(
            &mut s,
            &inc,
            Some(HighVoltageConfirmation::for_command(&cmd)),
        )
        .unwrap();
        // Stepping down needs no confirmation.
        legacy::request(&mut s, &Request::Qc3Dec(1), None).unwrap();
        assert_eq!(
            port.sent(),
            ["entry qc", "qc3 volt=9000", "qc3 inc=2", "qc3 dec=1"]
        );
    }

    #[test]
    fn qc_above_5v_without_confirmation_writes_nothing() {
        let port = supporting_port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Qc).unwrap();
        let err = legacy::request(
            &mut s,
            &Request::Fixed {
                protocol: FixedProtocol::Qc,
                volts: 9,
            },
            None,
        )
        .unwrap_err();
        assert!(
            matches!(err, LegacyError::Pdm(Error::HighVoltageNotConfirmed { .. })),
            "{err:?}"
        );
        // `qc3 inc` is always treated as above 5 V.
        let err = legacy::request(&mut s, &Request::Qc3Inc(1), None).unwrap_err();
        assert!(
            matches!(err, LegacyError::Pdm(Error::HighVoltageNotConfirmed { .. })),
            "{err:?}"
        );
        assert_eq!(port.sent(), ["entry qc"]);
    }

    #[test]
    fn qc_5v_needs_no_confirmation() {
        let port = supporting_port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Qc).unwrap();
        legacy::request(
            &mut s,
            &Request::Fixed {
                protocol: FixedProtocol::Qc,
                volts: 5,
            },
            None,
        )
        .unwrap();
        assert_eq!(port.sent(), ["entry qc", "qc 5V"]);
    }

    #[test]
    fn invalid_qc3_voltage_writes_nothing() {
        let port = supporting_port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Qc).unwrap();
        let err =
            legacy::request(&mut s, &Request::Qc3Volt { voltage_mv: 9100 }, None).unwrap_err();
        assert!(
            matches!(err, LegacyError::Qc3Voltage { voltage_mv: 9100 }),
            "{err:?}"
        );
        assert_eq!(port.sent(), ["entry qc"]);
    }

    // ---- AC-DGSQ6Q.2: FCP, AFC, SFCP ----

    #[test]
    fn fcp_afc_sfcp_requests_go_out_after_their_entry() {
        let cases = [
            (
                LegacyProtocol::Fcp,
                FixedProtocol::Fcp,
                9u8,
                "entry fcp",
                "fcp 9V",
            ),
            (
                LegacyProtocol::Afc,
                FixedProtocol::Afc,
                12,
                "entry afc",
                "afc 12V",
            ),
            (
                LegacyProtocol::Sfcp,
                FixedProtocol::Sfcp,
                9,
                "entry sfcp",
                "sfcp 9V",
            ),
        ];
        for (p, fp, volts, entry, cmd) in cases {
            let port = supporting_port();
            let req = Request::Fixed {
                protocol: fp,
                volts,
            };
            let out = enter_and_confirm(&port, p, req).expect(cmd);
            assert_eq!(out.command.render(), cmd.as_bytes());
            assert_eq!(port.sent(), [entry, cmd, "pdm close"]);
        }
    }

    #[test]
    fn fcp_20v_is_refused_before_writing() {
        let port = supporting_port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Fcp).unwrap();
        let req = Request::Fixed {
            protocol: FixedProtocol::Fcp,
            volts: 20,
        };
        let cmd = powerz_protocol::pdm::Command::FixedVoltage {
            protocol: FixedProtocol::Fcp,
            volts: 20,
        };
        let err = legacy::request(
            &mut s,
            &req,
            Some(HighVoltageConfirmation::for_command(&cmd)),
        )
        .unwrap_err();
        assert!(
            matches!(err, LegacyError::FixedVoltage { volts: 20, .. }),
            "{err:?}"
        );
        assert_eq!(port.sent(), ["entry fcp"]);
    }

    // ---- AC-DGSQ6Q.3: SCP / VFCP / TFCP capability blocks and requests ----

    #[test]
    fn entry_scp_reads_and_decodes_its_block() {
        let port = supporting_port();
        let mut s = open(&port);
        let e = legacy::enter(&mut s, LegacyProtocol::Scp).unwrap();
        assert_eq!(e.protocol, LegacyProtocol::Scp);
        assert_eq!(
            e.limits,
            [Limits {
                index: None,
                min_voltage_mv: 3400,
                max_voltage_mv: 20_000,
                min_current_ma: 100,
                max_current_ma: 4000,
                max_power_mw: Some(40_000),
            }]
        );
    }

    #[test]
    fn entry_vfcp_and_tfcp_decode_their_blocks() {
        let port = supporting_port();
        let mut s = open(&port);
        let e = legacy::enter(&mut s, LegacyProtocol::Tfcp).unwrap();
        assert_eq!(e.limits.len(), 1);
        assert_eq!(e.limits[0].min_voltage_mv, 5000);
        assert_eq!(e.limits[0].max_voltage_mv, 20_000);
        assert_eq!(e.limits[0].max_current_ma, 3000);
        assert_eq!(e.limits[0].max_power_mw, Some(60_000));
        let e = legacy::enter(&mut s, LegacyProtocol::Vfcp).unwrap();
        assert_eq!(e.limits.len(), 1);
        assert_eq!(e.limits[0].max_voltage_mv, 12_000);
        assert_eq!(e.limits[0].max_power_mw, Some(18_000));
    }

    #[test]
    fn scp_vfcp_tfcp_requests_inside_the_limits_are_sent() {
        let cases = [
            (
                LegacyProtocol::Scp,
                Request::Scp {
                    voltage_mv: 9000,
                    current_ma: 2000,
                },
                "scp volt=9000,cur=2000",
            ),
            (
                LegacyProtocol::Vfcp,
                Request::Vfcp {
                    voltage_mv: 9000,
                    current_ma: 1000,
                },
                "vfcp volt=9000,cur=1000",
            ),
            (
                LegacyProtocol::Tfcp,
                Request::Tfcp {
                    voltage_mv: 12_000,
                    current_ma: 2000,
                },
                "tfcp volt=12000,cur=2000",
            ),
        ];
        for (p, req, cmd) in cases {
            let port = supporting_port();
            let out = enter_and_confirm(&port, p, req).expect(cmd);
            assert_eq!(out.command.render(), cmd.as_bytes());
            let sent = port.sent();
            assert_eq!(sent.iter().rev().nth(1).map(String::as_str), Some(cmd));
            assert_eq!(sent.last().map(String::as_str), Some("pdm close"));
        }
    }

    #[test]
    fn scp_request_outside_the_limits_writes_nothing() {
        let port = supporting_port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Scp).unwrap();
        let req = Request::Scp {
            voltage_mv: 21_000,
            current_ma: 1000,
        };
        let cmd = powerz_protocol::pdm::Command::ScpRequest {
            voltage_mv: 21_000,
            current_ma: 1000,
        };
        let err = legacy::request(
            &mut s,
            &req,
            Some(HighVoltageConfirmation::for_command(&cmd)),
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                LegacyError::VoltageOutOfRange {
                    voltage_mv: 21_000,
                    ..
                }
            ),
            "{err:?}"
        );
        assert_eq!(port.sent(), ["entry scp"]);
    }

    // ---- AC-DGSQ6Q.4: UFCS ----

    #[test]
    fn entry_ufcs_lists_the_capabilities_by_number() {
        let port = supporting_port();
        let mut s = open(&port);
        let e = legacy::enter(&mut s, LegacyProtocol::Ufcs).unwrap();
        let idx: Vec<Option<u8>> = e.limits.iter().map(|l| l.index).collect();
        assert_eq!(idx, [Some(1), Some(2)]);
        assert_eq!(e.limits[0].max_voltage_mv, 11_000);
        assert_eq!(e.limits[0].max_current_ma, 5000);
        assert_eq!(e.limits[1].max_voltage_mv, 21_000);
        assert_eq!(e.limits[1].max_current_ma, 3000);
    }

    #[test]
    fn ufcs_request_by_number_and_control_command() {
        let port = supporting_port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Ufcs).unwrap();
        // 15 V is inside #2 but outside #1.
        let bad = Request::Ufcs {
            index: 1,
            voltage_mv: 15_000,
            current_ma: 2000,
        };
        let err = legacy::request(&mut s, &bad, None).unwrap_err();
        assert!(
            matches!(
                err,
                LegacyError::VoltageOutOfRange {
                    voltage_mv: 15_000,
                    max_mv: 11_000,
                    ..
                }
            ),
            "{err:?}"
        );
        let good = Request::Ufcs {
            index: 2,
            voltage_mv: 15_000,
            current_ma: 2000,
        };
        let cmd = legacy::validate(s.capabilities(), &good).unwrap();
        legacy::request(
            &mut s,
            &good,
            Some(HighVoltageConfirmation::for_command(&cmd)),
        )
        .unwrap();
        // UFCS control numbers are not proven safe, so they need a
        // confirmation like any request above 5 V (AC-PR41FT.4).
        assert!(legacy::request(&mut s, &Request::UfcsCmd(6), None).is_err());
        let ctl = legacy::validate(s.capabilities(), &Request::UfcsCmd(6)).unwrap();
        let out = legacy::request(
            &mut s,
            &Request::UfcsCmd(6),
            Some(HighVoltageConfirmation::for_command(&ctl)),
        )
        .unwrap();
        assert_eq!(out.command.render(), b"ufcs cmd=6");
        assert_eq!(
            port.sent(),
            ["entry ufcs", "ufcs req=2,volt=15000,cur=2000", "ufcs cmd=6"]
        );
    }

    // ---- AC-DGSQ6Q.5: unsupported protocols ----

    #[test]
    fn timed_out_entries_are_not_supported() {
        for p in [
            LegacyProtocol::Qc,
            LegacyProtocol::Fcp,
            LegacyProtocol::Scp,
            LegacyProtocol::Afc,
            LegacyProtocol::Sfcp,
            LegacyProtocol::Tfcp,
            LegacyProtocol::Ufcs,
        ] {
            let port = unsupporting_port();
            let mut s = open(&port);
            let err = legacy::enter(&mut s, p).unwrap_err();
            assert!(
                matches!(err, LegacyError::NotSupported { protocol } if protocol == p),
                "{p:?}: {err:?}"
            );
            assert!(err.to_string().contains("does not support"), "{err}");
            assert_eq!(port.sent(), [format!("entry {}", p.name())], "{p:?}");
        }
    }

    #[test]
    fn vfcp_ready_without_a_block_is_not_supported() {
        let port = unsupporting_port();
        let mut s = open(&port);
        let err = legacy::enter(&mut s, LegacyProtocol::Vfcp).unwrap_err();
        assert!(
            matches!(
                err,
                LegacyError::NotSupported {
                    protocol: LegacyProtocol::Vfcp
                }
            ),
            "{err:?}"
        );
        // And with no block known, a request is refused before writing.
        let req = Request::Vfcp {
            voltage_mv: 9000,
            current_ma: 1000,
        };
        let cmd = powerz_protocol::pdm::Command::VfcpRequest {
            voltage_mv: 9000,
            current_ma: 1000,
        };
        let err = legacy::request(
            &mut s,
            &req,
            Some(HighVoltageConfirmation::for_command(&cmd)),
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                LegacyError::NoCapabilities {
                    protocol: LegacyProtocol::Vfcp
                }
            ),
            "{err:?}"
        );
        assert_eq!(port.sent(), ["entry vfcp"]);
    }

    #[test]
    fn the_ufcs_timeouts_do_not_leak_into_the_next_reply() {
        // After the three timeouts, `reset` still finds its own `ok`.
        let port = unsupporting_port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Ufcs).unwrap_err();
        legacy::return_to_5v(&mut s).unwrap();
        assert_eq!(port.sent(), ["entry ufcs", "reset"]);
    }

    // ---- AC-DGSQ6Q.6: the way back to 5 V ----

    #[test]
    fn return_to_5v_sends_reset_and_waits_for_ok() {
        let port = supporting_port();
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Qc).unwrap();
        legacy::return_to_5v(&mut s).unwrap();
        assert_eq!(port.sent(), ["entry qc", "reset"]);
    }

    #[test]
    fn return_to_5v_without_ok_is_an_error() {
        let port = FakePort::default()
            .on("pdm open", OPEN_REPLY)
            .on("entry qc", b"ok\nready\n")
            .on("pdm close", b"ok\n");
        let mut s = open(&port);
        legacy::enter(&mut s, LegacyProtocol::Qc).unwrap();
        let err = legacy::return_to_5v(&mut s).unwrap_err();
        assert!(
            matches!(err, LegacyError::Pdm(Error::Timeout { .. })),
            "{err:?}"
        );
    }
}
