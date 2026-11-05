//! Startup logo image encoder and decoder (FR PVFHM7).
//!
//! Turns an RGBA image into the vendor-shaped startup logo blob and back.
//! Pure code: no I/O, nothing is sent to a meter. File loading (PNG, JPEG,
//! BMP, raw `.bin`) sits behind the optional `image-files` feature so the
//! core builds for wasm32 without it. See `docs/protocol.md`, "Startup logo".

/// Smallest side the box clamps up to \[static\].
pub const MIN_SIDE: u32 = 48;
/// Box limit per side in opaque (RGB565) mode \[static\].
pub const OPAQUE_LIMIT: u32 = 240;
/// Box limit per side in alpha (RGB565 + alpha byte) mode \[static\].
pub const ALPHA_LIMIT: u32 = 200;
/// Largest unpadded blob (alpha 200×200 plus the 16-byte header).
pub const MAX_BLOB: usize = 120_016;
/// Size of the blob header in bytes.
pub const HEADER_LEN: usize = 16;
/// Write chunk size; the written blob is padded with 0xFF to a multiple.
pub const CHUNK: usize = 256;

/// Pixel format of the logo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoMode {
    /// RGB565 little-endian, 2 bytes per pixel, format code 4.
    Opaque,
    /// RGB565 little-endian then one alpha byte, 3 bytes per pixel, format code 5.
    Alpha,
}

impl LogoMode {
    /// The mode for a format code, `None` for anything but 4 or 5.
    pub fn from_code(code: u32) -> Option<Self> {
        match code {
            4 => Some(LogoMode::Opaque),
            5 => Some(LogoMode::Alpha),
            _ => None,
        }
    }
    /// Format code stored in bits 0–4 of the info word.
    pub fn format_code(self) -> u32 {
        match self {
            LogoMode::Opaque => 4,
            LogoMode::Alpha => 5,
        }
    }
    /// Box limit per side for this mode.
    pub fn limit(self) -> u32 {
        match self {
            LogoMode::Opaque => OPAQUE_LIMIT,
            LogoMode::Alpha => ALPHA_LIMIT,
        }
    }
    /// Bytes per encoded pixel.
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            LogoMode::Opaque => 2,
            LogoMode::Alpha => 3,
        }
    }
}

/// An RGBA8888 image, row-major, top row first (`px.len() == w * h * 4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

/// Parsed, validated blob header fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoHeader {
    pub mode: LogoMode,
    pub width: u32,
    pub height: u32,
    /// Exact pixel byte count (`width * height * bytes_per_pixel`).
    pub byte_count: u32,
}

/// An encoded logo: 16-byte header followed by the pixel rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoBlob {
    pub info: u32,
    /// Unpadded blob bytes (header + pixels).
    pub bytes: Vec<u8>,
}

/// Width and height (bits 10–20 and 21–31) of an info word.
fn info_sides(info: u32) -> (u32, u32) {
    ((info >> 10) & 0x7FF, (info >> 21) & 0x7FF)
}

/// `len` rounded up to a multiple of [`CHUNK`].
pub(crate) const fn padded_len(len: usize) -> usize {
    len.div_ceil(CHUNK) * CHUNK
}

impl LogoBlob {
    /// The blob as written: padded with 0xFF to a multiple of [`CHUNK`].
    pub fn padded(&self) -> Vec<u8> {
        let mut out = self.bytes.clone();
        out.resize(padded_len(out.len()), 0xFF);
        out
    }
    /// Preview data for the UI.
    pub fn preview(&self) -> Preview {
        let (width, height) = info_sides(self.info);
        let mode = LogoMode::from_code(self.info & 0x1F).unwrap_or(LogoMode::Opaque);
        let blob_size = self.bytes.len();
        let padded_size = padded_len(blob_size);
        Preview {
            width,
            height,
            mode,
            blob_size,
            padded_size,
            chunks: padded_size / CHUNK,
            small_side_warning: width < MIN_SIDE || height < MIN_SIDE,
        }
    }
}

/// What the preview shows about an encoded logo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub mode: LogoMode,
    /// Unpadded blob size in bytes.
    pub blob_size: usize,
    /// Padded (written) size in bytes.
    pub padded_size: usize,
    /// Number of [`CHUNK`]-byte chunks written.
    pub chunks: usize,
    /// Either side is below [`MIN_SIDE`].
    pub small_side_warning: bool,
}

/// Every way encoding or decoding can refuse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LogoError {
    #[error("pixel format code {0} is not 4 or 5")]
    BadFormat(u32),
    #[error("logo side {width}×{height} is 0 or above the mode's limit")]
    BadSide { width: u32, height: u32 },
    #[error("logo width {0} is odd")]
    OddWidth(u32),
    #[error("pixel byte count {got} does not match width × height × bpp = {expected}")]
    ByteCount { expected: u32, got: u32 },
    #[error("info word {0:#010x} has reserved bits 5–9 set")]
    ReservedBits(u32),
    #[error("logo blob of {0} bytes exceeds 120 016 bytes")]
    TooLarge(usize),
    #[error("RGBA buffer length does not match its size")]
    BadImage,
    #[error("unrecognised logo data")]
    Unrecognised,
    #[error("cannot load image file: {0}")]
    Load(String),
}

/// Builds the info word: format bits 0–4, zero bits 5–9, width bits 10–20,
/// height bits 21–31.
pub fn info_word(format: u32, width: u32, height: u32) -> u32 {
    (format & 0x1F) | ((width & 0x7FF) << 10) | ((height & 0x7FF) << 21)
}

/// Final logo size for a source of `w × h` in `mode` (box clamp, aspect
/// kept, odd width rescaled down to even).
pub fn target_size(w: u32, h: u32, mode: LogoMode) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (0, 0);
    }
    let (w64, h64) = (u64::from(w), u64::from(h));
    let bw = u64::from(w.clamp(MIN_SIDE, mode.limit()));
    let bh = u64::from(h.clamp(MIN_SIDE, mode.limit()));
    let (mut tw, mut th) = if w64 * bh >= h64 * bw {
        (bw, (h64 * bw / w64).max(1))
    } else {
        ((w64 * bh / h64).max(1), bh)
    };
    if tw % 2 == 1 && tw > 1 {
        tw -= 1;
        th = (h64 * tw / w64).max(1);
    }
    (tw as u32, th as u32)
}

/// Scales `img` to [`target_size`] with smooth filtering.
pub fn scale_to_box(img: &Rgba, mode: LogoMode) -> Rgba {
    let (tw, th) = target_size(img.w, img.h, mode);
    if (tw, th) == (img.w, img.h) {
        return img.clone();
    }
    let (sw, sh) = (img.w as usize, img.h as usize);
    let (dw, dh) = (tw as usize, th as usize);
    let src = |i: usize| f32::from(img.px.get(i).copied().unwrap_or(0));
    // Horizontal pass into an f32 buffer of dw × sh, then vertical pass.
    let xw = filter_weights(sw, dw);
    let mut mid = vec![0f32; dw * sh * 4];
    for y in 0..sh {
        for (x, taps) in xw.iter().enumerate() {
            for c in 0..4 {
                mid[(y * dw + x) * 4 + c] = taps
                    .iter()
                    .map(|&(j, w)| src((y * sw + j) * 4 + c) * w)
                    .sum();
            }
        }
    }
    let yw = filter_weights(sh, dh);
    let mut px = Vec::with_capacity(dw * dh * 4);
    for taps in &yw {
        for x in 0..dw {
            for c in 0..4 {
                let v: f32 = taps
                    .iter()
                    .map(|&(j, w)| mid[(j * dw + x) * 4 + c] * w)
                    .sum();
                px.push(v.round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    Rgba { w: tw, h: th, px }
}

/// Triangle-filter taps for resampling `src` samples to `dst`: bilinear when
/// upscaling, a widened (area-like) triangle when downscaling. Edges clamp;
/// each output's weights sum to 1.
fn filter_weights(src: usize, dst: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = src as f32 / dst as f32;
    let support = scale.max(1.0);
    let last = src.saturating_sub(1) as isize;
    (0..dst)
        .map(|i| {
            let centre = (i as f32 + 0.5) * scale - 0.5;
            let lo = (centre - support).floor() as isize;
            let hi = (centre + support).ceil() as isize;
            let mut taps: Vec<(usize, f32)> = Vec::new();
            for j in lo..=hi {
                let w = 1.0 - ((j as f32 - centre).abs() / support);
                if w > 0.0 {
                    taps.push((j.clamp(0, last) as usize, w));
                }
            }
            let total: f32 = taps.iter().map(|t| t.1).sum();
            if total > 0.0 {
                taps.iter_mut().for_each(|t| t.1 /= total);
            } else {
                taps = vec![((centre.round() as isize).clamp(0, last) as usize, 1.0)];
            }
            taps
        })
        .collect()
}

/// Checks an info word and pixel byte count against every invariant.
/// Shared with the transport guard.
pub fn validate_header(info: u32, byte_count: u32) -> Result<LogoHeader, LogoError> {
    let code = info & 0x1F;
    let mode = LogoMode::from_code(code).ok_or(LogoError::BadFormat(code))?;
    if info & 0x3E0 != 0 {
        return Err(LogoError::ReservedBits(info));
    }
    let (width, height) = info_sides(info);
    let limit = mode.limit();
    if width == 0 || height == 0 || width > limit || height > limit {
        return Err(LogoError::BadSide { width, height });
    }
    if width % 2 == 1 {
        return Err(LogoError::OddWidth(width));
    }
    // Sides are at most 240, so this cannot overflow.
    let expected = width * height * mode.bytes_per_pixel() as u32;
    if byte_count != expected {
        return Err(LogoError::ByteCount {
            expected,
            got: byte_count,
        });
    }
    if HEADER_LEN + expected as usize > MAX_BLOB {
        return Err(LogoError::TooLarge(HEADER_LEN + expected as usize));
    }
    Ok(LogoHeader {
        mode,
        width,
        height,
        byte_count,
    })
}

/// Checks a whole (unpadded or padded) blob: header, duplicated info word,
/// zero field, length, and the 120 016-byte ceiling.
pub fn validate_blob(blob: &[u8]) -> Result<LogoHeader, LogoError> {
    if blob.len() > padded_len(MAX_BLOB) {
        return Err(LogoError::TooLarge(blob.len()));
    }
    let word = |i: usize| -> Option<u32> {
        let b = blob.get(i * 4..i * 4 + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let (Some(info), Some(count), Some(zero), Some(info2)) = (word(0), word(1), word(2), word(3))
    else {
        return Err(LogoError::Unrecognised);
    };
    if zero != 0 || info2 != info {
        return Err(LogoError::Unrecognised);
    }
    let header = validate_header(info, count)?;
    let total = HEADER_LEN + count as usize;
    let tail_ok = blob
        .get(total..)
        .is_some_and(|tail| blob.len() <= padded_len(total) && tail.iter().all(|&b| b == 0xFF));
    if !tail_ok {
        let got = u32::try_from(blob.len().saturating_sub(HEADER_LEN)).unwrap_or(u32::MAX);
        return Err(LogoError::ByteCount {
            expected: count,
            got,
        });
    }
    Ok(header)
}

/// Wraps already-encoded pixel bytes in the 16-byte header, refusing any
/// invariant break. `encode` ends with this.
pub fn assemble(
    format: u32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) -> Result<LogoBlob, LogoError> {
    if format > 0x1F {
        return Err(LogoError::BadFormat(format));
    }
    if width > 0x7FF || height > 0x7FF {
        return Err(LogoError::BadSide { width, height });
    }
    let info = info_word(format, width, height);
    let count = u32::try_from(pixels.len()).map_err(|_| LogoError::TooLarge(pixels.len()))?;
    validate_header(info, count)?;
    let mut bytes = Vec::with_capacity(HEADER_LEN + pixels.len());
    for word in [info, count, 0, info] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&pixels);
    Ok(LogoBlob { info, bytes })
}

/// Scales and encodes `img` into a logo blob.
pub fn encode(img: &Rgba, mode: LogoMode) -> Result<LogoBlob, LogoError> {
    if img.w == 0 || img.h == 0 || img.px.len() != img.w as usize * img.h as usize * 4 {
        return Err(LogoError::BadImage);
    }
    let s = scale_to_box(img, mode);
    let mut pixels = Vec::with_capacity(s.px.len() / 4 * mode.bytes_per_pixel());
    let (rgba, _) = s.px.as_chunks::<4>();
    for p in rgba {
        let rgb565 =
            (u16::from(p[0] >> 3) << 11) | (u16::from(p[1] >> 2) << 5) | u16::from(p[2] >> 3);
        pixels.extend_from_slice(&rgb565.to_le_bytes());
        if mode == LogoMode::Alpha {
            pixels.push(p[3]);
        }
    }
    assemble(mode.format_code(), s.w, s.h, pixels)
}

/// Decodes a blob (16-byte header) or a vendor-style raw file (4-byte info
/// word then pixels) into RGBA.
pub fn decode(data: &[u8]) -> Result<Rgba, LogoError> {
    let (header, pixels) = if let Ok(h) = validate_blob(data) {
        (h, data.get(HEADER_LEN..).ok_or(LogoError::Unrecognised)?)
    } else {
        decode_raw_header(data).ok_or(LogoError::Unrecognised)?
    };
    let bpp = header.mode.bytes_per_pixel();
    let pixels = pixels
        .get(..header.byte_count as usize)
        .ok_or(LogoError::Unrecognised)?;
    let mut px = Vec::with_capacity(pixels.len() / bpp * 4);
    for p in pixels.chunks_exact(bpp) {
        let (lo, hi) = match p {
            [lo, hi, ..] => (*lo, *hi),
            _ => return Err(LogoError::Unrecognised),
        };
        let v = u16::from_le_bytes([lo, hi]);
        let r = ((v >> 11) as u8 & 0x1F) << 3;
        let g = ((v >> 5) as u8 & 0x3F) << 2;
        let b = (v as u8 & 0x1F) << 3;
        let a = p.get(2).copied().unwrap_or(255);
        px.extend_from_slice(&[r, g, b, a]);
    }
    Ok(Rgba {
        w: header.width,
        h: header.height,
        px,
    })
}

/// Raw file layout: 4-byte info word then exactly `byte_count` pixel bytes
/// (optionally 0xFF-padded to a [`CHUNK`] multiple).
fn decode_raw_header(data: &[u8]) -> Option<(LogoHeader, &[u8])> {
    let info = u32::from_le_bytes(data.get(..4)?.try_into().ok()?);
    let rest = data.get(4..)?;
    let mode = LogoMode::from_code(info & 0x1F)?;
    let (width, height) = info_sides(info);
    let expected = width
        .checked_mul(height)?
        .checked_mul(mode.bytes_per_pixel() as u32)?;
    let header = validate_header(info, expected).ok()?;
    let n = expected as usize;
    let pixels = rest.get(..n)?;
    let tail = rest.get(n..)?;
    let padded_ok = data.len() <= padded_len(4 + n) && tail.iter().all(|&b| b == 0xFF);
    if !padded_ok {
        return None;
    }
    Some((header, pixels))
}

/// Loads a PNG, JPEG, BMP or raw `.bin` logo file into RGBA8888.
#[cfg(feature = "image-files")]
pub fn load_file(path: &std::path::Path) -> Result<Rgba, LogoError> {
    let bytes = std::fs::read(path)
        .map_err(|e| LogoError::Load(format!("cannot read {}: {e}", path.display())))?;
    let is_bin = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("bin"));
    if is_bin {
        decode(&bytes)
    } else {
        load_image_bytes(&bytes)
    }
}

/// Loads PNG, JPEG or BMP bytes (format sniffed from content) into RGBA8888.
#[cfg(feature = "image-files")]
pub fn load_image_bytes(bytes: &[u8]) -> Result<Rgba, LogoError> {
    let img = image::load_from_memory(bytes).map_err(|e| LogoError::Load(e.to_string()))?;
    // Palette, grayscale and 16-bit inputs all become RGBA8888 here.
    let rgba = img.into_rgba8();
    Ok(Rgba {
        w: rgba.width(),
        h: rgba.height(),
        px: rgba.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Rgba {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            px.extend_from_slice(&rgba);
        }
        Rgba { w, h, px }
    }

    fn u32_at(b: &[u8], off: usize) -> u32 {
        u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
    }

    fn pixel(img: &Rgba, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.w + x) * 4) as usize;
        [img.px[i], img.px[i + 1], img.px[i + 2], img.px[i + 3]]
    }

    /// RGB565-exact colours: R/B multiples of 8, G multiples of 4.
    fn exact_pattern(w: u32, h: u32, alpha: bool) -> Rgba {
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let r = ((x * 8) % 256) as u8 & 0xF8;
                let g = ((y * 4 + x) % 256) as u8 & 0xFC;
                let b = (((x + y) * 8) % 256) as u8 & 0xF8;
                let a = if alpha && (x + y) % 3 == 0 { 0 } else { 255 };
                px.extend_from_slice(&[r, g, b, a]);
            }
        }
        Rgba { w, h, px }
    }

    // ---- AC-PVFHM7.1: pixel formats and box limits ----

    #[test]
    fn ac1_mode_constants() {
        assert_eq!(LogoMode::Opaque.format_code(), 4);
        assert_eq!(LogoMode::Alpha.format_code(), 5);
        assert_eq!(LogoMode::Opaque.bytes_per_pixel(), 2);
        assert_eq!(LogoMode::Alpha.bytes_per_pixel(), 3);
        assert_eq!(LogoMode::Opaque.limit(), 240);
        assert_eq!(LogoMode::Alpha.limit(), 200);
    }

    #[test]
    fn ac1_opaque_encodes_rgb565_little_endian() {
        // Top-left red, next green, next blue, rest white.
        let mut img = solid(48, 48, [0xFF, 0xFF, 0xFF, 0xFF]);
        img.px[0..4].copy_from_slice(&[0xF8, 0x00, 0x00, 0xFF]);
        img.px[4..8].copy_from_slice(&[0x00, 0xFC, 0x00, 0xFF]);
        img.px[8..12].copy_from_slice(&[0x00, 0x00, 0xF8, 0xFF]);
        let blob = encode(&img, LogoMode::Opaque).expect("encode");
        assert_eq!(blob.bytes.len(), HEADER_LEN + 48 * 48 * 2);
        let p = &blob.bytes[HEADER_LEN..];
        assert_eq!(&p[0..2], &[0x00, 0xF8], "red = 0xF800 LE");
        assert_eq!(&p[2..4], &[0xE0, 0x07], "green = 0x07E0 LE");
        assert_eq!(&p[4..6], &[0x1F, 0x00], "blue = 0x001F LE");
        assert_eq!(&p[6..8], &[0xFF, 0xFF], "white = 0xFFFF");
    }

    #[test]
    fn ac1_alpha_encodes_rgb565_then_alpha_byte() {
        let mut img = solid(48, 48, [0x00, 0x00, 0x00, 0x00]);
        img.px[0..4].copy_from_slice(&[0xF8, 0x00, 0x00, 0xFF]);
        img.px[4..8].copy_from_slice(&[0x00, 0x00, 0xF8, 0xFF]);
        let blob = encode(&img, LogoMode::Alpha).expect("encode");
        assert_eq!(blob.bytes.len(), HEADER_LEN + 48 * 48 * 3);
        let p = &blob.bytes[HEADER_LEN..];
        assert_eq!(&p[0..3], &[0x00, 0xF8, 0xFF]);
        assert_eq!(&p[3..6], &[0x1F, 0x00, 0xFF]);
        assert_eq!(p[8], 0x00, "transparent pixel keeps alpha 0");
    }

    #[test]
    fn ac1_box_limit_depends_on_mode() {
        let img = solid(300, 300, [0x10, 0x20, 0x30, 0xFF]);
        let o = encode(&img, LogoMode::Opaque).expect("opaque");
        let a = encode(&img, LogoMode::Alpha).expect("alpha");
        assert_eq!(o.info, info_word(4, 240, 240));
        assert_eq!(o.bytes.len(), HEADER_LEN + 240 * 240 * 2);
        assert_eq!(a.info, info_word(5, 200, 200));
        assert_eq!(a.bytes.len(), HEADER_LEN + 200 * 200 * 3);
    }

    // ---- AC-PVFHM7.2: scaling rule ----

    #[test]
    fn ac2_target_size_worked_examples() {
        assert_eq!(target_size(1000, 100, LogoMode::Opaque), (240, 24));
        assert_eq!(target_size(10, 10, LogoMode::Opaque), (48, 48));
        assert_eq!(target_size(240, 135, LogoMode::Opaque), (240, 135));
    }

    #[test]
    fn ac2_target_size_alpha_box() {
        assert_eq!(target_size(1000, 1000, LogoMode::Alpha), (200, 200));
        assert_eq!(target_size(300, 100, LogoMode::Alpha), (200, 66));
        assert_eq!(target_size(10, 10, LogoMode::Alpha), (48, 48));
        // 240×135 does not fit the alpha box: 200 × 135·200/240 = 112.
        assert_eq!(target_size(240, 135, LogoMode::Alpha), (200, 112));
    }

    #[test]
    fn ac2_odd_width_rescaled_to_lower_even_keeping_aspect() {
        assert_eq!(target_size(49, 49, LogoMode::Opaque), (48, 48));
        assert_eq!(target_size(75, 50, LogoMode::Opaque), (74, 49));
        assert_eq!(target_size(101, 60, LogoMode::Opaque), (100, 59));
    }

    #[test]
    fn ac2_scaled_image_has_target_size() {
        let s = scale_to_box(&solid(1000, 100, [8, 4, 8, 255]), LogoMode::Opaque);
        assert_eq!((s.w, s.h), (240, 24));
        assert_eq!(s.px.len(), 240 * 24 * 4);
        let up = scale_to_box(&solid(10, 10, [8, 4, 8, 255]), LogoMode::Opaque);
        assert_eq!((up.w, up.h), (48, 48));
        assert_eq!(
            pixel(&up, 23, 23),
            [8, 4, 8, 255],
            "solid colour survives scaling"
        );
    }

    #[test]
    fn ac2_in_box_image_is_unchanged() {
        let img = exact_pattern(240, 135, false);
        let s = scale_to_box(&img, LogoMode::Opaque);
        assert_eq!(s, img);
    }

    #[test]
    fn ac2_downscale_is_smooth_not_nearest() {
        // 1-px black/white checkerboard halved: a smooth filter averages to grey.
        let (w, h) = (480u32, 270u32);
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = if (x + y) % 2 == 0 { 0 } else { 255 };
                px.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let s = scale_to_box(&Rgba { w, h, px }, LogoMode::Opaque);
        assert_eq!((s.w, s.h), (240, 135));
        let p = pixel(&s, 120, 67);
        assert!((64..=192).contains(&p[0]), "expected grey, got {p:?}");
    }

    #[test]
    fn ac2_upscale_is_smooth_not_nearest() {
        // Left half black, right half white, 10×10 → 48×48: some pixel blends.
        let mut px = Vec::new();
        for _y in 0..10 {
            for x in 0..10 {
                let v = if x < 5 { 0 } else { 255 };
                px.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let s = scale_to_box(&Rgba { w: 10, h: 10, px }, LogoMode::Opaque);
        assert_eq!((s.w, s.h), (48, 48));
        let row: Vec<u8> = (0..48).map(|x| pixel(&s, x, 24)[0]).collect();
        assert!(
            row.iter().any(|&v| v > 16 && v < 239),
            "no blended pixel in {row:?}"
        );
        assert_eq!(row[0], 0);
        assert_eq!(row[47], 255);
    }

    // ---- AC-PVFHM7.3: blob layout and info word ----

    #[test]
    fn ac3_info_word_worked_examples() {
        assert_eq!(info_word(4, 240, 135), 0x10E3_C004);
        assert_eq!(info_word(5, 200, 200), 0x1903_2005);
        assert_eq!(info_word(4, 48, 48), 0x0600_C004);
    }

    fn check_header(blob: &LogoBlob, info: u32, len: usize) {
        assert_eq!(blob.info, info);
        assert_eq!(blob.bytes.len(), len);
        let b = &blob.bytes;
        assert_eq!(u32_at(b, 0), info, "info word at 0");
        assert_eq!(u32_at(b, 4) as usize, len - HEADER_LEN, "pixel byte count");
        assert_eq!(u32_at(b, 8), 0, "zero field");
        assert_eq!(u32_at(b, 12), info, "info word repeated at 12");
    }

    #[test]
    fn ac3_blob_headers_and_sizes() {
        let o = encode(&solid(240, 135, [0, 0, 0, 255]), LogoMode::Opaque).expect("opaque");
        check_header(&o, 0x10E3_C004, 64_816);
        let a = encode(&solid(200, 200, [0, 0, 0, 255]), LogoMode::Alpha).expect("alpha");
        check_header(&a, 0x1903_2005, 120_016);
        let s = encode(&solid(48, 48, [0, 0, 0, 255]), LogoMode::Opaque).expect("small");
        check_header(&s, 0x0600_C004, 4_624);
        assert_eq!(
            &s.bytes[0..4],
            &[0x04, 0xC0, 0x00, 0x06],
            "little-endian info word"
        );
    }

    #[test]
    fn ac3_rows_top_first_no_padding() {
        // Width 50: 100-byte rows (not a multiple of 4 × anything special).
        let (w, h) = (50u32, 48u32);
        let mut img = solid(w, h, [0x00, 0x00, 0xF8, 0xFF]); // blue
        // Top row red, pixel (1, 1) green.
        for x in 0..w as usize {
            img.px[x * 4..x * 4 + 4].copy_from_slice(&[0xF8, 0, 0, 0xFF]);
        }
        let g = ((w + 1) * 4) as usize;
        img.px[g..g + 4].copy_from_slice(&[0, 0xFC, 0, 0xFF]);
        let blob = encode(&img, LogoMode::Opaque).expect("encode");
        assert_eq!(blob.bytes.len(), HEADER_LEN + (w * h * 2) as usize);
        let p = &blob.bytes[HEADER_LEN..];
        let row = (w * 2) as usize;
        assert_eq!(&p[0..2], &[0x00, 0xF8]);
        assert_eq!(&p[row - 2..row], &[0x00, 0xF8], "end of top row is red");
        assert_eq!(
            &p[row..row + 2],
            &[0x1F, 0x00],
            "row 1 starts right after row 0"
        );
        assert_eq!(&p[row + 2..row + 4], &[0xE0, 0x07], "pixel (1,1) green");
        assert_eq!(&p[p.len() - 2..], &[0x1F, 0x00], "bottom-right blue");
    }

    // ---- AC-PVFHM7.4: padding ----

    #[test]
    fn ac4_padded_to_256_with_ff() {
        let blob = encode(&solid(48, 48, [0, 0, 0, 255]), LogoMode::Opaque).expect("encode");
        let padded = blob.padded();
        assert_eq!(padded.len(), 4_864);
        assert_eq!(padded.len() / CHUNK, 19);
        assert_eq!(&padded[..4_624], &blob.bytes[..]);
        assert!(padded[4_624..].iter().all(|&b| b == 0xFF));
        assert_eq!(u32_at(&padded, 4), 4_608, "header byte count stays exact");
    }

    #[test]
    fn ac4_padding_other_sizes() {
        let o = encode(&solid(240, 135, [0, 0, 0, 255]), LogoMode::Opaque).expect("o");
        assert_eq!(o.padded().len(), 65_024);
        let a = encode(&solid(200, 200, [0, 0, 0, 255]), LogoMode::Alpha).expect("a");
        assert_eq!(a.padded().len(), 120_064);
        for b in [&o, &a] {
            assert_eq!(b.padded().len() % CHUNK, 0);
            assert!(b.padded().len() - b.bytes.len() < CHUNK);
        }
    }

    // ---- AC-PVFHM7.5: refusals ----

    #[test]
    fn ac5_validate_header_accepts_worked_examples() {
        let h = validate_header(0x10E3_C004, 64_800).expect("opaque");
        assert_eq!(
            h,
            LogoHeader {
                mode: LogoMode::Opaque,
                width: 240,
                height: 135,
                byte_count: 64_800
            }
        );
        let a = validate_header(0x1903_2005, 120_000).expect("alpha");
        assert_eq!((a.mode, a.width, a.height), (LogoMode::Alpha, 200, 200));
    }

    #[test]
    fn ac5_validate_header_refusal_table() {
        let cases: &[(u32, u32, LogoError)] = &[
            (info_word(3, 48, 48), 4_608, LogoError::BadFormat(3)),
            (info_word(6, 48, 48), 4_608, LogoError::BadFormat(6)),
            (
                info_word(4, 0, 48),
                0,
                LogoError::BadSide {
                    width: 0,
                    height: 48,
                },
            ),
            (
                info_word(4, 48, 0),
                0,
                LogoError::BadSide {
                    width: 48,
                    height: 0,
                },
            ),
            (
                info_word(4, 242, 48),
                242 * 48 * 2,
                LogoError::BadSide {
                    width: 242,
                    height: 48,
                },
            ),
            (
                info_word(4, 48, 241),
                48 * 241 * 2,
                LogoError::BadSide {
                    width: 48,
                    height: 241,
                },
            ),
            (
                info_word(5, 202, 48),
                202 * 48 * 3,
                LogoError::BadSide {
                    width: 202,
                    height: 48,
                },
            ),
            (
                info_word(5, 48, 201),
                48 * 201 * 3,
                LogoError::BadSide {
                    width: 48,
                    height: 201,
                },
            ),
            (info_word(4, 49, 48), 49 * 48 * 2, LogoError::OddWidth(49)),
            (
                info_word(4, 48, 48),
                4_607,
                LogoError::ByteCount {
                    expected: 4_608,
                    got: 4_607,
                },
            ),
            (
                info_word(5, 48, 48),
                4_608,
                LogoError::ByteCount {
                    expected: 6_912,
                    got: 4_608,
                },
            ),
            (
                0x0600_C004 | (1 << 5),
                4_608,
                LogoError::ReservedBits(0x0600_C004 | (1 << 5)),
            ),
            (
                0x0600_C004 | (1 << 9),
                4_608,
                LogoError::ReservedBits(0x0600_C004 | (1 << 9)),
            ),
        ];
        for (info, count, want) in cases {
            assert_eq!(
                validate_header(*info, *count).as_ref().err(),
                Some(want),
                "info {info:#010x} count {count}"
            );
        }
    }

    #[test]
    fn ac5_assemble_refuses_and_returns_no_blob() {
        assert_eq!(
            assemble(3, 48, 48, vec![0; 4_608]),
            Err(LogoError::BadFormat(3))
        );
        assert_eq!(
            assemble(4, 0, 48, vec![]),
            Err(LogoError::BadSide {
                width: 0,
                height: 48
            })
        );
        assert_eq!(
            assemble(4, 242, 2, vec![0; 242 * 2 * 2]),
            Err(LogoError::BadSide {
                width: 242,
                height: 2
            })
        );
        assert_eq!(
            assemble(4, 49, 48, vec![0; 49 * 48 * 2]),
            Err(LogoError::OddWidth(49))
        );
        assert_eq!(
            assemble(4, 48, 48, vec![0; 4_000]),
            Err(LogoError::ByteCount {
                expected: 4_608,
                got: 4_000
            })
        );
        let ok = assemble(4, 48, 48, vec![0; 4_608]).expect("valid");
        assert_eq!(ok.info, 0x0600_C004);
        assert_eq!(ok.bytes.len(), 4_624);
    }

    #[test]
    fn ac5_validate_blob_checks_whole_blob() {
        let good = encode(&solid(48, 48, [0, 0, 0, 255]), LogoMode::Opaque).expect("e");
        assert!(validate_blob(&good.bytes).is_ok());
        assert!(
            validate_blob(&good.padded()).is_ok(),
            "0xFF padding is accepted"
        );
        // Mismatched second info word.
        let mut b = good.bytes.clone();
        b[12] ^= 0x01;
        assert!(validate_blob(&b).is_err());
        // Non-zero third word.
        let mut b = good.bytes.clone();
        b[8] = 1;
        assert!(validate_blob(&b).is_err());
        // Truncated pixels.
        assert!(validate_blob(&good.bytes[..good.bytes.len() - 1]).is_err());
        // Reserved bits set in both copies.
        let mut b = good.bytes.clone();
        b[0] |= 1 << 5;
        b[12] |= 1 << 5;
        assert!(matches!(validate_blob(&b), Err(LogoError::ReservedBits(_))));
    }

    #[test]
    fn ac5_validate_blob_refuses_over_ceiling() {
        // Anything whose unpadded size would pass 120 016 bytes is refused.
        let big = vec![0u8; MAX_BLOB + 1 + CHUNK];
        assert!(validate_blob(&big).is_err());
        // A header claiming alpha 202×200 (above limit, 121 216 bytes).
        let mut b = Vec::new();
        let info = info_word(5, 202, 200);
        for w in [info, 202 * 200 * 3, 0, info] {
            b.extend_from_slice(&w.to_le_bytes());
        }
        b.resize(HEADER_LEN + 202 * 200 * 3, 0);
        assert!(validate_blob(&b).is_err());
    }

    #[test]
    fn ac5_encode_refuses_bad_rgba() {
        let bad = Rgba {
            w: 48,
            h: 48,
            px: vec![0; 10],
        };
        assert_eq!(encode(&bad, LogoMode::Opaque), Err(LogoError::BadImage));
        let empty = Rgba {
            w: 0,
            h: 0,
            px: vec![],
        };
        assert!(encode(&empty, LogoMode::Opaque).is_err());
    }

    // ---- AC-PVFHM7.6: decoder ----

    #[test]
    fn ac6_round_trip_opaque_exact() {
        let img = exact_pattern(240, 135, false);
        let blob = encode(&img, LogoMode::Opaque).expect("encode");
        assert_eq!(decode(&blob.bytes).expect("decode"), img);
        assert_eq!(decode(&blob.padded()).expect("decode padded"), img);
    }

    #[test]
    fn ac6_round_trip_alpha_keeps_0_and_255() {
        let img = exact_pattern(64, 48, true);
        let blob = encode(&img, LogoMode::Alpha).expect("encode");
        let back = decode(&blob.bytes).expect("decode");
        assert_eq!((back.w, back.h), (64, 48));
        for y in 0..48 {
            for x in 0..64 {
                let (a, b) = (pixel(&img, x, y), pixel(&back, x, y));
                assert_eq!(a[3], b[3], "alpha at ({x},{y})");
                if a[3] == 255 {
                    assert_eq!(a, b, "opaque pixel at ({x},{y})");
                }
            }
        }
    }

    #[test]
    fn ac6_decodes_raw_file_layout() {
        let img = exact_pattern(48, 50, false);
        let blob = encode(&img, LogoMode::Opaque).expect("encode");
        // Raw file = info word then pixels (the blob from offset 12).
        let raw = &blob.bytes[12..];
        assert_eq!(decode(raw).expect("raw"), img);
        let imga = exact_pattern(48, 48, true);
        let blob_a = encode(&imga, LogoMode::Alpha).expect("encode");
        let back = decode(&blob_a.bytes[12..]).expect("raw alpha");
        assert_eq!((back.w, back.h), (48, 48));
        assert_eq!(pixel(&back, 0, 0)[3], 0);
        assert_eq!(pixel(&back, 1, 0), pixel(&imga, 1, 0));
    }

    #[test]
    fn ac6_unrecognised_inputs() {
        let blob = encode(&solid(48, 48, [8, 8, 8, 255]), LogoMode::Opaque).expect("e");
        let mut reserved = blob.bytes.clone();
        reserved[0] |= 1 << 5;
        reserved[12] |= 1 << 5;
        let mut fmt = blob.bytes.clone();
        fmt[0] = (fmt[0] & !0x1F) | 7;
        fmt[12] = fmt[0];
        let bad: Vec<Vec<u8>> = vec![
            vec![],
            vec![0x04],
            vec![0x04, 0xC0, 0x00],
            vec![0; 16],
            vec![0xFF; 64],
            blob.bytes[..100].to_vec(),
            blob.bytes[12..200].to_vec(),
            reserved,
            fmt,
        ];
        for b in bad {
            assert_eq!(
                decode(&b),
                Err(LogoError::Unrecognised),
                "input len {}",
                b.len()
            );
        }
    }

    #[test]
    fn ac6_decode_never_panics_on_arbitrary_bytes() {
        use proptest::prelude::*;
        let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
            failure_persistence: None,
            ..Default::default()
        });
        runner
            .run(&proptest::collection::vec(any::<u8>(), 0..512), |bytes| {
                let _ = decode(&bytes);
                Ok(())
            })
            .expect("decode must not panic");
        // Headers with huge claimed sizes must not allocate/panic either.
        for info in [u32::MAX, 0xFFFF_FC04, 0xFFFF_FC05, info_word(5, 2046, 2047)] {
            let mut b = Vec::new();
            for w in [info, u32::MAX, 0, info] {
                b.extend_from_slice(&w.to_le_bytes());
            }
            assert_eq!(decode(&b), Err(LogoError::Unrecognised));
        }
    }

    // ---- AC-PVFHM7.7: preview ----

    #[test]
    fn ac7_preview_small_logo() {
        let blob = encode(&solid(48, 48, [0, 0, 0, 255]), LogoMode::Opaque).expect("e");
        assert_eq!(
            blob.preview(),
            Preview {
                width: 48,
                height: 48,
                mode: LogoMode::Opaque,
                blob_size: 4_624,
                padded_size: 4_864,
                chunks: 19,
                small_side_warning: false,
            }
        );
    }

    #[test]
    fn ac7_preview_warns_on_side_below_48() {
        let blob = encode(&solid(1000, 100, [0, 0, 0, 255]), LogoMode::Opaque).expect("e");
        let p = blob.preview();
        assert_eq!((p.width, p.height, p.mode), (240, 24, LogoMode::Opaque));
        assert_eq!(p.blob_size, 16 + 240 * 24 * 2);
        assert_eq!(p.padded_size, 11_520 + 256);
        assert_eq!(p.chunks, p.padded_size / 256);
        assert!(p.small_side_warning);
    }

    #[test]
    fn ac7_preview_alpha() {
        let blob = encode(&solid(200, 200, [0, 0, 0, 0]), LogoMode::Alpha).expect("e");
        let p = blob.preview();
        assert_eq!((p.width, p.height, p.mode), (200, 200, LogoMode::Alpha));
        assert_eq!(
            (p.blob_size, p.padded_size, p.chunks),
            (120_016, 120_064, 469)
        );
        assert!(!p.small_side_warning);
    }
}
