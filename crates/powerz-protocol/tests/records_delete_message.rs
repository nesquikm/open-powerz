//! AC-4PJRVM.6: the re-read mismatch says when the count matches but the
//! entries differ, rather than "lists 2 records, expected 2".

use powerz_protocol::session::Error;

#[test]
fn same_count_says_the_entries_differ() {
    let text = Error::RecordTableMismatch {
        expected: 2,
        actual: 2,
    }
    .to_string();
    assert!(text.contains("entries differ"), "{text}");
}

#[test]
fn another_count_gives_both_counts_as_before() {
    let text = Error::RecordTableMismatch {
        expected: 2,
        actual: 3,
    }
    .to_string();
    assert_eq!(
        text,
        "after the delete the meter lists 3 records, expected 2"
    );
}
