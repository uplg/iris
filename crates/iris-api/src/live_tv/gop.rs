//! Does a live MPEG-TS segment start on something a decoder can join?
//!
//! Some restreams cut their segments mid-GOP: the first H.264 slice comes
//! long before any SPS/PPS (Canal+ Foot's "1080p" Vavoo feed: first SPS
//! 286 KB into a 2.5 MB segment, 2026-10-20). A decoder that joins there has
//! no parameter sets and paints the frame green until the next IDR, and with
//! the feed's corrupt references the green can stick. The election prefers
//! a feed whose segments open with SPS + IDR.

const TS_PACKET: usize = 188;
const TS_SYNC: u8 = 0x47;
/// PMT `stream_type` of H.264 video. Other codecs aren't judged.
const STREAM_TYPE_H264: u8 = 0x1B;

/// How the first video access unit of a segment opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// SPS before the first slice: a decoder joins cleanly.
    Clean,
    /// A slice before any SPS: a decoder joining here shows garbage.
    MidGop,
}

/// The verdict on the head of a TS segment; `None` when it can't tell (not
/// TS, not H.264, or no slice in the bytes given).
pub fn segment_start(ts: &[u8]) -> Option<Start> {
    let video = video_pid(ts)?;
    first_nal_verdict(&video_es(ts, video))
}

/// Iterate `(pid, payload_unit_start, payload)` over the whole packets of
/// `ts`, from its first sync byte.
fn packets(ts: &[u8]) -> impl Iterator<Item = (u16, bool, &[u8])> {
    let start = ts.iter().position(|&b| b == TS_SYNC).unwrap_or(ts.len());
    ts[start..]
        .as_chunks::<TS_PACKET>()
        .0
        .iter()
        .take_while(|p| p[0] == TS_SYNC)
        .filter_map(|p| {
            let pid = (u16::from(p[1] & 0x1F) << 8) | u16::from(p[2]);
            let pusi = p[1] & 0x40 != 0;
            let control = (p[3] >> 4) & 0x3;
            let offset = match control {
                1 => 4,
                3 => 5 + usize::from(p[4]),
                _ => return None,
            };
            p.get(offset..).map(|payload| (pid, pusi, payload))
        })
}

/// A PSI section's body after its pointer field and 8-byte header, up to
/// its CRC.
fn section(payload: &[u8]) -> Option<&[u8]> {
    let start = 1 + usize::from(*payload.first()?);
    let s = payload.get(start..)?;
    let len = usize::from(u16::from(*s.get(1)? & 0x0F) << 8 | u16::from(*s.get(2)?));
    s.get(8..(3 + len).checked_sub(4)?)
}

/// The H.264 video PID, from the PAT and the first program's PMT.
fn video_pid(ts: &[u8]) -> Option<u16> {
    let pat = packets(ts).find(|(pid, pusi, _)| *pid == 0 && *pusi)?.2;
    let pmt_pid = section(pat)?
        .as_chunks::<4>()
        .0
        .iter()
        .find(|e| u16::from(e[0]) << 8 | u16::from(e[1]) != 0)
        .map(|e| (u16::from(e[2] & 0x1F) << 8) | u16::from(e[3]))?;
    let pmt = packets(ts)
        .find(|(pid, pusi, _)| *pid == pmt_pid && *pusi)?
        .2;
    let body = section(pmt)?;
    let info_len = usize::from(u16::from(*body.get(2)? & 0x0F) << 8 | u16::from(*body.get(3)?));
    let mut streams = body.get(4 + info_len..)?;
    while streams.len() >= 5 {
        let kind = streams[0];
        let pid = (u16::from(streams[1] & 0x1F) << 8) | u16::from(streams[2]);
        let es_len = usize::from(u16::from(streams[3] & 0x0F) << 8 | u16::from(streams[4]));
        if kind == STREAM_TYPE_H264 {
            return Some(pid);
        }
        streams = streams.get(5 + es_len..)?;
    }
    None
}

/// The elementary stream of `pid`, PES headers stripped, from its first
/// PES start.
fn video_es(ts: &[u8], video: u16) -> Vec<u8> {
    let mut es = Vec::new();
    let mut started = false;
    for (pid, pusi, payload) in packets(ts) {
        if pid != video {
            continue;
        }
        if pusi {
            let Some(header_len) = payload.get(8).map(|&n| 9 + usize::from(n)) else {
                continue;
            };
            if !payload.starts_with(&[0, 0, 1]) {
                continue;
            }
            started = true;
            es.extend_from_slice(payload.get(header_len..).unwrap_or_default());
        } else if started {
            es.extend_from_slice(payload);
        }
    }
    es
}

/// SPS (7) before the first slice (1 or 5) is clean; a slice first is not.
fn first_nal_verdict(es: &[u8]) -> Option<Start> {
    let mut i = 0;
    while i + 3 < es.len() {
        if es[i] == 0 && es[i + 1] == 0 && es[i + 2] == 1 {
            match es[i + 3] & 0x1F {
                7 => return Some(Start::Clean),
                1 | 5 => return Some(Start::MidGop),
                _ => {}
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const VIDEO_PID: u16 = 0x100;
    const PMT_PID: u16 = 0x1000;

    fn packet(pid: u16, pusi: bool, payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() <= TS_PACKET - 4);
        let mut p = vec![
            TS_SYNC,
            (if pusi { 0x40 } else { 0 }) | u8::try_from(pid >> 8).unwrap(),
            u8::try_from(pid & 0xFF).unwrap(),
            0x10,
        ];
        if payload.len() < TS_PACKET - 4 {
            // Stuff through an adaptation field so the payload ends the packet.
            let stuffing = TS_PACKET - 4 - payload.len();
            p[3] = 0x30;
            p.push(u8::try_from(stuffing - 1).unwrap());
            if stuffing > 1 {
                p.push(0);
                p.extend(std::iter::repeat_n(0xFF, stuffing - 2));
            }
        }
        p.extend_from_slice(payload);
        p
    }

    fn psi(table_id: u8, body: &[u8]) -> Vec<u8> {
        let len = u16::try_from(5 + body.len() + 4).unwrap();
        let mut s = vec![
            0,
            table_id,
            0xB0 | u8::try_from(len >> 8).unwrap(),
            u8::try_from(len & 0xFF).unwrap(),
            0,
            1,
            0xC1,
            0,
            0,
        ];
        s.extend_from_slice(body);
        s.extend_from_slice(&[0, 0, 0, 0]);
        s
    }

    /// A segment: PAT, PMT naming `stream_type` on the video PID, then one
    /// PES holding `nals` (each its type byte and a little payload).
    pub(crate) fn segment(stream_type: u8, nals: &[u8]) -> Vec<u8> {
        let pat = psi(
            0,
            &[
                0,
                1,
                0xE0 | u8::try_from(PMT_PID >> 8).unwrap(),
                u8::try_from(PMT_PID & 0xFF).unwrap(),
            ],
        );
        let pmt = psi(
            2,
            &[
                0xE1,
                0x00,
                0xF0,
                0x00,
                stream_type,
                0xE0 | u8::try_from(VIDEO_PID >> 8).unwrap(),
                u8::try_from(VIDEO_PID & 0xFF).unwrap(),
                0xF0,
                0x00,
            ],
        );
        let mut es = Vec::new();
        for &nal in nals {
            es.extend_from_slice(&[0, 0, 0, 1, nal, 0xAA, 0xBB, 0xCC]);
        }
        let mut pes = vec![0, 0, 1, 0xE0, 0, 0, 0x80, 0x80, 5, 0x21, 0, 1, 0, 1];
        pes.extend_from_slice(&es);
        let mut ts = packet(0, true, &pat);
        ts.extend(packet(PMT_PID, true, &pmt));
        for (i, chunk) in pes.chunks(TS_PACKET - 4).enumerate() {
            ts.extend(packet(VIDEO_PID, i == 0, chunk));
        }
        ts
    }

    #[test]
    fn sps_first_is_clean() {
        let aud_sps_pps_idr = [0x09, 0x67, 0x68, 0x65];
        assert_eq!(
            segment_start(&segment(STREAM_TYPE_H264, &aud_sps_pps_idr)),
            Some(Start::Clean)
        );
    }

    #[test]
    fn a_slice_before_any_sps_is_mid_gop() {
        assert_eq!(
            segment_start(&segment(STREAM_TYPE_H264, &[0x09, 0x06, 0x41, 0x67])),
            Some(Start::MidGop),
            "a P slice first"
        );
        assert_eq!(
            segment_start(&segment(STREAM_TYPE_H264, &[0x09, 0x65])),
            Some(Start::MidGop),
            "an IDR without its parameter sets"
        );
    }

    #[test]
    fn what_it_cannot_judge_is_left_alone() {
        assert_eq!(segment_start(&segment(0x24, &[0x41])), None, "HEVC");
        assert_eq!(
            segment_start(&segment(STREAM_TYPE_H264, &[0x09, 0x06])),
            None,
            "no slice yet"
        );
        assert_eq!(segment_start(b"#EXTM3U\n"), None);
        assert_eq!(segment_start(&[]), None);
        let mut ts = segment(STREAM_TYPE_H264, &[0x67, 0x65]);
        ts.truncate(TS_PACKET + 10);
        assert_eq!(segment_start(&ts), None, "cut before the video");
    }

    /// A real captured head: `IRIS_TS_HEAD=/path/seg.ts cargo test -p
    /// iris-api gop_real -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a captured segment on disk"]
    fn gop_real_segment() {
        let path = std::env::var("IRIS_TS_HEAD").expect("IRIS_TS_HEAD");
        let bytes = std::fs::read(path).unwrap();
        let head = &bytes[..bytes.len().min(super::super::GOP_PROBE_BYTES)];
        eprintln!("{:?}", segment_start(head));
    }
}
