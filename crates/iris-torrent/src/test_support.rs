//! Test fixtures for this crate and the ones above it.

/// A multi-file `.torrent` named `name`; `salt` (a `source` key) changes
/// the infohash without changing the files, like one release on two
/// trackers. Piece hashes are zeros: nothing is ever verified offline.
///
/// # Panics
/// On a total size past `usize` pieces.
pub fn torrent_bytes(name: &str, files: &[(&str, u64)], salt: &str) -> Vec<u8> {
    fn bstr(out: &mut Vec<u8>, s: &[u8]) {
        out.extend_from_slice(format!("{}:", s.len()).as_bytes());
        out.extend_from_slice(s);
    }
    const PIECE: u64 = 16_384;
    let total: u64 = files.iter().map(|(_, len)| len).sum();
    let pieces = usize::try_from(total.div_ceil(PIECE)).unwrap();
    let mut info = b"d".to_vec();
    bstr(&mut info, b"files");
    info.push(b'l');
    for (path, len) in files {
        info.extend_from_slice(format!("d6:lengthi{len}e4:pathl").as_bytes());
        for part in path.split('/') {
            bstr(&mut info, part.as_bytes());
        }
        info.extend_from_slice(b"ee");
    }
    info.push(b'e');
    bstr(&mut info, b"name");
    bstr(&mut info, name.as_bytes());
    info.extend_from_slice(format!("12:piece lengthi{PIECE}e").as_bytes());
    bstr(&mut info, b"pieces");
    bstr(&mut info, &vec![0u8; pieces * 20]);
    bstr(&mut info, b"source");
    bstr(&mut info, salt.as_bytes());
    info.push(b'e');
    let mut out = b"d4:info".to_vec();
    out.extend_from_slice(&info);
    out.push(b'e');
    out
}
