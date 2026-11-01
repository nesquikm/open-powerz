//! The firmware version policy (AC-AXHK2M.5): a pre-release sorts below its
//! release and keeps its tag, a tag must not be empty, and a number is digits
//! only. Every version here is made up.

use powerz_protocol::firmware::version::{Version, warnings};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap_or_else(|| panic!("{s:?} did not parse"))
}

#[test]
fn a_pre_release_compares_below_its_release() {
    assert!(v("2.0.7-rc.1") < v("2.0.7"));
    assert!(v("2.0.7") > v("2.0.7-rc.1"));
    assert!(v("2.0.7-rc.1") > v("2.0.6"));
    assert!(v("2.0.7-rc.1") < v("2.0.8-rc.1"));
}

#[test]
fn pre_releases_of_one_version_are_told_apart() {
    assert_ne!(v("2.0.7-beta"), v("2.0.7-rc"));
    assert!(v("2.0.7-alpha") < v("2.0.7-beta"));
    assert!(v("2.0.7-rc.1") < v("2.0.7-rc.2"));
    assert_eq!(v("2.0.7-rc.1"), v("v2.0.7-rc.1"));
}

#[test]
fn display_keeps_the_tag() {
    assert_eq!(v("2.0.7-beta").to_string(), "2.0.7-beta");
    assert_eq!(v("v2.0.7-rc.1").to_string(), "2.0.7-rc.1");
    assert_eq!(v("2.0.7-beta+5").to_string(), "2.0.7-beta");
    assert_eq!(v("2.0.7").to_string(), "2.0.7");
    assert_eq!(v("2.0.7+build5").to_string(), "2.0.7");
    assert_eq!(v("2.1").to_string(), "2.1.0");
}

#[test]
fn an_empty_tag_is_refused() {
    for s in ["2.0.7-", "v2.0.7-", "2.0.7-+5", "2-", " 2.0.7- "] {
        assert_eq!(Version::parse(s), None, "{s:?}");
    }
}

#[test]
fn a_number_with_a_leading_plus_is_refused() {
    for s in [
        "+2.0.7", "v+2.0.7", "2.+0.7", "2.0.+7", "+2", "2.0.7-+", "++2",
    ] {
        assert_eq!(Version::parse(s), None, "{s:?}");
    }
}

#[test]
fn a_tagged_string_gets_the_warning_of_its_order() {
    assert_eq!(warnings("2.0.7-").len(), 0, "unparsed: no version warning");
    assert_eq!(warnings("2.0.7-rc.1").len(), 1, "below 2.0.7: outdated");
}

#[test]
fn the_policy_doc_covers_the_tag_rules() {
    let doc = include_str!("../../../docs/protocol.md");
    for needle in ["`2.0.7-`", "keeps its tag", "leading `+`"] {
        assert!(doc.contains(needle), "docs/protocol.md lacks {needle}");
    }
    let source = include_str!("../src/firmware/version.rs");
    assert!(source.contains("`2.0.7-`"), "Version::parse lacks the rule");
}
