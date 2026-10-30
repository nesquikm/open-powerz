//! AC-4PJRVK.8: the crate's public API, ready for outside use. Public enums
//! are `#[non_exhaustive]`, `Issue` implements `Display`, and sub-decoders
//! follow one visibility rule: the `decode` of a whole wire value (a header, a
//! data object, a message body) is `pub`, with no `pub(crate)` middle ground;
//! a decoder of one variant's body stays private to its module.

use std::fs;
use std::path::{Path, PathBuf};

use powerz_pd::{CommandType, Issue, Request, Sop, Vdm, VdmBody, VdmCommand, VdmHeader};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn sources() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    rust_sources(&crate_dir().join("src"), &mut files);
    files
        .into_iter()
        .map(|p| {
            let text = fs::read_to_string(&p).expect("read source");
            (p, text)
        })
        .collect()
}

/// The attribute and doc lines right above line `i`.
fn attributes_above<'a>(lines: &[&'a str], i: usize) -> Vec<&'a str> {
    lines[..i]
        .iter()
        .rev()
        .take_while(|l| {
            let t = l.trim_start();
            t.starts_with("#[") || t.starts_with("///")
        })
        .copied()
        .collect()
}

#[test]
fn every_public_enum_is_non_exhaustive() {
    let mut enums = 0;
    for (path, text) in sources() {
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.trim_start().starts_with("pub enum ") {
                continue;
            }
            enums += 1;
            assert!(
                attributes_above(&lines, i)
                    .iter()
                    .any(|a| a.contains("#[non_exhaustive]")),
                "{}:{}: `{}` can grow new variants, mark it #[non_exhaustive]",
                path.display(),
                i + 1,
                line.trim()
            );
        }
    }
    assert!(enums > 10, "only {enums} public enums found");
}

#[test]
fn an_issue_reads_as_a_sentence_with_its_numbers() {
    let cases = [
        (Issue::NoSourceCapabilities, vec!["Source_Capabilities"]),
        (
            Issue::PositionOutOfRange {
                position: 9,
                available: 4,
            },
            vec!["9", "4"],
        ),
        (
            Issue::MissingChunk {
                expected: 2,
                got: 7,
            },
            vec!["2", "7"],
        ),
        (
            Issue::ShortBody {
                expected: 28,
                got: 13,
            },
            vec!["28", "13"],
        ),
    ];
    for (issue, parts) in cases {
        let text = issue.to_string();
        for part in parts {
            assert!(text.contains(part), "{issue:?} reads {text:?}");
        }
        assert_ne!(text, format!("{issue:?}"), "Display is not Debug");
    }
}

#[test]
fn a_vdm_decodes_through_the_public_api_like_every_other_message_body() {
    let v = Vdm::decode(&[0xFF00_A001], Sop::Sop).expect("a VDM with a header");
    match v.header {
        VdmHeader::Structured(h) => {
            assert_eq!(h.svid, 0xFF00);
            assert_eq!(h.command_type, CommandType::Req);
            assert_eq!(h.command, VdmCommand::DiscoverIdentity);
        }
        other => panic!("expected a structured header, got {other:?}"),
    }
    assert_eq!(v.body, VdmBody::Objects(Vec::new()));
    assert_eq!(Vdm::decode(&[], Sop::Sop), None, "no header, no VDM");
}

#[test]
fn a_request_decodes_through_the_public_api_with_or_without_its_offer() {
    const FIXED_RDO: u32 = 0x1000_0000;
    const FIXED_5V_PDO: u32 = 0x0001_912C;
    let (request, issues) = Request::against_caps(FIXED_RDO, None);
    assert_eq!(request.pdo, None);
    assert_eq!(issues, vec![Issue::NoSourceCapabilities]);
    assert!(Request::with_copy(FIXED_RDO, FIXED_5V_PDO).pdo.is_some());
}

#[test]
fn no_decode_is_crate_private() {
    for (path, text) in sources() {
        for (i, line) in text.lines().enumerate() {
            assert!(
                !line.contains("pub(crate) fn decode"),
                "{}:{}: a decode is `pub` or private to its module",
                path.display(),
                i + 1
            );
        }
    }
}

#[test]
fn the_api_docs_state_the_rules() {
    let page = crate_dir().join("../../docs/reference/api/powerz-pd.md");
    let docs = fs::read_to_string(page).expect("the powerz-pd API page");
    assert!(docs.contains("non_exhaustive"), "public enums may grow");
    assert!(docs.contains("Vdm::decode"), "the VDM decoder is public");
    assert!(
        docs.lines()
            .any(|l| l.contains("`Issue`") && l.contains("Display")),
        "`Issue` is listed with its Display"
    );
}
