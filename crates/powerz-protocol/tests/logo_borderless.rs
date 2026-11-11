//! FR BZJGJ5: the built-in startup logos are cut by their frame.
//!
//! The meter shows the 200 by 200 logo on its own background, so the art must
//! end inside the picture: a transparent margin at the border, binary alpha,
//! and only dark outline ink where the art meets the transparent area.

#![cfg(feature = "image-files")]

use powerz_protocol::logo::Rgba;
use powerz_protocol::logo::builtin::BuiltinLogo;

/// Width of the fully transparent band at every side (AC-BZJGJ5.1).
const MARGIN: u32 = 3;

/// The brightest channel value an outline pixel may have (AC-BZJGJ5.3):
/// ink `#111` pre-blended toward the navy `#1a1f4a` stays below this.
const INK_MAX: u8 = 0x50;

fn load(b: BuiltinLogo) -> Rgba {
    b.load().expect("the embedded PNG loads")
}

fn pixel(img: &Rgba, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.w + x) * 4) as usize;
    [img.px[i], img.px[i + 1], img.px[i + 2], img.px[i + 3]]
}

fn brand_file(name: &str) -> String {
    let path = format!("{}/../../assets/brand/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

// ---- AC-BZJGJ5.1 ----

#[test]
fn _bzjgj5_1_every_pixel_within_3_px_of_the_border_is_fully_transparent() {
    for b in BuiltinLogo::ALL {
        let img = load(b);
        let mut offenders = Vec::new();
        for y in 0..img.h {
            for x in 0..img.w {
                let in_margin =
                    x < MARGIN || y < MARGIN || x >= img.w - MARGIN || y >= img.h - MARGIN;
                if in_margin && pixel(&img, x, y)[3] != 0 {
                    offenders.push((x, y));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "{b}: {} opaque pixels in the {MARGIN}-px border margin, first few: {:?}",
            offenders.len(),
            &offenders[..offenders.len().min(8)]
        );
    }
}

#[test]
fn _bzjgj5_1_no_side_of_either_logo_is_touched_by_the_art() {
    for b in BuiltinLogo::ALL {
        let img = load(b);
        let (w, h) = (img.w, img.h);
        let left = (0..h).filter(|&y| pixel(&img, 0, y)[3] != 0).count();
        let right = (0..h).filter(|&y| pixel(&img, w - 1, y)[3] != 0).count();
        let top = (0..w).filter(|&x| pixel(&img, x, 0)[3] != 0).count();
        let bottom = (0..w).filter(|&x| pixel(&img, x, h - 1)[3] != 0).count();
        assert_eq!(
            (left, right, top, bottom),
            (0, 0, 0, 0),
            "{b}: opaque pixels on the (left, right, top, bottom) edge"
        );
    }
}

// ---- AC-BZJGJ5.2 ----

#[test]
fn _bzjgj5_2_both_logos_stay_200x200_with_alpha_0_or_255_only() {
    for b in BuiltinLogo::ALL {
        let img = load(b);
        assert_eq!((img.w, img.h), (200, 200), "{b}");
        assert_eq!(img.px.len(), 200 * 200 * 4, "{b}");
        let bad: Vec<(u32, u32, u8)> = (0..img.h)
            .flat_map(|y| (0..img.w).map(move |x| (x, y)))
            .map(|(x, y)| (x, y, pixel(&img, x, y)[3]))
            .filter(|&(_, _, a)| a != 0 && a != 255)
            .take(5)
            .collect();
        assert!(bad.is_empty(), "{b}: partial alpha pixels: {bad:?}");
    }
}

#[test]
fn _bzjgj5_2_both_logos_still_carry_the_mascot_and_its_sea() {
    for b in BuiltinLogo::ALL {
        let img = load(b);
        let all: Vec<[u8; 4]> = (0..img.h)
            .flat_map(|y| (0..img.w).map(move |x| (x, y)))
            .map(|(x, y)| pixel(&img, x, y))
            .collect();
        let opaque = all.iter().filter(|p| p[3] == 255).count();
        // The electron's yellow (#ffd23f) and the sea's blue (#2f6bff).
        let yellow = all
            .iter()
            .filter(|p| p[3] == 255 && p[0] > 0xe0 && p[1] > 0xb0 && p[2] < 0x80)
            .count();
        let blue = all
            .iter()
            .filter(|p| p[3] == 255 && p[0] < 0x70 && p[1] > 0x40 && p[1] < 0x90 && p[2] > 0xd0)
            .count();
        assert!(
            opaque > 200 * 200 / 5,
            "{b}: only {opaque} opaque pixels; the mascot is missing"
        );
        assert!(yellow > 400, "{b}: only {yellow} yellow electron pixels");
        assert!(blue > 1500, "{b}: only {blue} blue sea pixels");
    }
}

#[test]
fn _bzjgj5_2_the_sources_keep_the_electron_in_shades_on_a_sine_wave() {
    let plain = brand_file("meter-logo.svg");
    assert!(
        plain.contains("sine wave"),
        "meter-logo.svg: the sine wave sea"
    );
    for (name, svg) in [
        ("meter-logo.svg", plain.as_str()),
        (
            "meter-logo-wordmark.svg",
            brand_file("meter-logo-wordmark.svg").as_str(),
        ),
    ] {
        assert!(
            svg.contains(r##"<circle cx="100" cy="62" r="25" fill="#ffd23f""##),
            "{name}: the electron's head"
        );
        assert!(
            svg.contains(r##"<rect x="81" y="53" width="16" height="11" rx="5" fill="#111""##)
                && svg.contains(
                    r##"<rect x="103" y="53" width="16" height="11" rx="5" fill="#111""##
                ),
            "{name}: the shades"
        );
    }
    let generator = brand_file("tools/kit_svgs.py");
    assert!(
        generator.contains("def wave_line("),
        "kit_svgs.py still draws the wave"
    );
}

#[test]
fn _bzjgj5_2_the_lettered_logo_keeps_the_open_powerz_lettering() {
    let svg = brand_file("meter-logo-wordmark.svg");
    let hrefs: Vec<&str> = svg
        .split(r##"href="#"##)
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    let lettering = [
        "l-o", "l-p", "l-e", "l-n", "s-dash", "l-p", "l-o", "l-w", "l-e", "l-r", "l-z",
    ];
    assert!(
        hrefs.windows(lettering.len()).any(|w| w == lettering),
        "meter-logo-wordmark.svg no longer spells open-powerz with glyph uses"
    );
}

// ---- AC-BZJGJ5.3 ----

#[test]
fn _bzjgj5_3_every_opaque_pixel_next_to_a_transparent_one_is_dark_ink() {
    for b in BuiltinLogo::ALL {
        let img = load(b);
        let (w, h) = (img.w as i64, img.h as i64);
        let transparent = |x: i64, y: i64| {
            x < 0 || y < 0 || x >= w || y >= h || pixel(&img, x as u32, y as u32)[3] == 0
        };
        let mut fringe = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let p = pixel(&img, x as u32, y as u32);
                if p[3] == 0 {
                    continue;
                }
                let at_edge = transparent(x - 1, y)
                    || transparent(x + 1, y)
                    || transparent(x, y - 1)
                    || transparent(x, y + 1);
                if at_edge && p[..3].iter().any(|&c| c > INK_MAX) {
                    fringe.push((x, y, [p[0], p[1], p[2]]));
                }
            }
        }
        assert!(
            fringe.is_empty(),
            "{b}: {} light edge pixels, first few: {:?}",
            fringe.len(),
            &fringe[..fringe.len().min(8)]
        );
    }
}

// ---- AC-BZJGJ5.4 ----

#[test]
fn _bzjgj5_4_the_brand_readme_states_the_transparent_margin_rule() {
    let readme = brand_file("README.md");
    assert!(
        readme.contains("transparent margin"),
        "assets/brand/README.md must state that the meter logos keep a transparent margin"
    );
}

#[test]
fn _bzjgj5_4_the_render_script_writes_on_meter_previews_of_both_logos() {
    let src = brand_file("tools/render.py");
    assert!(
        src.contains("-onmeter.png"),
        "render.py writes <name>-onmeter.png"
    );
    for name in [r#""meter-logo""#, r#""meter-logo-wordmark""#] {
        let call = format!("meter_png({name}");
        assert!(
            src.contains(&call),
            "render.py renders {name} via meter_png"
        );
    }
}
