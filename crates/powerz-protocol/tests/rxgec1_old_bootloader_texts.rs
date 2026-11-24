//! FR RXGEC1 (AC .1, .2, .5, .6): the texts about the old (2.1) bootloader
//! say its flash is experimental and works only from the CLI. The docs quote
//! the new texts and keep their hedges, and no source or doc file still says
//! the old bootloader is "not supported yet".

use std::path::{Path, PathBuf};

use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::flash::{
    LEGACY_BOOTLOADER_HELP, SWITCHED_TO_LEGACY_BOOTLOADER_HELP,
};

const MODE: &str =
    "bootloader (old, 2.1-style; experimental CLI-only flash with --experimental-old-bootloader)";
const SUPPORT: &str = "supported only experimentally, from the CLI";
const SUFFIX: &str = "(old bootloader, experimental CLI update)";
const STALE: &str = "not supported yet";
/// The whole command, ready to copy.
const COMMAND: &str = "powerz firmware flash FILE --i-understand --experimental-old-bootloader";
const HOWTO_URL: &str = "https://github.com/nesquikm/open-powerz/blob/main/docs/how-to/update-firmware.md#old-bootloader";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn read(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    String::from_utf8_lossy(&bytes).replace("\r\n", "\n")
}

/// Emphasis, code and comment marks and line continuations dropped, every
/// run of white space one space.
fn plain(s: &str) -> String {
    s.replace(['*', '`'], "")
        .split_whitespace()
        .filter(|w| !matches!(*w, "//" | "///" | "//!" | "\\" | "/" | ">"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn doc(name: &str) -> String {
    plain(&read(&root().join("docs").join(name)))
}

fn how_to_section() -> String {
    let path = root()
        .join("docs")
        .join("how-to")
        .join("update-firmware.md");
    let text = read(&path);
    let start = text
        .find("\n## Old bootloader\n")
        .expect("an `## Old bootloader` section");
    let body = &text[start + 1..];
    let end = body[3..].find("\n## ").map_or(body.len(), |i| i + 3);
    plain(&body[..end])
}

// ---------------------------------------------------------------- AC-RXGEC1.1

#[test]
fn rxgec1_the_old_bootloader_mode_label_is_exact() {
    assert_eq!(DeviceMode::LegacyBootloader.label(), MODE);
}

#[test]
fn rxgec1_the_old_bootloader_mode_name_stays() {
    assert_eq!(DeviceMode::LegacyBootloader.name(), "legacy_bootloader");
}

// ---------------------------------------------------------------- AC-RXGEC1.2

fn assert_refusal_rest(text: &str) {
    assert!(text.contains("nothing was sent to it"), "{text}");
    assert!(text.contains(&format!("`{COMMAND}`")), "{text}");
    let lower = text.to_lowercase();
    assert!(
        lower.contains("unplug the meter and plug it back in (no buttons)"),
        "{text}"
    );
    assert!(lower.contains("application firmware"), "{text}");
    assert!(text.contains(HOWTO_URL), "{text}");
}

#[test]
fn rxgec1_the_refusal_says_supported_only_experimentally() {
    assert!(
        LEGACY_BOOTLOADER_HELP.contains(SUPPORT),
        "{LEGACY_BOOTLOADER_HELP}"
    );
}

#[test]
fn rxgec1_the_refusal_keeps_what_it_said() {
    assert_refusal_rest(LEGACY_BOOTLOADER_HELP);
}

#[test]
fn rxgec1_the_switched_refusal_says_supported_only_experimentally() {
    assert!(
        SWITCHED_TO_LEGACY_BOOTLOADER_HELP.contains(SUPPORT),
        "{SWITCHED_TO_LEGACY_BOOTLOADER_HELP}"
    );
}

#[test]
fn rxgec1_the_switched_refusal_keeps_what_it_said() {
    assert_refusal_rest(SWITCHED_TO_LEGACY_BOOTLOADER_HELP);
}

// ---------------------------------------------------------------- AC-RXGEC1.5

fn assert_quotes(what: &str, text: &str, quote: &str) {
    assert!(text.contains(quote), "{what} lacks {quote:?}:\n{text}");
}

#[test]
fn rxgec1_the_how_to_quotes_the_probe_mode() {
    assert_quotes("how-to", &how_to_section(), &format!("Mode: {MODE}"));
}

#[test]
fn rxgec1_the_how_to_quotes_the_device_list_label() {
    assert_quotes("how-to", &how_to_section(), SUFFIX);
}

#[test]
fn rxgec1_the_how_to_quotes_the_refusal() {
    assert_quotes("how-to", &how_to_section(), SUPPORT);
}

#[test]
fn rxgec1_the_cli_reference_quotes_the_refusal() {
    assert_quotes("cli.md", &doc("reference/cli.md"), SUPPORT);
}

#[test]
fn rxgec1_the_desktop_backend_reference_quotes_the_device_list_label() {
    let text = doc("reference/desktop-backend.md");
    assert_quotes("desktop-backend.md", &text, SUFFIX);
}

#[test]
fn rxgec1_the_library_reference_quotes_the_mode_label() {
    let text = doc("reference/api/powerz-protocol.md");
    assert_quotes("powerz-protocol.md", &text, MODE);
}

#[test]
fn rxgec1_the_library_reference_quotes_the_refusal() {
    let text = doc("reference/api/powerz-protocol.md");
    assert_quotes("powerz-protocol.md", &text, SUPPORT);
}

#[test]
fn rxgec1_the_docs_give_the_whole_command() {
    assert_quotes("how-to", &how_to_section(), COMMAND);
    for name in [
        "reference/cli.md",
        "reference/desktop-backend.md",
        "reference/api/powerz-protocol.md",
    ] {
        assert_quotes(name, &doc(name), COMMAND);
    }
}

#[test]
fn rxgec1_no_current_text_gives_the_command_without_its_arguments() {
    let short = "powerz firmware flash --experimental-old-bootloader";
    for name in [
        "how-to/update-firmware.md",
        "reference/cli.md",
        "reference/desktop-backend.md",
        "reference/api/powerz-protocol.md",
    ] {
        assert!(!doc(name).contains(short), "{name} gives {short:?}");
    }
    for text in [LEGACY_BOOTLOADER_HELP, SWITCHED_TO_LEGACY_BOOTLOADER_HELP] {
        assert!(!text.contains(short), "{text}");
    }
}

fn assert_hedged(what: &str, text: &str) {
    assert_quotes(what, text, "tested only in simulation");
    assert_quotes(what, text, "never sends a write below 0x4000");
    assert_quotes(
        what,
        text,
        "a retry after a failed run is expected but not verified",
    );
}

#[test]
fn rxgec1_the_how_to_keeps_the_hedges() {
    assert_hedged("how-to", &how_to_section());
}

#[test]
fn rxgec1_the_cli_reference_keeps_the_hedges() {
    assert_hedged("cli.md", &doc("reference/cli.md"));
}

// ---------------------------------------------------------------- AC-RXGEC1.6

/// The stale phrase with bootloader text within 200 characters of it.
fn stale(text: &str) -> Vec<String> {
    let text = plain(text).to_lowercase();
    let mut found = Vec::new();
    for (at, _) in text.match_indices(STALE) {
        let before: Vec<char> = text[..at].chars().rev().take(200).collect();
        let near: String = before
            .into_iter()
            .rev()
            .chain(text[at..].chars().take(200 + STALE.len()))
            .collect();
        if ["bootloader", "boot loader", "legacy", "2.1"]
            .iter()
            .any(|w| near.contains(w))
        {
            found.push(near);
        }
    }
    found
}

/// Text files under `dir`, without TypeScript tests (they may name the
/// phrase to say it is absent) and the generated wasm output.
fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("list {}: {e}", dir.display())) {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() {
            if name != "wasm" {
                files(&path, out);
            }
        } else if [".rs", ".ts", ".svelte", ".md", ".html", ".json"]
            .iter()
            .any(|ext| name.ends_with(ext))
            && !name.ends_with(".test.ts")
        {
            out.push(path);
        }
    }
}

fn assert_no_stale(dirs: &[PathBuf]) {
    let mut all = Vec::new();
    for dir in dirs {
        let before = all.len();
        files(dir, &mut all);
        assert!(all.len() > before, "no files under {}", dir.display());
    }
    let mut hits = Vec::new();
    for path in &all {
        for near in stale(&read(path)) {
            hits.push(format!("{}: {near}", path.display()));
        }
    }
    assert!(
        hits.is_empty(),
        "the old bootloader is still \"{STALE}\" in:\n{}",
        hits.join("\n")
    );
}

#[test]
fn rxgec1_the_scan_finds_the_phrase_across_a_wrapped_comment() {
    let hits = stale("/// the old bootloader is not\n/// supported yet.");
    assert_eq!(hits.len(), 1, "{hits:?}");
}

#[test]
fn rxgec1_the_scan_ignores_the_phrase_about_something_else() {
    assert!(stale("the Chinese UI is not supported yet").is_empty());
}

#[test]
fn rxgec1_no_crate_source_calls_the_old_bootloader_not_supported_yet() {
    let crates = root().join("crates");
    let dirs: Vec<PathBuf> = std::fs::read_dir(&crates)
        .expect("list crates")
        .map(|e| e.expect("a directory entry").path().join("src"))
        .filter(|p| p.is_dir())
        .collect();
    assert!(!dirs.is_empty(), "no crates under {}", crates.display());
    assert_no_stale(&dirs);
}

#[test]
fn rxgec1_no_web_ui_source_calls_the_old_bootloader_not_supported_yet() {
    assert_no_stale(&[root().join("apps/desktop/src")]);
}

#[test]
fn rxgec1_no_desktop_backend_source_calls_the_old_bootloader_not_supported_yet() {
    assert_no_stale(&[root().join("apps/desktop/src-tauri/src")]);
}

#[test]
fn rxgec1_no_doc_calls_the_old_bootloader_not_supported_yet() {
    assert_no_stale(&[root().join("docs")]);
}
