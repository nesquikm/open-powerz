//! Startup logo file loading (FR PVFHM7, AC-PVFHM7.8) and the docs section
//! (AC-PVFHM7.9).
//!
//! Test images are generated here at run time; no fixtures are copied from
//! anywhere. The loader tests only build with the `image-files` feature.

use std::path::PathBuf;

fn repo_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn read(rel: &str) -> String {
    let p = repo_file(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

// ---- AC-PVFHM7.8: the loader is optional and the core stays I/O-free ----

#[test]
fn ac8_image_files_feature_is_optional_and_minimal() {
    let toml = read("crates/powerz-protocol/Cargo.toml");
    let deps = toml
        .split("[dependencies]")
        .nth(1)
        .and_then(|s| s.split("\n[").next())
        .unwrap_or_default();
    let line = deps
        .lines()
        .find(|l| l.trim_start().starts_with("image "))
        .expect("`image` dependency in [dependencies]");
    assert!(line.contains("optional = true"), "{line}");
    assert!(line.contains("default-features = false"), "{line}");
    for f in ["\"png\"", "\"jpeg\"", "\"bmp\""] {
        assert!(line.contains(f), "{line} lacks {f}");
    }
    assert_eq!(
        line.matches('"').count(),
        2 + 6,
        "only png/jpeg/bmp: {line}"
    );
    assert!(toml.contains("image-files = [\"dep:image\"]"));
    let default = toml
        .lines()
        .find(|l| l.starts_with("default ="))
        .unwrap_or_default();
    assert!(
        !default.contains("image-files"),
        "feature must stay off by default"
    );
}

#[test]
fn ac8_core_logo_module_gates_all_file_io() {
    let src = read("crates/powerz-protocol/src/logo.rs");
    let core = src.split("#[cfg(test)]").next().unwrap_or_default();
    let lines: Vec<&str> = core.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        let code = l.split("//").next().unwrap_or_default();
        if code.contains("image::") || code.contains("std::fs") {
            // Some cfg(feature = "image-files") must precede it within the
            // enclosing item (look back a few lines).
            let gated = lines[i.saturating_sub(40)..i]
                .iter()
                .any(|p| p.contains("cfg(feature = \"image-files\")"));
            assert!(gated, "ungated file/image use at logo.rs:{}: {l}", i + 1);
        }
    }
}

#[cfg(feature = "image-files")]
mod loader {
    use image::{ImageEncoder, codecs};
    use powerz_protocol::logo::{self, LogoMode, Rgba};

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("powerz-logo-test-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).expect("tmp dir");
        d
    }

    fn rgba_image(w: u32, h: u32) -> Vec<u8> {
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[(x * 16) as u8, (y * 16) as u8, 0x80, 0xFF]);
            }
        }
        px
    }

    fn encode_with(w: u32, h: u32, kind: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let px = rgba_image(w, h);
        match kind {
            "png" => codecs::png::PngEncoder::new(&mut out)
                .write_image(&px, w, h, image::ExtendedColorType::Rgba8)
                .expect("png"),
            "bmp" => codecs::bmp::BmpEncoder::new(&mut out)
                .write_image(&px, w, h, image::ExtendedColorType::Rgba8)
                .expect("bmp"),
            "jpeg" => {
                let rgb: Vec<u8> = px.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
                codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95)
                    .write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)
                    .expect("jpeg")
            }
            _ => unreachable!(),
        }
        out
    }

    // Minimal hand-built PNG (stored deflate) so palette / grayscale inputs
    // need no encoder support.
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32fast::hash(&c).to_be_bytes());
    }

    fn adler32(d: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in d {
            a = (a + x as u32) % 65_521;
            b = (b + a) % 65_521;
        }
        (b << 16) | a
    }

    fn hand_png(
        w: u32,
        h: u32,
        color_type: u8,
        extra: &[(&[u8; 4], Vec<u8>)],
        rows: &[Vec<u8>],
    ) -> Vec<u8> {
        let mut raw = Vec::new();
        for r in rows {
            raw.push(0); // filter: none
            raw.extend_from_slice(r);
        }
        assert!(raw.len() < 65_535);
        let mut z = vec![0x78, 0x01, 0x01];
        z.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
        z.extend_from_slice(&raw);
        z.extend_from_slice(&adler32(&raw).to_be_bytes());
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, color_type, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        for (k, d) in extra {
            chunk(&mut out, k, d);
        }
        chunk(&mut out, b"IDAT", &z);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    fn px(img: &Rgba, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.w + x) * 4) as usize;
        [img.px[i], img.px[i + 1], img.px[i + 2], img.px[i + 3]]
    }

    #[test]
    fn ac8_loads_rgba_png_exactly() {
        let img = logo::load_image_bytes(&encode_with(8, 6, "png")).expect("png");
        assert_eq!((img.w, img.h), (8, 6));
        assert_eq!(img.px, rgba_image(8, 6));
    }

    #[test]
    fn ac8_loads_bmp_and_jpeg() {
        let bmp = logo::load_image_bytes(&encode_with(8, 6, "bmp")).expect("bmp");
        assert_eq!((bmp.w, bmp.h), (8, 6));
        assert_eq!(bmp.px.len(), 8 * 6 * 4);
        assert_eq!(&bmp.px[..], &rgba_image(8, 6)[..]);
        let jpg = logo::load_image_bytes(&encode_with(16, 16, "jpeg")).expect("jpeg");
        assert_eq!((jpg.w, jpg.h), (16, 16));
        assert_eq!(jpg.px.len(), 16 * 16 * 4);
        assert!(jpg.px.chunks(4).all(|p| p[3] == 255), "jpeg is opaque RGBA");
        // Lossy, but the flat blue channel stays near 0x80.
        assert!(jpg.px.chunks(4).all(|p| p[2].abs_diff(0x80) < 40));
    }

    #[test]
    fn ac8_palette_png_converts_to_rgba8888() {
        // 2-entry palette: red, and blue with tRNS alpha 0.
        let plte = vec![0xF8, 0x00, 0x00, 0x00, 0x00, 0xF8];
        let trns = vec![0xFF, 0x00];
        let rows: Vec<Vec<u8>> = (0..4).map(|y| vec![(y % 2) as u8, 1, 0, 1]).collect();
        let png = hand_png(4, 4, 3, &[(b"PLTE", plte), (b"tRNS", trns)], &rows);
        let img = logo::load_image_bytes(&png).expect("palette png");
        assert_eq!((img.w, img.h), (4, 4));
        assert_eq!(img.px.len(), 4 * 4 * 4);
        assert_eq!(px(&img, 0, 0), [0xF8, 0, 0, 0xFF]);
        assert_eq!(px(&img, 1, 0), [0, 0, 0xF8, 0x00]);
        assert_eq!(px(&img, 0, 1), [0, 0, 0xF8, 0x00]);
        // And it then encodes with the real alpha (the vendor's bug avoided).
        let big: Vec<Vec<u8>> = (0..48)
            .map(|_| (0..48).map(|x| (x % 2) as u8).collect())
            .collect();
        let png = hand_png(
            48,
            48,
            3,
            &[
                (b"PLTE", vec![0xF8, 0, 0, 0, 0, 0xF8]),
                (b"tRNS", vec![0xFF, 0x00]),
            ],
            &big,
        );
        let img = logo::load_image_bytes(&png).expect("palette 48");
        let blob = logo::encode(&img, LogoMode::Alpha).expect("encode");
        assert_eq!(&blob.bytes[16..22], &[0x00, 0xF8, 0xFF, 0x1F, 0x00, 0x00]);
    }

    #[test]
    fn ac8_grayscale_png_converts_to_rgba8888() {
        let rows: Vec<Vec<u8>> = (0..3)
            .map(|y| vec![0, 0x80, 0xFF, (y * 10) as u8])
            .collect();
        let png = hand_png(4, 3, 0, &[], &rows);
        let img = logo::load_image_bytes(&png).expect("gray png");
        assert_eq!((img.w, img.h), (4, 3));
        assert_eq!(px(&img, 0, 0), [0, 0, 0, 255]);
        assert_eq!(px(&img, 1, 0), [0x80, 0x80, 0x80, 255]);
        assert_eq!(px(&img, 2, 2), [0xFF, 0xFF, 0xFF, 255]);
        assert_eq!(px(&img, 3, 2), [20, 20, 20, 255]);
    }

    #[test]
    fn ac8_load_file_by_extension_including_raw_bin() {
        let dir = tmp_dir("files");
        let png_path = dir.join("logo.png");
        std::fs::write(&png_path, encode_with(8, 6, "png")).expect("write png");
        let png = logo::load_file(&png_path).expect("png file");
        assert_eq!(png.px, rgba_image(8, 6));

        // Raw file: 4-byte info word then pixels = the blob from offset 12.
        let mut src = Vec::new();
        for y in 0..48u32 {
            for x in 0..48u32 {
                src.extend_from_slice(&[
                    ((x * 8) & 0xF8) as u8,
                    ((y * 4) & 0xFC) as u8,
                    0x18,
                    0xFF,
                ]);
            }
        }
        let src = Rgba {
            w: 48,
            h: 48,
            px: src,
        };
        let blob = logo::encode(&src, LogoMode::Opaque).expect("encode");
        let bin_path = dir.join("logo.bin");
        std::fs::write(&bin_path, &blob.bytes[12..]).expect("write bin");
        assert_eq!(logo::load_file(&bin_path).expect("bin file"), src);

        let junk = dir.join("junk.png");
        std::fs::write(&junk, b"not an image").expect("write junk");
        assert!(matches!(
            logo::load_file(&junk),
            Err(logo::LogoError::Load(_))
        ));
        assert!(logo::load_file(&dir.join("missing.png")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ac8_garbage_bytes_are_an_error_not_a_panic() {
        assert!(logo::load_image_bytes(&[]).is_err());
        assert!(logo::load_image_bytes(&[0x89, b'P', b'N', b'G']).is_err());
    }
}

// ---- AC-PVFHM7.9: docs/protocol.md "Startup logo" section ----

fn logo_section() -> String {
    let doc = read("docs/protocol.md");
    let mut found = Vec::new();
    let mut cur: Option<String> = None;
    for line in doc.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            if let Some(s) = cur.take() {
                found.push(s);
            }
            if h.to_lowercase().contains("startup logo") {
                cur = Some(String::new());
            }
        } else if let Some(s) = cur.as_mut() {
            s.push_str(line);
            s.push('\n');
        }
    }
    if let Some(s) = cur {
        found.push(s);
    }
    assert_eq!(
        found.len(),
        1,
        "expected exactly one `## ... Startup logo` section"
    );
    found.remove(0)
}

#[test]
fn ac9_doc_has_startup_logo_section_with_key_facts() {
    let s = logo_section();
    let lower = s.to_lowercase();
    for fact in [
        "info word",
        "rgb565",
        "little-endian",
        "alpha",
        "240",
        "200",
        "48",
        "0x10e3c004",
        "0x19032005",
        "0x0600c004",
        "64 816",
        "120 016",
        "256",
        "0xff",
        "aspect",
        "odd",
    ] {
        assert!(lower.contains(fact), "Startup logo section lacks {fact:?}");
    }
    for bits in ["0–4", "5–9", "10–20", "21–31"] {
        assert!(
            s.contains(bits),
            "Startup logo section lacks bit range {bits}"
        );
    }
}

#[test]
fn ac9_doc_logo_facts_are_tagged() {
    let s = logo_section();
    assert!(s.contains("[static]"), "needs [static] tags");
    assert!(s.contains("[inferred]"), "needs [inferred] tags");
    // Every list item stating a fact carries a confidence tag.
    let tags = ["[device]", "[static]", "[doc]", "[inferred]"];
    for line in s.lines() {
        let t = line.trim();
        if t.starts_with("- ") && t.len() > 20 {
            assert!(
                tags.iter().any(|g| t.contains(g)),
                "untagged fact in Startup logo section: {t}"
            );
        }
    }
}
