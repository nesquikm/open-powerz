//! PDM text commands and their exact ASCII rendering (no terminator).
//!
//! Commands are lowercase ASCII, numbers in decimal, units mV / mA; the
//! firmware matches prefixes and is case-sensitive. \[doc\] \[static\] \[device\]

/// Protocols the meter can enter with `entry <name>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// `entry pd`
    Pd,
    /// `entry ufcs`
    Ufcs,
    /// `entry qc`
    Qc,
    /// `entry fcp`
    Fcp,
    /// `entry scp`
    Scp,
    /// `entry afc`
    Afc,
    /// `entry vfcp`
    Vfcp,
    /// `entry sfcp`
    Sfcp,
    /// `entry tfcp`
    Tfcp,
    /// `entry bc` (BC1.2 detection)
    Bc,
    /// `entry apple` (Apple detection)
    Apple,
    /// `entry list` (automatic detection)
    List,
    /// `entry list+` (automatic detection incl. VOOC / MTK PE)
    ListPlus,
}

/// Protocols with fixed-voltage requests of the form `<name> <n>V`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedProtocol {
    /// `qc <n>V` (QC 2.0)
    Qc,
    /// `fcp <n>V`
    Fcp,
    /// `afc <n>V`
    Afc,
    /// `sfcp <n>V`
    Sfcp,
}

/// One PDM text command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `pdm open`
    PdmOpen,
    /// `pdm close`
    PdmClose,
    /// `reset`
    Reset,
    /// `pdm parm`
    PdmParm,
    /// `pdm set type=<ty>,em=<em>,sink=<sink>`
    PdmSet {
        /// PD type.
        ty: u8,
        /// e-marker emulation.
        em: u8,
        /// Sink capabilities emulation.
        sink: u8,
    },
    /// `entry <protocol>`
    Entry(Protocol),
    /// `pd pdo`
    PdPdo,
    /// `ufcs pdo`
    UfcsPdo,
    /// `pd hrst`
    PdHardReset,
    /// `pd req=<pos>[,volt=<mV>][,cur=<mA>]` (volt only with cur).
    PdRequest {
        /// 1-based object position (slot).
        position: u8,
        /// Requested voltage (PPS / AVS), mV.
        voltage_mv: Option<u32>,
        /// Requested current, mA.
        current_ma: Option<u32>,
    },
    /// `pd cmd=<n>`
    PdCmd(u8),
    /// `pd data=<HEX>` (uppercase hex: SOP, header LE, data objects LE).
    PdData(Vec<u8>),
    /// `<qc|fcp|afc|sfcp> <n>V`
    FixedVoltage {
        /// Protocol.
        protocol: FixedProtocol,
        /// Volts.
        volts: u8,
    },
    /// `qc3 volt=<mV>`
    Qc3Volt {
        /// Voltage, mV.
        voltage_mv: u32,
    },
    /// `qc3 inc=<n>`
    Qc3Inc(u32),
    /// `qc3 dec=<n>`
    Qc3Dec(u32),
    /// `scp volt=<mV>,cur=<mA>`
    ScpRequest {
        /// Voltage, mV.
        voltage_mv: u32,
        /// Current, mA.
        current_ma: u32,
    },
    /// `vfcp volt=<mV>,cur=<mA>`
    VfcpRequest {
        /// Voltage, mV.
        voltage_mv: u32,
        /// Current, mA.
        current_ma: u32,
    },
    /// `tfcp volt=<mV>,cur=<mA>`
    TfcpRequest {
        /// Voltage, mV.
        voltage_mv: u32,
        /// Current, mA.
        current_ma: u32,
    },
    /// `ufcs req=<n>,volt=<mV>,cur=<mA>`
    UfcsRequest {
        /// 1-based capability index.
        index: u8,
        /// Voltage, mV.
        voltage_mv: u32,
        /// Current, mA.
        current_ma: u32,
    },
    /// `ufcs cmd=<n>`
    UfcsCmd(u8),
    /// Any other text, sent as-is.
    Raw(String),
}

impl Command {
    /// The exact bytes to write: ASCII, no terminator.
    pub fn render(&self) -> Vec<u8> {
        self.text().into_bytes()
    }

    fn text(&self) -> String {
        match self {
            Command::PdmOpen => "pdm open".into(),
            Command::PdmClose => "pdm close".into(),
            Command::Reset => "reset".into(),
            Command::PdmParm => "pdm parm".into(),
            Command::PdmSet { ty, em, sink } => format!("pdm set type={ty},em={em},sink={sink}"),
            Command::Entry(p) => format!("entry {}", p.name()),
            Command::PdPdo => "pd pdo".into(),
            Command::UfcsPdo => "ufcs pdo".into(),
            Command::PdHardReset => "pd hrst".into(),
            Command::PdRequest {
                position,
                voltage_mv,
                current_ma,
            } => match (voltage_mv, current_ma) {
                (Some(v), Some(c)) => format!("pd req={position},volt={v},cur={c}"),
                (_, Some(c)) => format!("pd req={position},cur={c}"),
                // `volt` is only sent together with `cur`.
                (_, None) => format!("pd req={position}"),
            },
            Command::PdCmd(n) => format!("pd cmd={n}"),
            Command::PdData(bytes) => {
                let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
                format!("pd data={hex}")
            }
            Command::FixedVoltage { protocol, volts } => format!("{} {volts}V", protocol.name()),
            Command::Qc3Volt { voltage_mv } => format!("qc3 volt={voltage_mv}"),
            Command::Qc3Inc(n) => format!("qc3 inc={n}"),
            Command::Qc3Dec(n) => format!("qc3 dec={n}"),
            Command::ScpRequest {
                voltage_mv,
                current_ma,
            } => format!("scp volt={voltage_mv},cur={current_ma}"),
            Command::VfcpRequest {
                voltage_mv,
                current_ma,
            } => format!("vfcp volt={voltage_mv},cur={current_ma}"),
            Command::TfcpRequest {
                voltage_mv,
                current_ma,
            } => format!("tfcp volt={voltage_mv},cur={current_ma}"),
            Command::UfcsRequest {
                index,
                voltage_mv,
                current_ma,
            } => format!("ufcs req={index},volt={voltage_mv},cur={current_ma}"),
            Command::UfcsCmd(n) => format!("ufcs cmd={n}"),
            Command::Raw(text) => text.clone(),
        }
    }

    /// Parse command text; anything not recognised exactly becomes
    /// [`Command::Raw`]. A parsed command renders back to the same text
    /// (numbers in canonical decimal, hex in uppercase).
    pub fn parse(text: &str) -> Command {
        parse_structured(text).unwrap_or_else(|| Command::Raw(text.to_string()))
    }
}

impl Protocol {
    const ALL: [Protocol; 13] = [
        Protocol::Pd,
        Protocol::Ufcs,
        Protocol::Qc,
        Protocol::Fcp,
        Protocol::Scp,
        Protocol::Afc,
        Protocol::Vfcp,
        Protocol::Sfcp,
        Protocol::Tfcp,
        Protocol::Bc,
        Protocol::Apple,
        Protocol::List,
        Protocol::ListPlus,
    ];

    fn name(self) -> &'static str {
        match self {
            Protocol::Pd => "pd",
            Protocol::Ufcs => "ufcs",
            Protocol::Qc => "qc",
            Protocol::Fcp => "fcp",
            Protocol::Scp => "scp",
            Protocol::Afc => "afc",
            Protocol::Vfcp => "vfcp",
            Protocol::Sfcp => "sfcp",
            Protocol::Tfcp => "tfcp",
            Protocol::Bc => "bc",
            Protocol::Apple => "apple",
            Protocol::List => "list",
            Protocol::ListPlus => "list+",
        }
    }
}

impl FixedProtocol {
    const ALL: [FixedProtocol; 4] = [
        FixedProtocol::Qc,
        FixedProtocol::Fcp,
        FixedProtocol::Afc,
        FixedProtocol::Sfcp,
    ];

    fn name(self) -> &'static str {
        match self {
            FixedProtocol::Qc => "qc",
            FixedProtocol::Fcp => "fcp",
            FixedProtocol::Afc => "afc",
            FixedProtocol::Sfcp => "sfcp",
        }
    }
}

/// A canonical decimal number: digits only, no sign, no leading zero.
fn num<T: std::str::FromStr>(s: &str) -> Option<T> {
    let canonical =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if canonical { s.parse().ok() } else { None }
}

/// `k1=<n>,k2=<n>,...` with exactly `keys`, in order.
fn fields<const N: usize>(s: &str, keys: [&str; N]) -> Option<[u32; N]> {
    let mut out = [0u32; N];
    let mut parts = s.split(',');
    for (slot, key) in out.iter_mut().zip(keys) {
        let value = parts.next()?.strip_prefix(key)?.strip_prefix('=')?;
        *slot = num(value)?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

/// Uppercase hex, even length, at least one byte.
fn hex(s: &str) -> Option<Vec<u8>> {
    let ok = !s.is_empty()
        && s.len().is_multiple_of(2)
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b));
    if !ok {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
        .collect()
}

fn parse_structured(text: &str) -> Option<Command> {
    let exact = match text {
        "pdm open" => Some(Command::PdmOpen),
        "pdm close" => Some(Command::PdmClose),
        "reset" => Some(Command::Reset),
        "pdm parm" => Some(Command::PdmParm),
        "pd pdo" => Some(Command::PdPdo),
        "ufcs pdo" => Some(Command::UfcsPdo),
        "pd hrst" => Some(Command::PdHardReset),
        _ => None,
    };
    if exact.is_some() {
        return exact;
    }
    if let Some(rest) = text.strip_prefix("pdm set ") {
        let [ty, em, sink] = fields(rest, ["type", "em", "sink"])?;
        return Some(Command::PdmSet {
            ty: u8::try_from(ty).ok()?,
            em: u8::try_from(em).ok()?,
            sink: u8::try_from(sink).ok()?,
        });
    }
    if let Some(rest) = text.strip_prefix("entry ") {
        return Protocol::ALL
            .into_iter()
            .find(|p| p.name() == rest)
            .map(Command::Entry);
    }
    if let Some(rest) = text.strip_prefix("pd req=") {
        let mut parts = rest.split(',');
        let position = num(parts.next()?)?;
        let tail: Vec<&str> = parts.collect();
        let (voltage_mv, current_ma) = match tail.as_slice() {
            [] => (None, None),
            [cur] => (None, Some(num(cur.strip_prefix("cur=")?)?)),
            [volt, cur] => (
                Some(num(volt.strip_prefix("volt=")?)?),
                Some(num(cur.strip_prefix("cur=")?)?),
            ),
            _ => return None,
        };
        return Some(Command::PdRequest {
            position,
            voltage_mv,
            current_ma,
        });
    }
    if let Some(rest) = text.strip_prefix("pd cmd=") {
        return num(rest).map(Command::PdCmd);
    }
    if let Some(rest) = text.strip_prefix("pd data=") {
        return hex(rest).map(Command::PdData);
    }
    if let Some(rest) = text.strip_prefix("qc3 volt=") {
        return num(rest).map(|voltage_mv| Command::Qc3Volt { voltage_mv });
    }
    if let Some(rest) = text.strip_prefix("qc3 inc=") {
        return num(rest).map(Command::Qc3Inc);
    }
    if let Some(rest) = text.strip_prefix("qc3 dec=") {
        return num(rest).map(Command::Qc3Dec);
    }
    type Vc = fn(u32, u32) -> Command;
    let volt_cur: [(&str, Vc); 3] = [
        ("scp ", |voltage_mv, current_ma| Command::ScpRequest {
            voltage_mv,
            current_ma,
        }),
        ("vfcp ", |voltage_mv, current_ma| Command::VfcpRequest {
            voltage_mv,
            current_ma,
        }),
        ("tfcp ", |voltage_mv, current_ma| Command::TfcpRequest {
            voltage_mv,
            current_ma,
        }),
    ];
    for (prefix, make) in volt_cur {
        if let Some(rest) = text.strip_prefix(prefix)
            && let Some([v, c]) = fields(rest, ["volt", "cur"])
        {
            return Some(make(v, c));
        }
    }
    if let Some(rest) = text.strip_prefix("ufcs req=") {
        let mut parts = rest.splitn(2, ',');
        let index = num(parts.next()?)?;
        let [voltage_mv, current_ma] = fields(parts.next()?, ["volt", "cur"])?;
        return Some(Command::UfcsRequest {
            index,
            voltage_mv,
            current_ma,
        });
    }
    if let Some(rest) = text.strip_prefix("ufcs cmd=") {
        return num(rest).map(Command::UfcsCmd);
    }
    for protocol in FixedProtocol::ALL {
        let volts = text
            .strip_prefix(protocol.name())
            .and_then(|r| r.strip_prefix(' '))
            .and_then(|r| r.strip_suffix('V'))
            .and_then(num);
        if let Some(volts) = volts {
            return Some(Command::FixedVoltage { protocol, volts });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(c: &Command) -> String {
        String::from_utf8(c.render()).expect("ASCII")
    }

    // ---- AC-PR41FT.2: commands render to the exact device text, no terminator ----

    #[test]
    fn session_commands_render_exactly() {
        assert_eq!(r(&Command::PdmOpen), "pdm open");
        assert_eq!(r(&Command::PdmClose), "pdm close");
        assert_eq!(r(&Command::Reset), "reset");
        assert_eq!(r(&Command::PdmParm), "pdm parm");
        assert_eq!(
            r(&Command::PdmSet {
                ty: 0,
                em: 2,
                sink: 0
            }),
            "pdm set type=0,em=2,sink=0"
        );
    }

    #[test]
    fn entry_commands_render_exactly() {
        let cases = [
            (Protocol::Pd, "entry pd"),
            (Protocol::Ufcs, "entry ufcs"),
            (Protocol::Qc, "entry qc"),
            (Protocol::Fcp, "entry fcp"),
            (Protocol::Scp, "entry scp"),
            (Protocol::Afc, "entry afc"),
            (Protocol::Vfcp, "entry vfcp"),
            (Protocol::Sfcp, "entry sfcp"),
            (Protocol::Tfcp, "entry tfcp"),
            (Protocol::Bc, "entry bc"),
            (Protocol::Apple, "entry apple"),
            (Protocol::List, "entry list"),
            (Protocol::ListPlus, "entry list+"),
        ];
        for (p, text) in cases {
            assert_eq!(r(&Command::Entry(p)), text);
        }
    }

    #[test]
    fn request_commands_render_exactly() {
        let pd = |position, voltage_mv, current_ma| Command::PdRequest {
            position,
            voltage_mv,
            current_ma,
        };
        assert_eq!(r(&pd(2, None, None)), "pd req=2");
        assert_eq!(r(&pd(2, None, Some(1500))), "pd req=2,cur=1500");
        assert_eq!(
            r(&pd(9, Some(15000), Some(1000))),
            "pd req=9,volt=15000,cur=1000"
        );
        assert_eq!(r(&Command::PdHardReset), "pd hrst");
        assert_eq!(r(&Command::PdPdo), "pd pdo");
        assert_eq!(r(&Command::UfcsPdo), "ufcs pdo");
        assert_eq!(r(&Command::PdCmd(7)), "pd cmd=7");
        assert_eq!(
            r(&Command::PdData(vec![
                0x00, 0x82, 0x10, 0x2C, 0xB1, 0x04, 0x23
            ])),
            "pd data=0082102CB10423"
        );
        assert_eq!(
            r(&Command::FixedVoltage {
                protocol: FixedProtocol::Qc,
                volts: 9
            }),
            "qc 9V"
        );
        assert_eq!(
            r(&Command::FixedVoltage {
                protocol: FixedProtocol::Sfcp,
                volts: 12
            }),
            "sfcp 12V"
        );
        assert_eq!(r(&Command::Qc3Volt { voltage_mv: 6000 }), "qc3 volt=6000");
        assert_eq!(r(&Command::Qc3Inc(2)), "qc3 inc=2");
        assert_eq!(r(&Command::Qc3Dec(3)), "qc3 dec=3");
        assert_eq!(
            r(&Command::ScpRequest {
                voltage_mv: 9000,
                current_ma: 2000
            }),
            "scp volt=9000,cur=2000"
        );
        assert_eq!(
            r(&Command::VfcpRequest {
                voltage_mv: 9000,
                current_ma: 1000
            }),
            "vfcp volt=9000,cur=1000"
        );
        assert_eq!(
            r(&Command::TfcpRequest {
                voltage_mv: 5000,
                current_ma: 3000
            }),
            "tfcp volt=5000,cur=3000"
        );
        assert_eq!(
            r(&Command::UfcsRequest {
                index: 1,
                voltage_mv: 9000,
                current_ma: 2000
            }),
            "ufcs req=1,volt=9000,cur=2000"
        );
        assert_eq!(r(&Command::UfcsCmd(6)), "ufcs cmd=6");
        assert_eq!(r(&Command::Raw("bogus".into())), "bogus");
    }

    #[test]
    fn rendered_commands_carry_no_terminator() {
        for c in [
            Command::PdmOpen,
            Command::Entry(Protocol::Pd),
            Command::Reset,
        ] {
            let bytes = c.render();
            assert!(!bytes.contains(&b'\n') && !bytes.contains(&b'\r') && !bytes.contains(&0));
        }
    }

    // ---- AC-PR41FT.4: parsing for the classifier; unknown text is Raw ----

    #[test]
    fn parse_round_trips_structured_commands() {
        let all = [
            Command::PdmOpen,
            Command::PdmClose,
            Command::Reset,
            Command::PdmParm,
            Command::PdmSet {
                ty: 2,
                em: 3,
                sink: 1,
            },
            Command::Entry(Protocol::ListPlus),
            Command::Entry(Protocol::Vfcp),
            Command::PdPdo,
            Command::UfcsPdo,
            Command::PdHardReset,
            Command::PdRequest {
                position: 8,
                voltage_mv: None,
                current_ma: None,
            },
            Command::PdRequest {
                position: 2,
                voltage_mv: None,
                current_ma: Some(1500),
            },
            Command::PdRequest {
                position: 9,
                voltage_mv: Some(21000),
                current_ma: Some(1000),
            },
            Command::PdCmd(18),
            Command::PdData(vec![0x00, 0x8D, 0x00]),
            Command::FixedVoltage {
                protocol: FixedProtocol::Afc,
                volts: 5,
            },
            Command::Qc3Volt { voltage_mv: 3600 },
            Command::Qc3Inc(1),
            Command::Qc3Dec(1),
            Command::ScpRequest {
                voltage_mv: 5000,
                current_ma: 1000,
            },
            Command::VfcpRequest {
                voltage_mv: 7000,
                current_ma: 500,
            },
            Command::TfcpRequest {
                voltage_mv: 9000,
                current_ma: 2000,
            },
            Command::UfcsRequest {
                index: 2,
                voltage_mv: 5000,
                current_ma: 1000,
            },
            Command::UfcsCmd(6),
        ];
        for c in all {
            let text = r(&c);
            assert_eq!(Command::parse(&text), c, "{text}");
        }
    }

    #[test]
    fn unknown_or_unparsable_text_is_raw() {
        for text in [
            "bogus",
            "PDM PARM",
            "pd req=abc",
            "pd req=2,volt=",
            "qc 9",
            "qc3 volt=lots",
            "pd data=XYZ",
            "",
        ] {
            assert_eq!(
                Command::parse(text),
                Command::Raw(text.to_string()),
                "{text:?}"
            );
        }
    }
}
