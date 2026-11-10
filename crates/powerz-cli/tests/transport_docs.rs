//! AC-SPYPSD.3: the CLI reference states the transport rules the CLI enforces.
//!
//! The serial-port commands (`trigger`, `detect`, `console`, `pd send`) run
//! with `bulk`, `cdc` and `sim` and are refused over `hid`; `watch --source
//! cdc` and `stream --source cdc` run with any pipe but `hid`. The `--transport`
//! row of `docs/reference/cli.md` must say both.

use std::path::PathBuf;

use clap::Parser;
use powerz_cli::{Cli, validate};

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).unwrap_or_else(|e| panic!("{args:?}: {e}"))
}

fn with_transport<'a>(transport: &'a str, command: &[&'a str]) -> Vec<&'a str> {
    let mut v = vec!["powerz", "--transport", transport];
    v.extend_from_slice(command);
    v
}

/// The serial-port commands, by the name the reference uses, with real arguments.
const SERIAL_COMMANDS: &[(&str, &[&str])] = &[
    ("trigger", &["trigger", "pd", "list"]),
    ("detect", &["detect"]),
    ("console", &["console"]),
    ("pd send", &["pd", "send", "8D00"]),
];

/// The `--source cdc` commands, with real arguments.
const CDC_SOURCE_COMMANDS: &[(&str, &[&str])] = &[
    ("watch", &["watch", "--source", "cdc"]),
    (
        "stream",
        &[
            "stream", "--source", "cdc", "--rate", "50", "--csv", "o.csv",
        ],
    ),
];

#[test]
fn hid_refuses_the_serial_port_commands() {
    for (name, cmd) in SERIAL_COMMANDS {
        let e = validate(&parse(&with_transport("hid", cmd)))
            .expect_err(&format!("`{name}` over hid must be refused"));
        assert_eq!(e.exit_code(), 2, "`{name}` over hid: {e:?}");
    }
}

#[test]
fn the_serial_port_commands_run_over_bulk_cdc_and_sim() {
    for (name, cmd) in SERIAL_COMMANDS {
        for transport in ["bulk", "cdc", "sim"] {
            assert_eq!(
                validate(&parse(&with_transport(transport, cmd))),
                Ok(()),
                "`{name}` over {transport}"
            );
        }
    }
}

#[test]
fn the_cdc_source_runs_with_any_pipe_but_hid() {
    for (name, cmd) in CDC_SOURCE_COMMANDS {
        for transport in ["bulk", "cdc", "sim"] {
            assert_eq!(
                validate(&parse(&with_transport(transport, cmd))),
                Ok(()),
                "`{name} --source cdc` over {transport}"
            );
        }
        let e = validate(&parse(&with_transport("hid", cmd)))
            .expect_err(&format!("`{name} --source cdc` over hid must be refused"));
        assert_eq!(e.exit_code(), 2, "`{name} --source cdc` over hid: {e:?}");
    }
}

/// The `--transport` row of the CLI reference's options table.
fn transport_row() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/cli.md");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.lines()
        .find(|l| l.trim_start().starts_with("| `--transport"))
        .unwrap_or_else(|| panic!("no `--transport` row in {}", path.display()))
        .to_owned()
}

#[test]
fn the_reference_says_hid_refuses_each_serial_port_command() {
    let row = transport_row();
    for (name, _) in SERIAL_COMMANDS {
        assert!(
            row.contains(&format!("`{name}`")),
            "the --transport row does not name `{name}`: {row}"
        );
    }
    let lower = row.to_lowercase();
    assert!(
        lower.contains("`hid` refuses them") || lower.contains("hid refuses"),
        "the --transport row does not say hid refuses the serial-port commands: {row}"
    );
    assert!(
        lower.contains("serial port"),
        "the --transport row does not name the serial port: {row}"
    );
    // ...and that bulk and cdc carry them.
    assert!(
        lower.contains("with `bulk` or `cdc`"),
        "the --transport row does not say the serial-port commands run with bulk or cdc: {row}"
    );
}

#[test]
fn the_reference_says_the_cdc_source_runs_with_any_pipe_but_hid() {
    let row = transport_row();
    for (name, _) in CDC_SOURCE_COMMANDS {
        assert!(
            row.contains(&format!("`{name} --source cdc`")),
            "the --transport row does not name `{name} --source cdc`: {row}"
        );
    }
    assert!(
        row.contains("any pipe but `hid`"),
        "the --transport row does not say the CDC source runs with any pipe but hid: {row}"
    );
}
