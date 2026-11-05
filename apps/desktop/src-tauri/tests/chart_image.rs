//! Saving a chart image (FR AXHK2V): the PDF writer that wraps the chart's
//! PNG in one page, and the file the native dialog's path receives.
//!
//! API pinned here, beyond the FR's Technical Design:
//!
//! - `chart_image::ChartImageFormat::{Png, Svg, Pdf}`, `from_name("png" |
//!   "svg" | "pdf")` (anything else, GIF, BMP or JPG among them, is `None`);
//! - `chart_image::pdf_from_png(png) -> Result<Vec<u8>, _>`: a one-page PDF
//!   (classic `xref` table) whose page holds the PNG as an image XObject of
//!   the PNG's pixel size, the page in the PNG's aspect ratio; 8-bit RGB and
//!   RGBA PNGs are taken, anything that is not a PNG is refused;
//! - `chart_image::write_chart_image(path, format, data) -> Result<(), _>`:
//!   PNG and SVG bytes are written as given, a PDF is `pdf_from_png(data)`;
//! - like the dialog, both refuse a PNG narrower or wider (or shorter or
//!   taller) than 320 to 7680 px (AC-AXHK2V.5).
//!
//! Every PNG here is built by the test, uncompressed (stored deflate blocks).

use std::path::PathBuf;

use open_powerz_desktop_lib::chart_image::{ChartImageFormat, pdf_from_png, write_chart_image};

// ---- A minimal PNG encoder --------------------------------------------------

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1_u32, 0_u32);
    for &x in bytes {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

/// `raw` as a zlib stream of stored (uncompressed) deflate blocks.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (k, chunk) in chunks.iter().enumerate() {
        out.push(u8::from(k + 1 == chunks.len()));
        let len = u16::try_from(chunk.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// A `width` × `height` 8-bit PNG, RGB (`alpha` false) or RGBA, a gradient.
fn png(width: u32, height: u32, alpha: bool) -> Vec<u8> {
    let channels = if alpha { 4 } else { 3 };
    let mut raw = Vec::new();
    for y in 0..height {
        raw.push(0); // filter: none
        for x in 0..width {
            raw.push((x % 256) as u8);
            raw.push((y % 256) as u8);
            raw.push(128);
            if alpha {
                raw.push(255);
            }
        }
    }
    assert_eq!(raw.len(), height as usize * (1 + width as usize * channels));
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, if alpha { 6 } else { 2 }, 0, 0, 0]);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

// ---- A minimal PDF reader ---------------------------------------------------

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

/// How many dictionaries say `/Type /Page` (and not `/Pages`), however spaced.
fn page_objects(pdf: &[u8]) -> usize {
    let text = String::from_utf8_lossy(pdf);
    let squeezed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let squeezed = squeezed.replace("/Type/", "/Type /");
    squeezed
        .match_indices("/Type /Page")
        .filter(|(at, m)| {
            squeezed[at + m.len()..]
                .chars()
                .next()
                .is_none_or(|c| c != 's')
        })
        .count()
}

/// The number after `key` in the first dictionary that has it, e.g. `/Width`.
fn number_after(pdf: &[u8], key: &str) -> Option<f64> {
    let text = String::from_utf8_lossy(pdf);
    let at = text.find(key)? + key.len();
    let rest = text[at..].trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn media_box(pdf: &[u8]) -> Option<[f64; 4]> {
    let text = String::from_utf8_lossy(pdf);
    let at = text.find("/MediaBox")?;
    let open = at + text[at..].find('[')? + 1;
    let close = open + text[open..].find(']')?;
    let nums: Vec<f64> = text[open..close]
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    nums.try_into().ok()
}

/// Checks the `startxref` / `xref` structure: every in-use object's offset
/// points at `N G obj`. Returns the number of in-use objects.
fn check_xref(pdf: &[u8]) -> usize {
    let at = rfind(pdf, b"startxref").expect("a startxref");
    let tail = String::from_utf8_lossy(&pdf[at + 9..]);
    let offset: usize = tail
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .expect("startxref offset");
    assert!(
        pdf[offset..].starts_with(b"xref"),
        "startxref points at the xref table"
    );
    let table = String::from_utf8_lossy(&pdf[offset + 4..]);
    let mut lines = table.lines().map(str::trim).filter(|l| !l.is_empty());
    let header = lines.next().expect("an xref subsection header");
    let mut parts = header.split_whitespace();
    let first: usize = parts.next().and_then(|n| n.parse().ok()).expect("first");
    let n: usize = parts.next().and_then(|n| n.parse().ok()).expect("count");
    let mut used = 0;
    for k in 0..n {
        let entry = lines.next().expect("an xref entry");
        let fields: Vec<&str> = entry.split_whitespace().collect();
        assert_eq!(fields.len(), 3, "xref entry {entry:?}");
        if fields[2] == "n" {
            let at: usize = fields[0].parse().expect("an offset");
            let object = format!(
                "{} {} obj",
                first + k,
                fields[1].parse::<u32>().unwrap_or(0)
            );
            assert!(
                pdf[at..].starts_with(object.as_bytes()),
                "xref entry {} points at `{object}`",
                first + k
            );
            used += 1;
        }
    }
    used
}

fn check_one_page(pdf: &[u8], width: u32, height: u32) {
    assert!(pdf.starts_with(b"%PDF-1."), "a PDF header");
    let end = pdf
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(0, |p| p + 1);
    assert!(pdf[..end].ends_with(b"%%EOF"), "ends with %%EOF");
    assert!(
        check_xref(pdf) >= 4,
        "catalog, pages, page and image at least"
    );
    assert_eq!(page_objects(pdf), 1, "exactly one page");
    assert!(
        find(pdf, b"/Count 1").is_some(),
        "the page tree counts one page"
    );
    assert!(find(pdf, b"/Catalog").is_some(), "a catalog");
    assert!(count(pdf, b"/Subtype /Image") + count(pdf, b"/Subtype/Image") >= 1);
    assert_eq!(number_after(pdf, "/Width"), Some(f64::from(width)));
    assert_eq!(number_after(pdf, "/Height"), Some(f64::from(height)));
    let [x0, y0, x1, y1] = media_box(pdf).expect("a MediaBox");
    assert!(x1 > x0 && y1 > y0, "a page with an area");
    let page_ratio = (x1 - x0) / (y1 - y0);
    let image_ratio = f64::from(width) / f64::from(height);
    assert!(
        (page_ratio - image_ratio).abs() / image_ratio < 0.01,
        "page {page_ratio} in the image's ratio {image_ratio}"
    );
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "open-powerz-chart-image-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

// ---- The tests --------------------------------------------------------------

#[test]
fn ac_axhk2v_7_an_rgb_png_becomes_a_one_page_pdf() {
    let pdf = pdf_from_png(&png(400, 320, false)).expect("a PDF");
    check_one_page(&pdf, 400, 320);
}

#[test]
fn ac_axhk2v_7_an_rgba_png_as_a_canvas_writes_it_becomes_a_one_page_pdf() {
    let pdf = pdf_from_png(&png(640, 320, true)).expect("a PDF");
    check_one_page(&pdf, 640, 320);
}

#[test]
fn ac_axhk2v_7_a_tall_image_keeps_its_aspect_ratio() {
    let pdf = pdf_from_png(&png(320, 900, false)).expect("a PDF");
    check_one_page(&pdf, 320, 900);
}

#[test]
fn ac_axhk2v_7_anything_but_a_png_is_refused() {
    assert!(pdf_from_png(b"not a png").is_err());
    assert!(pdf_from_png(&[]).is_err());
    let mut broken = png(400, 300, false);
    broken.truncate(40);
    assert!(pdf_from_png(&broken).is_err());
}

#[test]
fn ac_axhk2v_5_only_png_svg_and_pdf_are_formats() {
    assert_eq!(
        ChartImageFormat::from_name("png"),
        Some(ChartImageFormat::Png)
    );
    assert_eq!(
        ChartImageFormat::from_name("svg"),
        Some(ChartImageFormat::Svg)
    );
    assert_eq!(
        ChartImageFormat::from_name("pdf"),
        Some(ChartImageFormat::Pdf)
    );
    for other in ["gif", "bmp", "jpg", "jpeg", "", "PNG "] {
        assert_eq!(ChartImageFormat::from_name(other), None, "{other:?}");
    }
}

#[test]
fn ac_axhk2v_5_sizes_outside_320_to_7680_px_are_refused() {
    for (w, h) in [(319, 400), (400, 319), (7681, 400), (400, 7681)] {
        let image = png(w, h, false);
        assert!(pdf_from_png(&image).is_err(), "{w} × {h} as a PDF");
        let dir = temp_dir(&format!("refuse-{w}-{h}"));
        let path = dir.join("chart.png");
        assert!(
            write_chart_image(&path, ChartImageFormat::Png, &image).is_err(),
            "{w} × {h} as a PNG"
        );
        assert!(!path.exists(), "nothing written for {w} × {h}");
        let _ = std::fs::remove_dir_all(dir);
    }
    for (w, h) in [(320, 320), (7680, 320)] {
        assert!(pdf_from_png(&png(w, h, false)).is_ok(), "{w} × {h}");
    }
}

#[test]
fn ac_axhk2v_5_writes_the_file_in_the_chosen_format() {
    let dir = temp_dir("write");
    let image = png(400, 320, true);

    let png_path = dir.join("powerz-chart-20261102-090507.png");
    write_chart_image(&png_path, ChartImageFormat::Png, &image).expect("PNG written");
    assert_eq!(std::fs::read(&png_path).expect("PNG read back"), image);

    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="320"/>"#;
    let svg_path = dir.join("powerz-chart-20261102-090507.svg");
    write_chart_image(&svg_path, ChartImageFormat::Svg, svg).expect("SVG written");
    assert_eq!(
        std::fs::read(&svg_path).expect("SVG read back"),
        svg.to_vec()
    );

    let pdf_path = dir.join("powerz-chart-20261102-090507.pdf");
    write_chart_image(&pdf_path, ChartImageFormat::Pdf, &image).expect("PDF written");
    let pdf = std::fs::read(&pdf_path).expect("PDF read back");
    check_one_page(&pdf, 400, 320);

    let _ = std::fs::remove_dir_all(dir);
}
