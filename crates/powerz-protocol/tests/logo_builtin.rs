//! Built-in open-powerz startup logos (FR 70X5YX, AC-70X5YX.1).
//!
//! The two brand meter logos are embedded in the library: `surf` (the
//! mascot) and `surf-wordmark` (the mascot with the lettering). Names,
//! labels and the embedded PNG bytes need no feature; loading and encoding
//! them sits behind `image-files` like every other image decode.

use powerz_protocol::logo::builtin::BuiltinLogo;
use powerz_protocol::logo::{self, LogoMode};

const SURF_PNG: &[u8] = include_bytes!("../../../assets/brand/meter-logo.png");
const WORDMARK_PNG: &[u8] = include_bytes!("../../../assets/brand/meter-logo-wordmark.png");

#[test]
fn _70x5yx_1_there_are_exactly_two_builtins_named_surf_and_surf_wordmark() {
    let names: Vec<&str> = BuiltinLogo::ALL.iter().map(|b| b.name()).collect();
    assert_eq!(names, ["surf", "surf-wordmark"]);
    assert_eq!(BuiltinLogo::Surf.name(), "surf");
    assert_eq!(BuiltinLogo::SurfWordmark.name(), "surf-wordmark");
    assert_eq!(BuiltinLogo::Surf.to_string(), "surf");
    assert_eq!(BuiltinLogo::SurfWordmark.to_string(), "surf-wordmark");
}

#[test]
fn _70x5yx_1_builtins_parse_from_their_names() {
    assert_eq!("surf".parse::<BuiltinLogo>(), Ok(BuiltinLogo::Surf));
    assert_eq!(
        "surf-wordmark".parse::<BuiltinLogo>(),
        Ok(BuiltinLogo::SurfWordmark)
    );
    for b in BuiltinLogo::ALL {
        assert_eq!(b.name().parse::<BuiltinLogo>(), Ok(b));
    }
}

#[test]
fn _70x5yx_1_an_unknown_name_is_refused_and_the_error_lists_both_names() {
    for bad in ["", "nope", "Surf ", "surf_wordmark", "surfwordmark"] {
        let err = bad
            .parse::<BuiltinLogo>()
            .expect_err("only the two names parse");
        let msg = err.to_string();
        assert!(msg.contains("surf"), "{bad:?}: {msg}");
        assert!(msg.contains("surf-wordmark"), "{bad:?}: {msg}");
    }
}

#[test]
fn _70x5yx_1_each_builtin_has_a_distinct_label() {
    let surf = BuiltinLogo::Surf.label();
    let wordmark = BuiltinLogo::SurfWordmark.label();
    assert!(!surf.trim().is_empty());
    assert!(!wordmark.trim().is_empty());
    assert_ne!(surf, wordmark);
}

#[test]
fn _70x5yx_1_the_embedded_pngs_are_the_brand_meter_logos() {
    assert_eq!(
        BuiltinLogo::Surf.png(),
        SURF_PNG,
        "assets/brand/meter-logo.png"
    );
    assert_eq!(
        BuiltinLogo::SurfWordmark.png(),
        WORDMARK_PNG,
        "assets/brand/meter-logo-wordmark.png"
    );
    for b in BuiltinLogo::ALL {
        assert_eq!(&b.png()[..8], b"\x89PNG\r\n\x1a\n", "{b} is a PNG");
    }
}

#[test]
fn _70x5yx_1_builtins_always_use_the_transparent_mode() {
    for b in BuiltinLogo::ALL {
        assert_eq!(b.mode(), LogoMode::Alpha, "{b}");
    }
}

#[cfg(feature = "image-files")]
mod encoded {
    use super::*;

    #[test]
    fn _70x5yx_1_each_builtin_loads_as_the_200x200_brand_png() {
        for (b, png) in [
            (BuiltinLogo::Surf, SURF_PNG),
            (BuiltinLogo::SurfWordmark, WORDMARK_PNG),
        ] {
            let img = b.load().expect("the embedded PNG loads");
            assert_eq!((img.w, img.h), (200, 200), "{b}");
            assert_eq!(img, logo::load_image_bytes(png).expect("loads"), "{b}");
        }
    }

    #[test]
    fn _70x5yx_1_each_builtin_encodes_to_a_valid_200x200_alpha_blob_of_469_chunks() {
        for b in BuiltinLogo::ALL {
            let blob = b.encode().expect("encodes");
            let header = logo::validate_blob(&blob.bytes).expect("the validator accepts it");
            assert_eq!(header.mode, LogoMode::Alpha, "{b}");
            assert_eq!((header.width, header.height), (200, 200), "{b}");
            assert_eq!(header.byte_count, 200 * 200 * 3, "{b}");
            assert_eq!(blob.bytes.len(), 120_016, "{b}");
            let p = blob.preview();
            assert_eq!((p.width, p.height), (200, 200), "{b}");
            assert_eq!(p.mode, LogoMode::Alpha, "{b}");
            assert_eq!(p.padded_size, 120_064, "{b}");
            assert_eq!(p.chunks, 469, "{b}");
            assert!(!p.small_side_warning, "{b}");
            assert_eq!(blob.padded().len(), 469 * 256, "{b}");
        }
    }

    #[test]
    fn _70x5yx_1_the_encoding_is_the_brand_png_in_the_transparent_mode() {
        for (b, png) in [
            (BuiltinLogo::Surf, SURF_PNG),
            (BuiltinLogo::SurfWordmark, WORDMARK_PNG),
        ] {
            let expected = logo::encode(
                &logo::load_image_bytes(png).expect("loads"),
                LogoMode::Alpha,
            )
            .expect("encodes");
            assert_eq!(b.encode().expect("encodes"), expected, "{b}");
        }
    }

    #[test]
    fn _70x5yx_1_the_two_builtins_are_different_pictures() {
        let surf = BuiltinLogo::Surf.encode().expect("encodes");
        let wordmark = BuiltinLogo::SurfWordmark.encode().expect("encodes");
        assert_ne!(surf.bytes, wordmark.bytes);
    }
}
