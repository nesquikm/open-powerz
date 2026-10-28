//! Structural checks on `docs/protocol.md` (FR ZVJ3EB).
//!
//! The protocol reference is hand-written Markdown; these tests keep it from
//! silently losing its required sections, confidence tags, device-confirmed
//! facts, never-send list, and from leaking key bytes or device identifiers.

use std::path::PathBuf;

const TAGS: [&str; 4] = ["[device]", "[static]", "[doc]", "[inferred]"];
const FW: &str = "2.0.6";

fn repo_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn read_repo_file(rel: &str) -> String {
    let path = repo_file(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn doc() -> String {
    read_repo_file("docs/protocol.md")
}

/// Lines of the doc outside fenced code blocks.
fn prose_lines(text: &str) -> Vec<&str> {
    let mut in_fence = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            out.push(line);
        }
    }
    out
}

/// `## ` sections: (heading text, body lines). Text before the first `## `
/// heading is returned under the empty heading "".
fn sections(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = vec![(String::new(), Vec::new())];
    for line in prose_lines(text) {
        if let Some(h) = line.strip_prefix("## ") {
            out.push((h.trim().to_string(), Vec::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push(line.to_string());
        }
    }
    out
}

/// Required `##` sections, each identified by keywords that must all appear
/// (case-insensitive) in exactly one heading.
const REQUIRED: &[(&str, &[&str])] = &[
    ("usb identity", &["usb identity", "interface"]),
    ("header", &["header"]),
    ("message catalogue", &["message catalog"]),
    ("get_data attributes", &["get_data", "attribute"]),
    ("adc snapshot", &["adc snapshot"]),
    ("adc queue", &["adc queue"]),
    ("settings", &["settings"]),
    ("memory read", &["memory read", "device info"]),
    ("authentication", &["authentication"]),
    ("session flow", &["session flow", "error"]),
    ("hid limits", &["hid limit"]),
    ("cdc", &["cdc", "adcnew"]),
    ("pd capture", &["pd capture"]),
    ("never send", &["never"]),
];

fn heading_matches(heading: &str, keys: &[&str]) -> bool {
    let h = heading.to_lowercase();
    keys.iter().all(|k| h.contains(k))
}

/// Body of the single section whose heading matches the REQUIRED entry `id`.
fn section(text: &str, id: &str) -> Vec<String> {
    let keys = REQUIRED
        .iter()
        .find(|(name, _)| *name == id)
        .map(|(_, k)| *k)
        .unwrap_or_else(|| panic!("unknown section id {id}"));
    let found: Vec<_> = sections(text)
        .into_iter()
        .filter(|(h, _)| heading_matches(h, keys))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one `##` heading matching {keys:?}, found {:?}",
        found.iter().map(|(h, _)| h.clone()).collect::<Vec<_>>()
    );
    found.into_iter().next().map(|(_, b)| b).unwrap_or_default()
}

/// Statement units: each table row on its own, otherwise paragraphs / list
/// items separated by blank lines, joined into one string.
fn units(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut para = String::new();
    for line in lines {
        let t = line.trim();
        if t.starts_with('|') {
            if !para.is_empty() {
                out.push(std::mem::take(&mut para));
            }
            out.push(t.to_string());
        } else if t.is_empty() || t.starts_with("- ") || t.starts_with("* ") {
            if !para.is_empty() {
                out.push(std::mem::take(&mut para));
            }
            para.push_str(t);
        } else {
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(t);
        }
    }
    if !para.is_empty() {
        out.push(para);
    }
    out
}

fn all_lines(text: &str) -> Vec<String> {
    prose_lines(text).into_iter().map(str::to_string).collect()
}

fn has_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

/// Contains `needle` not immediately followed by a hex digit (so `0x42`
/// does not match inside `0x420`).
fn contains_token(hay: &str, needle: &str) -> bool {
    let hay = hay.to_lowercase();
    let needle = needle.to_lowercase();
    let mut start = 0;
    while let Some(pos) = hay[start..].find(&needle) {
        let end = start + pos + needle.len();
        let next = hay[end..].chars().next();
        if !next.is_some_and(|c| c.is_ascii_hexdigit()) {
            return true;
        }
        start = start + pos + 1;
    }
    false
}

/// Asserts some unit is tagged `[device]` + firmware 2.0.6 and contains all
/// of `all` plus at least one of each group in `any_of`.
fn assert_device_fact(units: &[String], what: &str, all: &[&str], any_of: &[&[&str]]) {
    let ok = units.iter().any(|u| {
        let l = u.to_lowercase();
        l.contains("[device]")
            && l.contains(FW)
            && all.iter().all(|k| l.contains(k))
            && any_of.iter().all(|g| has_any(&l, g))
    });
    assert!(
        ok,
        "no statement tagged `[device]` with firmware {FW} states: {what} \
         (needs {all:?} and one of each {any_of:?})"
    );
}

const NO_AUTH: &[&str] = &["without auth", "no auth", "unauthenticated", "before auth"];

// ---------------------------------------------------------------- AC .1

#[test]
fn ac1_doc_has_every_required_section_once() {
    let text = doc();
    let heads: Vec<String> = sections(&text).into_iter().map(|(h, _)| h).collect();
    for (id, keys) in REQUIRED {
        let n = heads.iter().filter(|h| heading_matches(h, keys)).count();
        assert_eq!(
            n, 1,
            "section `{id}`: expected exactly one `##` heading containing {keys:?}, \
             found {n} in {heads:?}"
        );
    }
}

#[test]
fn ac1_header_section_covers_three_views() {
    let text = doc();
    let body = section(&text, "header").join("\n").to_lowercase();
    for view in ["main control", "main data", "extended"] {
        assert!(
            body.contains(view),
            "header section does not describe the `{view}` layout"
        );
    }
}

// ---------------------------------------------------------------- AC .2

#[test]
fn ac2_doc_opens_with_legend_of_all_tags() {
    let text = doc();
    // The legend sits before the first required content section.
    let secs = sections(&text);
    let first_required = secs
        .iter()
        .position(|(h, _)| REQUIRED.iter().any(|(_, k)| heading_matches(h, k)))
        .expect("no required section found");
    let preamble: String = secs[..first_required]
        .iter()
        .flat_map(|(h, b)| std::iter::once(h.clone()).chain(b.iter().cloned()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        preamble.to_lowercase().contains("legend"),
        "no legend before the first protocol section"
    );
    for tag in TAGS {
        let defined = preamble
            .lines()
            .any(|l| l.contains(tag) && l.replace(tag, "").trim().len() > 3);
        assert!(defined, "legend does not define {tag}");
    }
}

#[test]
fn ac2_every_table_data_row_carries_a_tag() {
    let text = doc();
    let mut rows_seen = 0;
    let mut untagged = Vec::new();
    let mut idx_in_table = 0usize;
    for line in prose_lines(&text) {
        let t = line.trim();
        if !t.starts_with('|') {
            idx_in_table = 0;
            continue;
        }
        idx_in_table += 1;
        let is_separator = t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '));
        if idx_in_table == 1 || is_separator {
            continue; // header row or alignment row
        }
        rows_seen += 1;
        if !TAGS.iter().any(|tag| t.contains(tag)) {
            untagged.push(t.to_string());
        }
    }
    assert!(rows_seen > 0, "doc has no tables");
    assert!(
        untagged.is_empty(),
        "{} table rows carry no confidence tag, e.g. {:?}",
        untagged.len(),
        untagged.iter().take(5).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------- AC .3

#[test]
fn ac3_adc_payload_44_bytes_is_device_confirmed() {
    let text = doc();
    let u = units(&section(&text, "adc snapshot"));
    assert_device_fact(&u, "ADC payload is 44 bytes", &["44"], &[&["byte"]]);
}

#[test]
fn ac3_rate_codes_are_device_confirmed() {
    let text = doc();
    let u = units(&section(&text, "adc queue"));
    for sps in ["2 sps", "10 sps", "50 sps", "1000 sps"] {
        assert_device_fact(&u, &format!("rate code -> {sps}"), &[sps], &[]);
    }
    let code4 = u.iter().any(|x| {
        let l = x.to_lowercase();
        l.contains("[device]")
            && l.contains(FW)
            && (l.contains("code 4") || l.starts_with("| 4 ") || l.starts_with("| `4`"))
            && has_any(&l, &["code 0", "as 0", "like 0", "2 sps"])
    });
    assert!(
        code4,
        "no [device] {FW} statement that rate code 4 behaves like code 0"
    );
}

#[test]
fn ac3_queue_polling_without_auth_and_rejects_below_level_1() {
    let text = doc();
    let u = units(&all_lines(&text));
    assert_device_fact(
        &u,
        "queue polling works without auth",
        &["queue"],
        &[NO_AUTH],
    );
    assert_device_fact(
        &u,
        "0x0E/0x0F/0x10/0x11 are REJECTed below auth level 1",
        &["reject", "0x0e", "0x0f", "0x10", "0x11"],
        &[&["level 1"]],
    );
}

#[test]
fn ac3_memory_read_without_auth() {
    let text = doc();
    let u = units(&section(&text, "memory read"));
    assert_device_fact(&u, "memory read works without auth", &[], &[NO_AUTH]);
}

#[test]
fn ac3_reply_ids_echoed_except_0x4c() {
    let text = doc();
    let u = units(&all_lines(&text));
    assert_device_fact(
        &u,
        "reply ids echoed except the 0x4C reply (id 0)",
        &["echo", "0x4c"],
        &[&["id 0", "id = 0", "id=0", "id `0`", "zero"]],
    );
}

#[test]
fn ac3_auth_level_in_header_bits_and_lost_on_reopen() {
    let text = doc();
    let u = units(&all_lines(&text));
    assert_device_fact(
        &u,
        "auth level in reply header bits 17..18",
        &["auth", "17", "18"],
        &[&["bit"]],
    );
    assert_device_fact(
        &u,
        "auth level lost on reopen",
        &["reopen"],
        &[&["auth", "level"]],
    );
}

#[test]
fn ac3_settings_180_bytes_two_crc32_blocks() {
    let text = doc();
    let u = units(&section(&text, "settings"));
    assert_device_fact(&u, "SETTINGS is 180 bytes", &["180"], &[&["byte"]]);
    assert_device_fact(
        &u,
        "SETTINGS has two CRC-32 blocks",
        &[],
        &[&["crc-32", "crc32"], &["two", "2 "]],
    );
}

#[test]
fn ac3_pd_status_size_zero_with_trailing_bytes() {
    let text = doc();
    let u = units(&all_lines(&text));
    assert_device_fact(
        &u,
        "PD_STATUS declares size 0 with trailing bytes",
        &["pd_status", "trailing"],
        &[&["size 0", "size = 0", "size=0", "size `0`", "zero"]],
    );
}

// ---------------------------------------------------------------- AC .4

fn assert_hid_device(u: &[String], what: &str, all: &[&str], any_of: &[&[&str]]) {
    let ok = u.iter().any(|x| {
        let l = x.to_lowercase();
        l.contains("[device]")
            && all.iter().all(|k| l.contains(k))
            && any_of.iter().all(|g| has_any(&l, g))
    });
    assert!(
        ok,
        "HID section lacks a `[device]` statement: {what} (needs {all:?}, {any_of:?})"
    );
}

#[test]
fn ac4_hid_section_device_statements() {
    let text = doc();
    let u = units(&section(&text, "hid limits"));
    assert_hid_device(
        &u,
        "ADC snapshot (GET_DATA mask 0x001) is the only safe HID request",
        &["0x001", "only", "safe"],
        &[],
    );
    assert_hid_device(
        &u,
        "PD_PACKET / QC_PACKET fit 64 bytes only with nothing attached",
        &["pd_packet", "qc_packet", "64"],
        &[&[
            "nothing attached",
            "nothing is attached",
            "no charger",
            "unattached",
            "idle",
        ]],
    );
    assert_hid_device(
        &u,
        "PD_PACKET / QC_PACKET grow past 64 bytes with a charger",
        &["charger"],
        &[&["pd_packet", "qc_packet"], &["64"]],
    );
    assert_hid_device(
        &u,
        "control commands get no reply over HID",
        &["control"],
        &[&[
            "no reply",
            "never reply",
            "not reply",
            "no response",
            "without a reply",
        ]],
    );
    assert_hid_device(
        &u,
        "a reply longer than 64 bytes hangs and reboots the meter",
        &["64", "reboot"],
        &[&["hang"]],
    );
}

// ---------------------------------------------------------------- AC .5

#[test]
fn ac5_never_send_section_lists_forbidden_requests() {
    let text = doc();
    let body = section(&text, "never send").join("\n");
    for ty in ["0x04", "0x08", "0x09", "0x42", "0x48", "0x4A"] {
        assert!(
            contains_token(&body, ty),
            "never-send section does not name type {ty}"
        );
    }
    for addr in ["0x420", "0x4420", "0x03000C00", "0x40010450"] {
        assert!(
            contains_token(&body, addr),
            "never-send section does not name allowed memory address {addr}"
        );
    }
    let l = body.to_lowercase();
    assert!(
        l.contains("memory read")
            && has_any(&l, &["outside", "other than", "except", "any address"]),
        "never-send section does not forbid memory reads outside the allowed addresses"
    );
    assert!(
        l.contains("hid") && l.contains("snapshot"),
        "never-send section does not forbid HID requests other than the ADC snapshot"
    );
    assert!(
        l.contains("guard") && has_any(&l, &["refuse", "reject", "block"]),
        "never-send section does not state that the library guard refuses them"
    );
}

// ---------------------------------------------------------------- AC .6

fn longest_hex_run(text: &str) -> usize {
    let mut best = 0;
    let mut cur = 0;
    for c in text.chars() {
        if c.is_ascii_hexdigit() {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 0;
        }
    }
    best
}

/// Longest run of comma-separated `0xNN` byte literals on one line.
fn longest_byte_list(text: &str) -> usize {
    let mut best = 0;
    for line in text.lines() {
        let mut cur = 0;
        for tok in line.split(|c: char| c == ',' || c.is_whitespace() || c == '[' || c == ']') {
            if tok.is_empty() {
                continue;
            }
            let t = tok.to_lowercase();
            if t.len() == 4 && t.starts_with("0x") && t[2..].chars().all(|c| c.is_ascii_hexdigit())
            {
                cur += 1;
                best = best.max(cur);
            } else {
                cur = 0;
            }
        }
    }
    best
}

#[test]
fn ac6_doc_has_no_key_bytes() {
    let text = doc();
    let run = longest_hex_run(&text);
    assert!(
        run < 32,
        "doc contains a {run}-char hex run (possible AES key)"
    );
    let list = longest_byte_list(&text);
    assert!(
        list < 16,
        "doc contains a list of {list} byte literals (possible AES key)"
    );
    let l = text.to_lowercase();
    assert!(!l.contains("key_hex"), "doc contains a key_hex dump");
}

#[test]
fn ac6_doc_has_no_vendor_code() {
    let l = doc().to_lowercase();
    for marker in [
        "typedef struct",
        "#include",
        "libusb_bulk_transfer(",
        "_typedef {",
    ] {
        assert!(
            !l.contains(marker),
            "doc contains vendor-style code: {marker}"
        );
    }
}

#[test]
fn ac6_doc_names_key_module() {
    let text = doc();
    assert!(
        has_any(&text, &["crypto::keys", "crypto/keys.rs"]),
        "doc does not name the crate module that holds the AES keys (crypto::keys)"
    );
}

#[test]
fn doc_has_no_session1_serial_or_uid() {
    // The session-1 identifiers are kept out of the repo: only their SHA-256
    // digests live here, and every window of the doc is hashed and compared.
    const SERIAL_SHA256: &str = "373c9204a506742bdde27bc2340e74bf9af68440d4baaa4bed5bc821a7992281";
    const UID_SHA256: &str = "f2b044ae70a59bb91d6fb348ecd8b2dc98da30e70bf249cd859081962f64b5cc";
    let text = doc();
    let digits: Vec<char> = text.chars().filter(|c| c.is_ascii_digit()).collect();
    assert!(
        !window_hashes(&digits, 6).any(|h| h == SERIAL_SHA256),
        "doc contains the session-1 device serial"
    );
    let squashed: Vec<char> = text
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    assert!(
        !window_hashes(&squashed, 24).any(|h| h == UID_SHA256),
        "doc contains the session-1 device UID"
    );
}

fn window_hashes(chars: &[char], len: usize) -> impl Iterator<Item = String> + '_ {
    use sha2::{Digest, Sha256};
    chars.windows(len).map(|w| {
        let s: String = w.iter().collect();
        Sha256::digest(s.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    })
}

// ---------------------------------------------------------------- AC .7

#[test]
fn ac7_readme_links_protocol_doc() {
    let readme = read_repo_file("README.md");
    assert!(
        has_any(&readme, &["](docs/protocol.md", "](./docs/protocol.md"]),
        "README.md has no Markdown link to docs/protocol.md"
    );
}

// ------------------------------------------------- AC-X2SPDW.12 (FR X2SPDW)

#[test]
fn x2spdw_12_clearing_an_endpoint_halt_is_listed_as_safe_with_a_tag() {
    let text = doc();
    let found = units(&all_lines(&text)).into_iter().any(|u| {
        let l = u.to_lowercase();
        l.contains("halt") && l.contains("clear") && l.contains("safe") && has_any(&l, &TAGS)
    });
    assert!(
        found,
        "docs/protocol.md does not list clearing an endpoint halt among the safe \
         operations with a confidence tag"
    );
}

// ---- AC-1HHT01.5: every decoded settings field is documented with a tag ----

/// Spellings of a bit range `a..b` the doc may use.
fn bit_range(a: u8, b: u8) -> Vec<String> {
    vec![
        format!("{a}..{b}"),
        format!("{a}-{b}"),
        format!("{a}–{b}"),
        format!("{a} to {b}"),
    ]
}

/// The SETTINGS section has a table row naming `keyword` with one of the
/// `position` spellings and a confidence tag.
fn assert_settings_field_row(rows: &[String], keyword: &str, position: &[String]) {
    let ok = rows.iter().any(|r| {
        let l = r.to_lowercase();
        l.contains(keyword)
            && position.iter().any(|p| l.contains(&p.to_lowercase()))
            && (l.contains("[device]") || l.contains("[inferred]"))
    });
    assert!(
        ok,
        "SETTINGS: no table row documents {keyword:?} at one of {position:?} with a \
         [device] or [inferred] tag"
    );
}

#[test]
fn settings_doc_documents_every_decoded_field_with_a_tag() {
    let text = doc();
    let rows: Vec<String> = units(&section(&text, "settings"))
        .into_iter()
        .filter(|u| u.starts_with('|'))
        .collect();
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_settings_field_row(&rows, "language", &s(&["bit 0"]));
    assert_settings_field_row(&rows, "brightness", &bit_range(3, 9));
    assert_settings_field_row(&rows, "dim", &bit_range(10, 11));
    assert_settings_field_row(&rows, "startup logo", &bit_range(14, 15));
    assert_settings_field_row(&rows, "theme", &bit_range(16, 18));
    assert_settings_field_row(&rows, "auto-rotate", &s(&["bit 27"]));
    assert_settings_field_row(&rows, "rotation", &bit_range(0, 1));
    assert_settings_field_row(&rows, "rate", &bit_range(2, 3));
    assert_settings_field_row(&rows, "voltage", &s(&["0x1c"]));
    assert_settings_field_row(&rows, "current", &s(&["0x30"]));
    assert_settings_field_row(&rows, "offset", &s(&["0x58"]));
}

#[test]
fn settings_doc_says_crc_mismatch_is_refused_and_nothing_is_written() {
    let text = doc();
    let body = section(&text, "settings").join("\n").to_lowercase();
    assert!(
        body.contains("mismatch") && body.contains("crc"),
        "SETTINGS must say a CRC mismatch is reported instead of decoding"
    );
    assert!(
        contains_token(&body, "0x0a") && contains_token(&body, "0x48"),
        "SETTINGS must say settings writes (0x0A) and SET_DATA (0x48) are never sent"
    );
}

// ---- AC-1HHT00.3: offline records are documented with tags and evidence ----

/// Body of the single `##` section about offline records.
fn offline_records_section(text: &str) -> Vec<String> {
    let found: Vec<_> = sections(text)
        .into_iter()
        .filter(|(h, _)| heading_matches(h, &["offline record"]))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one `##` heading naming offline records, found {:?}",
        found.iter().map(|(h, _)| h.clone()).collect::<Vec<_>>()
    );
    found.into_iter().next().map(|(_, b)| b).unwrap_or_default()
}

/// A table row of `rows` names `keyword` at `offset` and carries a tag.
fn assert_layout_row(rows: &[String], what: &str, offset: &str, keyword: &str) {
    let ok = rows.iter().any(|r| {
        let l = r.to_lowercase();
        contains_token(&l, offset) && l.contains(keyword) && TAGS.iter().any(|t| l.contains(t))
    });
    assert!(
        ok,
        "{what}: no table row documents {keyword:?} at {offset} with a confidence tag"
    );
}

#[test]
fn offline_records_doc_documents_the_48_byte_entry_with_tags() {
    let text = doc();
    let rows: Vec<String> = units(&offline_records_section(&text))
        .into_iter()
        .filter(|u| u.starts_with('|'))
        .collect();
    assert_layout_row(&rows, "entry", "0x00", "name");
    assert_layout_row(&rows, "entry", "0x11", "interval");
    assert_layout_row(&rows, "entry", "0x12", "point");
    assert_layout_row(&rows, "entry", "0x18", "duration");
    assert_layout_row(&rows, "entry", "0x1c", "charge");
    assert_layout_row(&rows, "entry", "0x20", "energy");
    assert_layout_row(&rows, "entry", "0x24", "offset");
    let body = offline_records_section(&text).join("\n").to_lowercase();
    assert!(body.contains("48"), "the entry size (48 bytes) is stated");
    assert!(
        body.contains("unknown"),
        "the unknown entry bytes are named"
    );
}

#[test]
fn offline_records_doc_documents_the_16_byte_sample_as_inferred_with_static_evidence() {
    let text = doc();
    let section = offline_records_section(&text);
    let rows: Vec<String> = units(&section)
        .into_iter()
        .filter(|u| u.starts_with('|'))
        .collect();
    for (off, kw) in [
        ("0x00", "vbus"),
        ("0x04", "ibus"),
        ("0x08", "charge"),
        ("0x0c", "energy"),
    ] {
        let ok = rows.iter().any(|r| {
            let l = r.to_lowercase();
            contains_token(&l, off)
                && l.contains(kw)
                && (l.contains("[inferred]") || l.contains("[device]"))
                && l.contains("[static]")
        });
        assert!(
            ok,
            "sample: no row documents {kw} at {off} with its [static] evidence and an \
             [inferred] (or, once confirmed, [device]) tag"
        );
    }
    let body = section.join("\n").to_lowercase();
    assert!(body.contains("16"), "the sample size (16 bytes) is stated");
    assert!(body.contains("interval"), "sample time = interval × index");
}

#[test]
fn offline_records_doc_states_the_download_window_and_its_bounds() {
    let text = doc();
    let body = offline_records_section(&text).join("\n").to_lowercase();
    assert!(
        contains_token(&body, "0x98100000"),
        "the download address base 0x98100000 is stated"
    );
    assert!(
        contains_token(&body, "0x200000") || contains_token(&body, "0x300000"),
        "the record-area bound is stated"
    );
    assert!(
        contains_token(&body, "0x200"),
        "the RecordList GET_DATA mask 0x200 is named"
    );
    assert!(
        body.contains("delete") || body.contains("never"),
        "the section says nothing is deleted or written"
    );
}

#[test]
fn offline_records_doc_states_the_device_facts_of_the_real_meter_run() {
    let text = doc();
    let section = offline_records_section(&text);
    let body = section.join("\n").to_lowercase();
    assert!(
        contains_token(&body, "256"),
        "the 256-byte memory-read reply limit is stated"
    );
    assert!(
        body.contains("chunk") || body.contains("consecutive"),
        "the download is a run of consecutive reads"
    );
    assert!(body.contains("capacity"), "the 0x14 capacity is named");
    assert!(body.contains("timestamp"), "no start timestamp is said");
    assert!(
        body.contains("magnitude") && body.contains("negative"),
        "the sign of charge and energy and the magnitude display are stated"
    );
    let rows: Vec<String> = units(&section)
        .into_iter()
        .filter(|u| u.starts_with('|'))
        .collect();
    assert_layout_row(&rows, "entry", "0x14", "capacity");
    for (off, kw) in [
        ("0x00", "vbus"),
        ("0x04", "ibus"),
        ("0x08", "charge"),
        ("0x0c", "energy"),
    ] {
        let ok = rows.iter().any(|r| {
            let l = r.to_lowercase();
            contains_token(&l, off)
                && l.contains(kw)
                && l.contains("[device]")
                && !l.contains("[inferred]")
        });
        assert!(ok, "sample: {kw} at {off} is now [device]");
    }
}

#[test]
fn memory_read_doc_states_the_256_byte_reply_limit() {
    let text = doc();
    let body = section(&text, "memory read").join("\n").to_lowercase();
    assert!(
        contains_token(&body, "256") && body.contains("[device]"),
        "Memory read must state the 256-byte reply limit with a [device] tag"
    );
}

/// The settings-field table row naming `keyword`, lowercase.
fn settings_field_row(rows: &[String], keyword: &str) -> String {
    rows.iter()
        .map(|r| r.to_lowercase())
        .find(|l| l.starts_with(&format!("| {keyword}")))
        .unwrap_or_else(|| panic!("no settings-field row for {keyword:?}"))
}

#[test]
fn settings_doc_marks_the_screen_checked_fields_device_and_rotation_inferred() {
    let text = doc();
    let rows: Vec<String> = units(&section(&text, "settings"))
        .into_iter()
        .filter(|u| u.starts_with('|'))
        .collect();
    for kw in [
        "language",
        "brightness",
        "dim",
        "startup logo",
        "theme",
        "auto-rotate",
        "rate",
    ] {
        let row = settings_field_row(&rows, kw);
        assert!(row.contains("[device]"), "{kw} is screen-checked: {row}");
        assert!(
            row.contains("[static]"),
            "{kw} keeps its [static] map tag: {row}"
        );
        assert!(!row.contains("meaning [inferred]"), "{kw}: {row}");
    }
    for kw in ["rotation", "voltage", "current", "offset"] {
        let row = settings_field_row(&rows, kw);
        assert!(row.contains("[inferred]"), "{kw} stays inferred: {row}");
    }
    let body = section(&text, "settings").join("\n").to_lowercase();
    assert!(
        body.contains("0°") && body.contains("90°") && body.contains("auto-rotate on"),
        "the rotation mismatch (screen 0°, bits read 90°, auto-rotate on) is noted"
    );
}

#[test]
fn never_send_allows_only_the_record_window_besides_the_four_blocks() {
    let text = doc();
    let body = section(&text, "never send").join("\n").to_lowercase();
    assert!(
        body.contains("offline record") || contains_token(&body, "0x98100000"),
        "Never-send must name the record-download window as the one memory-read \
         exception to the four known blocks"
    );
}
