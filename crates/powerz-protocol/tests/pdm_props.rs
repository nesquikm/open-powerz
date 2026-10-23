//! PDM reply parser robustness (FR PR41FT, AC .3) and classifier totality
//! (AC .4): no input makes the parser or the classifier panic.

use powerz_protocol::pdm::{Capabilities, Command, ReplyReader, Risk, caps, classify};
use proptest::collection::vec;
use proptest::prelude::*;

/// Random bytes biased toward block tags, newlines and frame starts.
fn stream() -> impl Strategy<Value = Vec<u8>> {
    let piece = prop_oneof![
        vec(any::<u8>(), 0..64),
        Just(b"pdo:".to_vec()),
        Just(b"ufcs:".to_vec()),
        Just(b"scp:".to_vec()),
        Just(b"vfcp:".to_vec()),
        Just(b"tfcp:".to_vec()),
        Just(b"\n".to_vec()),
        Just(b"ready:5191mV,0mA\n".to_vec()),
        Just(vec![0x41, 0x01, 0x80, 0x02]),
        Just(vec![0x02, 0x12, 0x05, 0x00]),
        (0u8..=255).prop_map(|n| vec![n]),
    ];
    vec(piece, 0..24).prop_map(|parts| parts.concat())
}

/// Test paths carry `pdm` so `cargo test pdm` selects them.
mod pdm_props {
    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        // ---- AC-PR41FT.3: no input makes the reply parser panic ----

        #[test]
        fn reader_never_panics_and_always_drains(bytes in stream(), cuts in vec(1usize..32, 0..16)) {
            let mut r = ReplyReader::new();
            let mut at: usize = 0;
            let mut items = 0usize;
            for c in cuts.iter().chain(std::iter::once(&usize::MAX)) {
                let end = at.saturating_add(*c).min(bytes.len());
                r.push(&bytes[at..end]);
                at = end;
                while let Some(_item) = r.next_item() {
                    items += 1;
                    prop_assert!(items <= bytes.len() + 1, "more items than bytes");
                }
                if at == bytes.len() {
                    break;
                }
            }
            prop_assert!(r.buffered() <= bytes.len());
        }

        #[test]
        fn block_parser_never_panics(bytes in stream()) {
            for tag in [&b"pdo:"[..], b"ufcs:", b"scp:", b"vfcp:", b"tfcp:"] {
                let mut buf = tag.to_vec();
                buf.extend(&bytes);
                if let Ok(Some((_, len))) = caps::parse(&buf) {
                    prop_assert!(len <= buf.len());
                }
                let _ = caps::block_tag(&buf);
            }
        }

        // ---- AC-PR41FT.4: the classifier is total; raw text is never below Above5V unless parsed ----

        #[test]
        fn classify_is_total_and_unparsed_text_is_above_5v(text in "[ -~]{0,40}") {
            let c = Command::parse(&text);
            let risk = classify(&c, &Capabilities::default());
            if matches!(c, Command::Raw(_)) {
                prop_assert_eq!(risk, Risk::Above5V);
            }
        }
    }
}
