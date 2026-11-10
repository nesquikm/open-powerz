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

// ---- FR P747X7: the lettered logo leaves an empty strip on the right ----

/// Decode an embedded built-in PNG to RGBA8 with the `image` dev-dependency,
/// so these checks run with or without the `image-files` feature.
fn decode_rgba(b: BuiltinLogo) -> image::RgbaImage {
    image::load_from_memory_with_format(b.png(), image::ImageFormat::Png)
        .expect("the embedded PNG decodes")
        .to_rgba8()
}

fn column_has_opaque_pixel(img: &image::RgbaImage, x: u32) -> bool {
    (0..img.height()).any(|y| img.get_pixel(x, y)[3] > 0)
}

#[test]
fn _p747x7_1_both_builtins_have_opaque_pixels_in_the_leftmost_and_rightmost_columns() {
    for b in BuiltinLogo::ALL {
        let img = decode_rgba(b);
        let last = img.width() - 1;
        assert!(
            column_has_opaque_pixel(&img, 0),
            "{b}: the leftmost column (x = 0) is fully transparent"
        );
        assert!(
            column_has_opaque_pixel(&img, last),
            "{b}: the rightmost column (x = {last}) is fully transparent"
        );
    }
}

#[test]
fn _p747x7_1_the_wordmark_logo_has_no_empty_strip_at_either_edge() {
    let img = decode_rgba(BuiltinLogo::SurfWordmark);
    let w = img.width();
    let empty_right = (0..w)
        .rev()
        .take_while(|&x| !column_has_opaque_pixel(&img, x))
        .count();
    let empty_left = (0..w)
        .take_while(|&x| !column_has_opaque_pixel(&img, x))
        .count();
    assert_eq!(empty_right, 0, "empty columns on the right");
    assert_eq!(empty_left, 0, "empty columns on the left");
}

/// AC-P747X7.2: size and binary alpha. The "look inside the old area is
/// unchanged" half is checked by a pixel diff against the previous PNG
/// outside this test (the old image is not kept in the repo).
#[test]
fn _p747x7_2_the_wordmark_logo_stays_200x200_with_alpha_0_or_255_only() {
    let img = decode_rgba(BuiltinLogo::SurfWordmark);
    assert_eq!((img.width(), img.height()), (200, 200));
    let bad: Vec<(u32, u32, u8)> = img
        .enumerate_pixels()
        .filter(|(_, _, p)| p[3] != 0 && p[3] != 255)
        .map(|(x, y, p)| (x, y, p[3]))
        .take(5)
        .collect();
    assert!(bad.is_empty(), "partial alpha pixels, first few: {bad:?}");
}

/// The `scene(xmin, xmax, ...)` call inside `meter_logo_wordmark()` of the
/// brand generator, as `(xmin, xmax)`.
fn wordmark_scene_range() -> (f64, f64) {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/brand/tools/kit_svgs.py"
    ))
    .expect("the brand generator is readable");
    let start = src
        .find("def meter_logo_wordmark(")
        .expect("meter_logo_wordmark() is defined");
    let rest = &src[start..];
    let body = match rest[1..].find("\ndef ") {
        Some(end) => &rest[..end + 1],
        None => rest,
    };
    let call = body
        .find("scene(")
        .map(|i| &body[i + "scene(".len()..])
        .expect("meter_logo_wordmark() calls scene(");
    let mut args = call.split(',').map(str::trim);
    let xmin: f64 = args
        .next()
        .and_then(|a| a.parse().ok())
        .expect("scene()'s first argument is a number");
    let xmax: f64 = args
        .next()
        .and_then(|a| a.parse().ok())
        .expect("scene()'s second argument is a number");
    assert!(
        body.contains("translate(20,0) scale(0.8)"),
        "the wordmark logo still maps the scene with translate(20,0) scale(0.8)"
    );
    (xmin, xmax)
}

#[test]
fn _p747x7_3_the_generator_draws_the_wordmark_scene_past_the_right_edge() {
    // The left edge already fills (checked on the PNG by AC-P747X7.1), so
    // only the right end of the scene is pinned here; xmin is left alone so
    // the art inside the old area stays as it was.
    let (_xmin, xmax) = wordmark_scene_range();
    // The logo maps scene x to image x as 20 + 0.8 * x on a 200-wide image.
    let right = 20.0 + 0.8 * xmax;
    assert!(
        right >= 200.0 + 2.0,
        "scene ends at image x = {right} (xmax {xmax}); it must bleed past 200"
    );
}
