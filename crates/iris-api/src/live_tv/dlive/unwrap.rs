//! MPEG-TS segments disguised as images (dlive's Player 1 uploads them to an
//! image CDN). A port of the player's own `unwrap()`, tried in its order:
//! WebP `EXIF` chunk, bytes after the PNG `IEND`, PNG pixels
//! (`TIKTIKPX` + u32 BE length + gzip), then raw `TIKTIKRAW` / `TIKTIKTSGZ`
//! markers and a bare TS sync run. Upstream switches between them.

use std::io::Read as _;

const TS_SYNC: u8 = 0x47;
const TS_PACKET: usize = 188;
const PIXELS_MAGIC: &[u8] = b"TIKTIKPX";
const RAW_MAGIC: &[u8] = b"TIKTIKRAW";
const GZIP_MAGIC: &[u8] = b"TIKTIKTSGZ";
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
/// Ceiling for every inflated buffer (a real segment is ~5 MB).
const MAX_INFLATED: usize = 64 * 1024 * 1024;

/// The TS hidden in `bytes`, or `None` when no known wrapper holds one.
pub fn unwrap_segment(bytes: &[u8]) -> Option<Vec<u8>> {
    if let Some(ts) = webp_exif(bytes) {
        return Some(ts.to_vec());
    }
    if bytes.starts_with(&PNG_SIGNATURE[..2]) {
        if let Some(ts) = png_after_iend(bytes) {
            return Some(ts.to_vec());
        }
        return png_pixels(bytes);
    }
    if let Some(at) = find(bytes, RAW_MAGIC) {
        let ts = &bytes[at + RAW_MAGIC.len()..];
        if ts.first() == Some(&TS_SYNC) {
            return Some(ts.to_vec());
        }
    }
    if let Some(at) = find(bytes, GZIP_MAGIC) {
        return gunzip(&bytes[at + GZIP_MAGIC.len()..]).filter(|ts| is_ts(ts));
    }
    (0..bytes.len().saturating_sub(TS_PACKET))
        .find(|&i| bytes[i] == TS_SYNC && bytes[i + TS_PACKET] == TS_SYNC)
        .map(|i| bytes[i..].to_vec())
}

fn is_ts(b: &[u8]) -> bool {
    b.first() == Some(&TS_SYNC)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn u32_be(b: &[u8], at: usize) -> Option<usize> {
    let raw: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
    usize::try_from(u32::from_be_bytes(raw)).ok()
}

fn webp_exif(b: &[u8]) -> Option<&[u8]> {
    if b.len() < 16 || &b[..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return None;
    }
    let mut off = 12;
    while off + 8 <= b.len() {
        let tag = &b[off..off + 4];
        let raw: [u8; 4] = b[off + 4..off + 8].try_into().ok()?;
        let n = usize::try_from(u32::from_le_bytes(raw)).ok()?;
        off += 8;
        let data = b.get(off..off.checked_add(n)?)?;
        if tag == b"EXIF" {
            let sync = data.len() > TS_PACKET && data[0] == TS_SYNC && data[TS_PACKET] == TS_SYNC;
            return sync.then_some(data);
        }
        off += n + (n & 1);
    }
    None
}

/// `(type, data)` of a PNG chunk.
type Chunk<'a> = (&'a [u8], &'a [u8]);

/// Every chunk, plus the offset just past `IEND`.
fn png_chunks(b: &[u8]) -> Option<(Vec<Chunk<'_>>, Option<usize>)> {
    let mut off = 8;
    let mut chunks = Vec::new();
    while off + 8 <= b.len() {
        let len = u32_be(b, off)?;
        let end = off.checked_add(12)?.checked_add(len)?;
        if end > b.len() {
            return None;
        }
        let kind = &b[off + 4..off + 8];
        chunks.push((kind, &b[off + 8..off + 8 + len]));
        off = end;
        if kind == b"IEND" {
            return Some((chunks, Some(off)));
        }
    }
    Some((chunks, None))
}

fn png_after_iend(b: &[u8]) -> Option<&[u8]> {
    let (_, Some(end)) = png_chunks(b)? else {
        return None;
    };
    let tail = &b[end..];
    (tail.len() > TS_PACKET && tail[0] == TS_SYNC && tail[TS_PACKET] == TS_SYNC).then_some(tail)
}

/// 8-bit RGB / RGBA, non-interlaced: inflate the IDATs, reverse the row
/// filters, keep RGB, read `TIKTIKPX` + length + gzip(TS).
fn png_pixels(b: &[u8]) -> Option<Vec<u8>> {
    let (chunks, _) = png_chunks(b)?;
    let ihdr = chunks.iter().find(|(k, _)| *k == b"IHDR")?.1;
    let width = u32_be(ihdr, 0)?;
    let height = u32_be(ihdr, 4)?;
    let (depth, colour, interlace) = (*ihdr.get(8)?, *ihdr.get(9)?, *ihdr.get(12)?);
    let bpp = match colour {
        2 => 3,
        6 => 4,
        _ => return None,
    };
    if width == 0 || height == 0 || depth != 8 || interlace != 0 {
        return None;
    }
    let stride = width.checked_mul(bpp)?;
    let raw_len = stride.checked_add(1)?.checked_mul(height)?;
    if raw_len > MAX_INFLATED {
        return None;
    }
    let idat: Vec<u8> = chunks
        .iter()
        .filter(|(k, _)| *k == b"IDAT")
        .flat_map(|(_, d)| d.iter().copied())
        .collect();
    let raw = inflate_capped(flate2::read::ZlibDecoder::new(&idat[..]), raw_len)?;
    if raw.len() < raw_len {
        return None;
    }
    let rgb = unfilter(&raw, stride, height, bpp)?;
    if !rgb.starts_with(PIXELS_MAGIC) {
        return None;
    }
    let n = u32_be(&rgb, 8)?;
    let gz = rgb.get(12..12usize.checked_add(n)?)?;
    if n == 0 || !gz.starts_with(&[0x1f, 0x8b]) {
        return None;
    }
    gunzip(gz).filter(|ts| is_ts(ts))
}

/// Reverse PNG filters 0–4 and drop alpha: the RGB bytes, row after row.
fn unfilter(raw: &[u8], stride: usize, height: usize, bpp: usize) -> Option<Vec<u8>> {
    let mut rgb = Vec::with_capacity(stride / bpp * 3 * height);
    let mut prev = vec![0u8; stride];
    let mut row = vec![0u8; stride];
    for y in 0..height {
        let start = y * (stride + 1);
        let filter = raw[start];
        let line = &raw[start + 1..start + 1 + stride];
        for i in 0..stride {
            let a = if i >= bpp { row[i - bpp] } else { 0 };
            let up = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let add = match filter {
                0 => 0,
                1 => a,
                2 => up,
                3 => a.midpoint(up),
                4 => paeth(a, up, c),
                _ => return None,
            };
            row[i] = line[i].wrapping_add(add);
        }
        if bpp == 3 {
            rgb.extend_from_slice(&row);
        } else {
            for px in row.as_chunks::<4>().0 {
                rgb.extend_from_slice(&px[..3]);
            }
        }
        std::mem::swap(&mut prev, &mut row);
    }
    Some(rgb)
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

fn gunzip(b: &[u8]) -> Option<Vec<u8>> {
    inflate_capped(flate2::read::GzDecoder::new(b), MAX_INFLATED)
}

/// Read a decoder to its end, refusing more than `cap` bytes.
fn inflate_capped(reader: impl std::io::Read, cap: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    reader
        .take(u64::try_from(cap).ok()? + 1)
        .read_to_end(&mut out)
        .ok()?;
    (out.len() <= cap).then_some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write as _;

    use super::*;

    /// Two TS packets with a recognisable payload.
    pub(crate) fn ts() -> Vec<u8> {
        let mut ts = Vec::new();
        for n in 0..2u8 {
            let mut packet = vec![TS_SYNC, 0x40, 0x11, 0x10];
            packet.resize(TS_PACKET, n.wrapping_add(0xa0));
            ts.extend(packet);
        }
        ts
    }

    fn gzip(b: &[u8]) -> Vec<u8> {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b).unwrap();
        enc.finish().unwrap()
    }

    fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(data.len()).unwrap().to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out
    }

    /// The forward PNG filter, so the decoder is checked against every type.
    fn filter_row(kind: u8, row: &[u8], prev: &[u8], bpp: usize) -> Vec<u8> {
        (0..row.len())
            .map(|i| {
                let a = if i >= bpp { row[i - bpp] } else { 0 };
                let b = prev[i];
                let c = if i >= bpp { prev[i - bpp] } else { 0 };
                let predicted = match kind {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => a.midpoint(b),
                    _ => paeth(a, b, c),
                };
                row[i].wrapping_sub(predicted)
            })
            .collect()
    }

    /// A PNG whose RGB(A) pixels carry `payload`, each row with the next
    /// filter type (0..=4), and optional bytes after `IEND`.
    pub(crate) fn png(payload: &[u8], colour: u8, after_iend: &[u8]) -> Vec<u8> {
        let bpp = if colour == 6 { 4 } else { 3 };
        let width = 8usize;
        let rgb_per_row = width * 3;
        let rows = payload.len().div_ceil(rgb_per_row) + 1;
        let mut rgb = payload.to_vec();
        rgb.resize(rows * rgb_per_row, 0);
        let mut raw = Vec::new();
        let mut prev = vec![0u8; width * bpp];
        for (y, line) in rgb.chunks(rgb_per_row).enumerate() {
            let row: Vec<u8> = if bpp == 4 {
                line.chunks(3)
                    .flat_map(|p| [p[0], p[1], p[2], 0xff])
                    .collect()
            } else {
                line.to_vec()
            };
            let kind = u8::try_from(y % 5).unwrap();
            raw.push(kind);
            raw.extend(filter_row(kind, &row, &prev, bpp));
            prev = row;
        }
        let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        zlib.write_all(&raw).unwrap();
        let idat = zlib.finish().unwrap();
        let mut ihdr = u32::try_from(width).unwrap().to_be_bytes().to_vec();
        ihdr.extend(u32::try_from(rows).unwrap().to_be_bytes());
        ihdr.extend([8, colour, 0, 0, 0]);
        let mut out = PNG_SIGNATURE.to_vec();
        out.extend(chunk(b"IHDR", &ihdr));
        let (a, b) = idat.split_at(idat.len() / 2);
        out.extend(chunk(b"IDAT", a));
        out.extend(chunk(b"IDAT", b));
        out.extend(chunk(b"IEND", &[]));
        out.extend_from_slice(after_iend);
        out
    }

    /// The pixel layout dlive serves today: `TIKTIKPX` + length + gzip(TS).
    pub(crate) fn pixel_png(ts: &[u8]) -> Vec<u8> {
        let gz = gzip(ts);
        let mut payload = PIXELS_MAGIC.to_vec();
        payload.extend(u32::try_from(gz.len()).unwrap().to_be_bytes());
        payload.extend(gz);
        png(&payload, 2, &[])
    }

    #[test]
    fn png_pixels_rgb_and_rgba_every_filter() {
        let ts = ts();
        assert_eq!(unwrap_segment(&pixel_png(&ts)), Some(ts.clone()));
        let gz = gzip(&ts);
        let mut payload = PIXELS_MAGIC.to_vec();
        payload.extend(u32::try_from(gz.len()).unwrap().to_be_bytes());
        payload.extend(gz);
        assert_eq!(unwrap_segment(&png(&payload, 6, &[])), Some(ts));
    }

    #[test]
    fn png_after_iend() {
        let ts = ts();
        assert_eq!(unwrap_segment(&png(b"cover", 2, &ts)), Some(ts));
    }

    #[test]
    fn webp_exif_chunk() {
        let ts = ts();
        let mut body = b"WEBP".to_vec();
        body.extend(b"VP8 ");
        body.extend(3u32.to_le_bytes());
        body.extend([1, 2, 3, 0]);
        body.extend(b"EXIF");
        body.extend(u32::try_from(ts.len()).unwrap().to_le_bytes());
        body.extend(&ts);
        let mut webp = b"RIFF".to_vec();
        webp.extend(u32::try_from(body.len()).unwrap().to_le_bytes());
        webp.extend(body);
        assert_eq!(unwrap_segment(&webp), Some(ts));
    }

    #[test]
    fn raw_markers_and_bare_sync() {
        let ts = ts();
        let mut raw = b"junk TIKTIKRAW".to_vec();
        raw.extend(&ts);
        assert_eq!(unwrap_segment(&raw), Some(ts.clone()));
        let mut gz = b"GIF89a....TIKTIKTSGZ".to_vec();
        gz.extend(gzip(&ts));
        assert_eq!(unwrap_segment(&gz), Some(ts.clone()));
        let mut bare = b"\xff\xd8\xff garbage".to_vec();
        bare.extend(&ts);
        assert_eq!(unwrap_segment(&bare), Some(ts));
    }

    #[test]
    fn no_ts_payload_is_refused() {
        assert_eq!(unwrap_segment(b""), None);
        assert_eq!(unwrap_segment(b"<html>not found</html>"), None);
        // A real image: the pixels carry no magic.
        assert_eq!(unwrap_segment(&png(b"just a picture", 2, &[])), None);
        // The magic but a payload that isn't TS.
        let gz = gzip(b"definitely not transport stream");
        let mut payload = PIXELS_MAGIC.to_vec();
        payload.extend(u32::try_from(gz.len()).unwrap().to_be_bytes());
        payload.extend(gz);
        assert_eq!(unwrap_segment(&png(&payload, 2, &[])), None);
        // Truncated PNG.
        let full = pixel_png(&ts());
        assert_eq!(unwrap_segment(&full[..full.len() / 2]), None);
        // WebP without the TS sync in EXIF falls through to "nothing".
        let mut webp = b"RIFF\x10\x00\x00\x00WEBPEXIF\x04\x00\x00\x00abcd".to_vec();
        webp.extend([0u8; 4]);
        assert_eq!(unwrap_segment(&webp), None);
    }

    /// Check against a real captured segment:
    /// `DLIVE_SEGMENT=/path/seg.bin cargo test -p iris-api dlive_real_segment -- --ignored`.
    #[test]
    #[ignore = "needs a captured segment on disk"]
    fn dlive_real_segment() {
        let path = std::env::var("DLIVE_SEGMENT").expect("DLIVE_SEGMENT");
        let bytes = std::fs::read(path).unwrap();
        let ts = unwrap_segment(&bytes).expect("unwrapped");
        assert!(ts.len() > 1_000_000 && ts[0] == TS_SYNC && ts[TS_PACKET] == TS_SYNC);
        if let Ok(expected) = std::env::var("DLIVE_SEGMENT_TS") {
            assert!(
                ts == std::fs::read(expected).unwrap(),
                "same TS as the reference"
            );
        }
    }
}
