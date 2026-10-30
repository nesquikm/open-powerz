//! AC-4PJRVH.7: GoodCRC detection lives in the library and in the UI, so one
//! fixture pins both. This side runs `tests/vectors/goodcrc_parity.json`
//! through the library's refusal; `apps/desktop/src/lib/goodcrcParity.test.ts`
//! runs the same file through the UI's.

use powerz_protocol::pdm::console;
use powerz_protocol::pdm::message::{self, MessageError, PdMessage, SopTarget};
use serde_json::Value;

const FIXTURE: &str = include_str!("vectors/goodcrc_parity.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("the fixture is JSON")
}

/// What the library says about a console line, in the fixture's words.
fn line_verdict(line: &str) -> &'static str {
    match message::refusal(&console::command_of(line)) {
        None => "none",
        Some(reason) if reason.to_lowercase().contains("cannot tell") => "unsure",
        Some(_) => "goodcrc",
    }
}

#[test]
fn the_library_gives_every_line_its_fixture_verdict() {
    let fixture = fixture();
    let lines = fixture["lines"].as_array().expect("lines");
    assert!(lines.len() > 40, "the fixture lost its lines");
    let wrong: Vec<String> = lines
        .iter()
        .filter_map(|case| {
            let line = case["line"].as_str().expect("line");
            let want = case["verdict"].as_str().expect("verdict");
            let got = line_verdict(line);
            (got != want).then(|| format!("{line:?}: want {want}, got {got}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn the_library_refuses_exactly_the_fixture_s_built_goodcrc_messages() {
    let fixture = fixture();
    let messages = fixture["messages"].as_array().expect("messages");
    assert!(!messages.is_empty());
    for case in messages {
        let sop = match case["sop"].as_str().expect("sop") {
            "sop" => SopTarget::Sop,
            "sop_prime" => SopTarget::SopPrime,
            "sop_double_prime" => SopTarget::SopDoublePrime,
            other => panic!("unknown sop {other}"),
        };
        let msg_type = case["msg_type"].as_u64().expect("msg_type") as u8;
        let data_objects: Vec<u32> = case["data_objects"]
            .as_array()
            .expect("data_objects")
            .iter()
            .map(|o| o.as_u64().expect("object") as u32)
            .collect();
        let refused = matches!(
            PdMessage {
                sop,
                msg_type,
                data_objects
            }
            .to_command(),
            Err(MessageError::GoodCrc)
        );
        let want = case["good_crc"].as_bool().expect("good_crc");
        assert_eq!(refused, want, "{case}");
    }
}
