//! Saving a chart image (FR AXHK2V): PNG and SVG bytes are written as the UI
//! rendered them, a PDF wraps the PNG in one page with a small hand-written
//! writer. A PNG (or the PNG inside a PDF) must be 320 to 7680 px on each
//! side (AC-AXHK2V.5).

use std::fmt;
use std::io::{Read, Write};
use std::path::Path;

/// The smallest and largest width or height, in px (AC-AXHK2V.5).
pub const MIN_PX: u32 = 320;
pub const MAX_PX: u32 = 7680;

/// The formats "Save image" writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartImageFormat {
    Png,
    Svg,
    Pdf,
}

impl ChartImageFormat {
    /// `"png"`, `"svg"` or `"pdf"`; anything else is `None`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "png" => Some(Self::Png),
            "svg" => Some(Self::Svg),
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }
}

/// Why a chart image was not written.
#[derive(Debug)]
pub enum ChartImageError {
    /// The bytes are not a PNG this writer takes.
    NotPng(&'static str),
    /// A side outside 320 to 7680 px.
    Size { width: u32, height: u32 },
    /// Writing the file failed.
    Io(std::io::Error),
}

impl fmt::Display for ChartImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPng(why) => write!(f, "the image is not a PNG this app can save ({why})"),
            Self::Size { width, height } => write!(
                f,
                "the image is {width} × {height} px; width and height must be from {MIN_PX} to {MAX_PX} px"
            ),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ChartImageError {}

impl From<std::io::Error> for ChartImageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// The parts of a PNG the PDF writer needs.
struct Png<'a> {
    width: u32,
    height: u32,
    /// 3 (RGB) or 4 (RGBA), 8 bits each.
    channels: usize,
    idat: Vec<&'a [u8]>,
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Walks the chunks of `bytes`, guarding every length; refuses anything but
/// an 8-bit, non-interlaced RGB or RGBA PNG with an image size in range.
fn parse_png(bytes: &[u8]) -> Result<Png<'_>, ChartImageError> {
    let rest = bytes
        .strip_prefix(&SIGNATURE)
        .ok_or(ChartImageError::NotPng("no PNG signature"))?;
    let mut at = 0;
    let mut header: Option<(u32, u32, usize)> = None;
    let mut idat = Vec::new();
    loop {
        let head = rest
            .get(at..at + 8)
            .ok_or(ChartImageError::NotPng("truncated"))?;
        let len = usize::try_from(be32(&head[..4]))
            .map_err(|_| ChartImageError::NotPng("a chunk too long"))?;
        let kind = &head[4..8];
        let data = rest
            .get(at + 8..at + 8 + len)
            .ok_or(ChartImageError::NotPng("truncated"))?;
        // The chunk's CRC must be there too.
        rest.get(at + 8 + len..at + 12 + len)
            .ok_or(ChartImageError::NotPng("truncated"))?;
        at += 12 + len;
        match kind {
            b"IHDR" => {
                if data.len() != 13 {
                    return Err(ChartImageError::NotPng("a bad header"));
                }
                let (width, height) = (be32(&data[0..4]), be32(&data[4..8]));
                let channels = match (data[8], data[9]) {
                    (8, 2) => 3,
                    (8, 6) => 4,
                    _ => return Err(ChartImageError::NotPng("not 8-bit RGB or RGBA")),
                };
                if data[10] != 0 || data[11] != 0 || data[12] != 0 {
                    return Err(ChartImageError::NotPng("an interlaced or unknown encoding"));
                }
                check_size(width, height)?;
                header = Some((width, height, channels));
            }
            b"IDAT" => idat.push(data),
            b"IEND" => break,
            _ => {}
        }
    }
    let (width, height, channels) = header.ok_or(ChartImageError::NotPng("no header"))?;
    if idat.is_empty() {
        return Err(ChartImageError::NotPng("no image data"));
    }
    Ok(Png {
        width,
        height,
        channels,
        idat,
    })
}

fn check_size(width: u32, height: u32) -> Result<(), ChartImageError> {
    let ok = |px: u32| (MIN_PX..=MAX_PX).contains(&px);
    if ok(width) && ok(height) {
        Ok(())
    } else {
        Err(ChartImageError::Size { width, height })
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// The PNG's pixels, unfiltered: `height` rows of `width × channels` bytes.
fn pixels(png: &Png<'_>) -> Result<Vec<u8>, ChartImageError> {
    let width = png.width as usize;
    let height = png.height as usize;
    let bpp = png.channels;
    let stride = width * bpp;
    let expected = height * (stride + 1);
    let mut zlib = Vec::with_capacity(png.idat.iter().map(|d| d.len()).sum());
    for d in &png.idat {
        zlib.extend_from_slice(d);
    }
    let mut raw = Vec::with_capacity(expected);
    flate2::read::ZlibDecoder::new(zlib.as_slice())
        .take(expected as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| ChartImageError::NotPng("broken image data"))?;
    if raw.len() != expected {
        return Err(ChartImageError::NotPng("the image data is the wrong size"));
    }
    let mut out = vec![0_u8; height * stride];
    for y in 0..height {
        let line = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let (filter, src) = (line[0], &line[1..]);
        let (done, row) = out.split_at_mut(y * stride);
        let row = &mut row[..stride];
        let prev = if y == 0 {
            None
        } else {
            Some(&done[(y - 1) * stride..])
        };
        for i in 0..stride {
            let a = if i >= bpp { row[i - bpp] } else { 0 };
            let b = prev.map_or(0, |p| p[i]);
            let c = if i >= bpp {
                prev.map_or(0, |p| p[i - bpp])
            } else {
                0
            };
            let predicted = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                4 => paeth(a, b, c),
                _ => return Err(ChartImageError::NotPng("an unknown row filter")),
            };
            row[i] = src[i].wrapping_add(predicted);
        }
    }
    Ok(out)
}

fn deflate(bytes: &[u8]) -> Result<Vec<u8>, ChartImageError> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(bytes)?;
    Ok(z.finish()?)
}

/// A one-page PDF holding `png` as an image XObject of its pixel size, the
/// page 1 pt per pixel. RGBA keeps its alpha as a soft mask.
pub fn pdf_from_png(png: &[u8]) -> Result<Vec<u8>, ChartImageError> {
    let parsed = parse_png(png)?;
    let (w, h) = (parsed.width, parsed.height);
    let px = pixels(&parsed)?;
    let (rgb, alpha) = if parsed.channels == 4 {
        let mut rgb = Vec::with_capacity(px.len() / 4 * 3);
        let mut alpha = Vec::with_capacity(px.len() / 4);
        for p in px.as_chunks::<4>().0 {
            rgb.extend_from_slice(&p[..3]);
            alpha.push(p[3]);
        }
        // An opaque image needs no mask.
        let mask = alpha.iter().any(|&a| a != 255).then_some(alpha);
        (rgb, mask)
    } else {
        (px, None)
    };

    let mut objects: Vec<Vec<u8>> = Vec::new();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.push(
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] \
             /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>"
        )
        .into_bytes(),
    );
    let image = |data: &[u8], space: &str, extra: &str| -> Result<Vec<u8>, ChartImageError> {
        let z = deflate(data)?;
        let mut o = format!(
            "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace {space} \
             /BitsPerComponent 8 /Filter /FlateDecode{extra} /Length {} >>\nstream\n",
            z.len()
        )
        .into_bytes();
        o.extend_from_slice(&z);
        o.extend_from_slice(b"\nendstream");
        Ok(o)
    };
    let smask = if alpha.is_some() { " /SMask 6 0 R" } else { "" };
    objects.push(image(&rgb, "/DeviceRGB", smask)?);
    let content = format!("q {w} 0 0 {h} 0 0 cm /Im0 Do Q");
    objects.push(
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
    );
    if let Some(alpha) = alpha {
        objects.push(image(&alpha, "/DeviceGray", "")?);
    }

    let mut pdf = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (k, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", k + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let n = objects.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    Ok(pdf)
}

/// Writes the chart image to `path`: PNG and SVG bytes as given, a PDF as
/// [`pdf_from_png`] of `data`. A PNG outside the size range is refused and
/// nothing is written.
pub fn write_chart_image(
    path: &Path,
    format: ChartImageFormat,
    data: &[u8],
) -> Result<(), ChartImageError> {
    match format {
        ChartImageFormat::Png => {
            parse_png(data)?;
            std::fs::write(path, data)?;
        }
        ChartImageFormat::Svg => std::fs::write(path, data)?,
        ChartImageFormat::Pdf => std::fs::write(path, pdf_from_png(data)?)?,
    }
    Ok(())
}

/// Checks `data` before the save dialog opens: the format is known and a
/// PNG (or a PDF's PNG) is in range.
pub fn check_chart_image(format: ChartImageFormat, data: &[u8]) -> Result<(), ChartImageError> {
    match format {
        ChartImageFormat::Svg => Ok(()),
        ChartImageFormat::Png | ChartImageFormat::Pdf => parse_png(data).map(|_| ()),
    }
}
