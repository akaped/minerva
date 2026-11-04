//! Splitting long recordings for transcription: providers take up to 25 MB at
//! a time, so a longer recording goes in parts, each a valid file of its own.
//! Cuts fall where the format allows: between WebM clusters (the headers
//! copied to each part), between Ogg pages (the header pages copied, pages
//! renumbered), inside a WAV's samples (a new header each), or between MP3
//! frames. Other formats are sent whole.

/// Splits `bytes` into parts of at most `max` bytes; a file that already fits
/// is one part. `None` when the format cannot be split.
pub(crate) fn split(bytes: &[u8], max: usize) -> Option<Vec<Vec<u8>>> {
    if bytes.len() <= max {
        return Some(vec![bytes.to_vec()]);
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        webm(bytes, max)
    } else if bytes.starts_with(b"OggS") {
        ogg(bytes, max)
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        wav(bytes, max)
    } else if bytes.starts_with(b"ID3")
        || (bytes.len() > 2 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0)
    {
        mp3(bytes, max)
    } else {
        None
    }
}

/// Groups consecutive `pieces` (byte ranges) into parts of at most `room`
/// bytes each; `None` if one piece alone is too big.
fn group(pieces: &[(usize, usize)], room: usize) -> Option<Vec<(usize, usize)>> {
    let mut out: Vec<(usize, usize)> = vec![];
    for &(start, end) in pieces {
        if end - start > room {
            return None;
        }
        match out.last_mut() {
            Some(last) if end - last.0 <= room => last.1 = end,
            _ => out.push((start, end)),
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// WebM (Matroska): EBML elements; browsers write the segment, and often its
// clusters, with an unknown size.

const SEGMENT: u32 = 0x1853_8067;
const CLUSTER: u32 = 0x1F43_B675;
const CLUSTER_CHILDREN: [u32; 7] = [0xE7, 0xA7, 0xAB, 0xA3, 0xA0, 0xAF, 0x5854];

/// An element's ID (with its length marker) and where its size field ends.
fn ebml_id(b: &[u8], at: usize) -> Option<(u32, usize)> {
    let first = *b.get(at)?;
    let len = first.leading_zeros() as usize + 1;
    if len > 4 {
        return None;
    }
    let mut id = 0u32;
    for i in 0..len {
        id = (id << 8) | u32::from(*b.get(at + i)?);
    }
    Some((id, at + len))
}
/// An element's size (`None` inside for "unknown") and where its data starts.
fn ebml_size(b: &[u8], at: usize) -> Option<(Option<u64>, usize)> {
    let first = *b.get(at)?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 {
        return None;
    }
    let mut value = u64::from(first) & ((1u64 << (8 - len)) - 1);
    let mut all_ones = value == (1u64 << (8 - len)) - 1;
    for i in 1..len {
        let byte = *b.get(at + i)?;
        all_ones &= byte == 0xff;
        value = (value << 8) | u64::from(byte);
    }
    Some(((!all_ones).then_some(value), at + len))
}
fn webm(b: &[u8], max: usize) -> Option<Vec<Vec<u8>>> {
    // The EBML header, then the segment.
    let (_, at) = ebml_id(b, 0)?;
    let (size, data) = ebml_size(b, at)?;
    let segment_at = data + usize::try_from(size?).ok()?;
    let (id, at) = ebml_id(b, segment_at)?;
    if id != SEGMENT {
        return None;
    }
    let (_, segment_data) = ebml_size(b, at)?;
    // The segment's children: headers until the first cluster, then clusters.
    let mut pos = segment_data;
    let mut first_cluster = None;
    let mut clusters: Vec<(usize, usize)> = vec![];
    while pos < b.len() {
        let (id, at) = ebml_id(b, pos)?;
        let (size, data) = ebml_size(b, at)?;
        let end = match size {
            Some(n) => data.checked_add(usize::try_from(n).ok()?)?.min(b.len()),
            None if id == CLUSTER => {
                // Up to the first element that is not one of a cluster's.
                let mut p = data;
                while p < b.len() {
                    let Some((cid, cat)) = ebml_id(b, p) else {
                        break;
                    };
                    if !CLUSTER_CHILDREN.contains(&cid) {
                        break;
                    }
                    let (Some(n), cdata) = ebml_size(b, cat)? else {
                        break;
                    };
                    p = cdata.checked_add(usize::try_from(n).ok()?)?.min(b.len());
                }
                p
            }
            None => return None,
        };
        if id == CLUSTER {
            first_cluster.get_or_insert(pos);
            clusters.push((pos, end));
        }
        if end <= pos {
            return None;
        }
        pos = end;
    }
    let first_cluster = first_cluster?;
    // Each part: the EBML header, the segment (of unknown size) and its
    // headers, then its clusters.
    let mut head = b[..segment_at].to_vec();
    head.extend_from_slice(&[
        0x18, 0x53, 0x80, 0x67, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ]);
    head.extend_from_slice(&b[segment_data..first_cluster]);
    let parts = group(&clusters, max.checked_sub(head.len())?)?;
    Some(
        parts
            .into_iter()
            .map(|(s, e)| [head.as_slice(), &b[s..e]].concat())
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Ogg: pages; the first ones (granule position 0) hold the codec's headers.

fn ogg_pages(b: &[u8]) -> Option<Vec<(usize, usize)>> {
    let mut out = vec![];
    let mut pos = 0;
    while pos + 27 <= b.len() {
        if &b[pos..pos + 4] != b"OggS" {
            return None;
        }
        let segments = usize::from(b[pos + 26]);
        let table = b.get(pos + 27..pos + 27 + segments)?;
        let body: usize = table.iter().map(|&n| usize::from(n)).sum();
        let end = pos + 27 + segments + body;
        if end > b.len() {
            return None;
        }
        out.push((pos, end));
        pos = end;
    }
    (pos == b.len()).then_some(out)
}
fn ogg_crc(page: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &byte in page {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}
/// A page renumbered as `sequence`, with its checksum redone.
fn ogg_page(page: &[u8], sequence: u32) -> Vec<u8> {
    let mut p = page.to_vec();
    p[18..22].copy_from_slice(&sequence.to_le_bytes());
    p[22..26].copy_from_slice(&[0; 4]);
    let crc = ogg_crc(&p);
    p[22..26].copy_from_slice(&crc.to_le_bytes());
    p
}
fn ogg(b: &[u8], max: usize) -> Option<Vec<Vec<u8>>> {
    let pages = ogg_pages(b)?;
    let granule = |(s, _): (usize, usize)| u64::from_le_bytes(b[s + 6..s + 14].try_into().unwrap());
    let headers = pages
        .iter()
        .take_while(|p| granule(**p) == 0)
        .count()
        .max(1);
    let head: usize = pages[..headers].iter().map(|(s, e)| e - s).sum();
    let parts = group(&pages[headers..], max.checked_sub(head)?)?;
    Some(
        parts
            .into_iter()
            .map(|(start, end)| {
                let mut out = vec![];
                for (sequence, &(s, e)) in pages[..headers]
                    .iter()
                    .chain(pages.iter().filter(|(s, _)| *s >= start && *s < end))
                    .enumerate()
                {
                    out.extend(ogg_page(&b[s..e], sequence as u32));
                }
                out
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// WAV: the samples, cut on whole frames, each part with its own header.

fn wav(b: &[u8], max: usize) -> Option<Vec<Vec<u8>>> {
    let mut pos = 12;
    let mut fmt: Option<&[u8]> = None;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let size = u32::from_le_bytes(b[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = b.get(pos + 8..(pos + 8 + size).min(b.len()))?;
        match id {
            b"fmt " => fmt = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        pos += 8 + size + size % 2;
    }
    let (fmt, data) = (fmt?, data?);
    let align = usize::from(u16::from_le_bytes(fmt.get(12..14)?.try_into().ok()?)).max(1);
    let head = 12 + 8 + fmt.len() + 8;
    let room = max.checked_sub(head)? / align * align;
    if room == 0 {
        return None;
    }
    Some(
        data.chunks(room)
            .map(|samples| {
                let mut out = b"RIFF".to_vec();
                out.extend(((head - 8 + samples.len()) as u32).to_le_bytes());
                out.extend(b"WAVEfmt ");
                out.extend((fmt.len() as u32).to_le_bytes());
                out.extend(fmt);
                out.extend(b"data");
                out.extend((samples.len() as u32).to_le_bytes());
                out.extend(samples);
                out
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// MP3: frames stand alone; a cut goes just before a frame's sync word.

fn mp3(b: &[u8], max: usize) -> Option<Vec<Vec<u8>>> {
    let frame_at = |from: usize| {
        (from..b.len().saturating_sub(1))
            .find(|&i| b[i] == 0xff && b[i + 1] & 0xe0 == 0xe0 && b[i + 1] & 0x06 != 0)
    };
    let mut out = vec![];
    let mut start = 0;
    while b.len() - start > max {
        // The last frame start before the limit.
        let mut cut = start + max;
        while cut > start
            && !(b[cut] == 0xff
                && b.get(cut + 1)
                    .is_some_and(|n| n & 0xe0 == 0xe0 && n & 0x06 != 0))
        {
            cut -= 1;
        }
        if cut <= start {
            cut = frame_at(start + 1)?;
        }
        out.push(b[start..cut].to_vec());
        start = cut;
    }
    out.push(b[start..].to_vec());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ogg_file(pages: usize) -> Vec<u8> {
        // Two header pages, then audio pages of 255 bytes each.
        let mut out = vec![];
        for i in 0..pages {
            let mut p = b"OggS\0".to_vec();
            p.push(if i == 0 { 2 } else { 0 });
            let granule: u64 = if i < 2 { 0 } else { (i as u64) * 960 };
            p.extend(granule.to_le_bytes());
            p.extend(7u32.to_le_bytes());
            p.extend((i as u32).to_le_bytes());
            p.extend([0; 4]);
            p.push(1);
            p.push(255);
            p.extend(vec![i as u8; 255]);
            out.extend(ogg_page(&p, i as u32));
        }
        out
    }
    #[test]
    fn ogg_parts_keep_their_headers_and_numbering() {
        let file = ogg_file(12);
        let parts = split(&file, 1500).unwrap();
        assert!(parts.len() > 1);
        for part in &parts {
            assert!(part.len() <= 1500);
            let pages = ogg_pages(part).unwrap();
            // Headers first, numbered from 0, checksums right.
            assert_eq!(part[5], 2, "begins with the first header page");
            for (n, (s, e)) in pages.iter().enumerate() {
                let page = &part[*s..*e];
                assert_eq!(
                    u32::from_le_bytes(page[18..22].try_into().unwrap()),
                    n as u32
                );
                assert_eq!(ogg_page(page, n as u32), page);
            }
        }
        let audio: usize = parts.iter().map(|p| ogg_pages(p).unwrap().len() - 2).sum();
        assert_eq!(audio, 10, "every audio page once");
    }
    #[test]
    fn webm_parts_repeat_the_headers() {
        // EBML header, a segment of unknown size with Info and Tracks, then
        // clusters of unknown size holding SimpleBlocks.
        let mut file = vec![0x1a, 0x45, 0xdf, 0xa3, 0x84, 1, 2, 3, 4];
        file.extend([
            0x18, 0x53, 0x80, 0x67, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        ]);
        file.extend([0x15, 0x49, 0xa9, 0x66, 0x82, 9, 9]);
        file.extend([0x16, 0x54, 0xae, 0x6b, 0x82, 8, 8]);
        for c in 0..6u8 {
            file.extend([
                0x1f, 0x43, 0xb6, 0x75, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            ]);
            // A Timecode, then a SimpleBlock of 100 bytes (a two-byte size).
            file.extend([0xe7, 0x81, c]);
            file.extend([0xa3, 0x40, 100]);
            file.extend(vec![c; 100]);
        }
        let parts = split(&file, 300).unwrap();
        assert!(parts.len() >= 2, "{}", parts.len());
        for part in &parts {
            assert!(part.len() <= 300);
            assert_eq!(&part[..4], &[0x1a, 0x45, 0xdf, 0xa3]);
            assert!(
                part.windows(4).any(|w| w == [0x16, 0x54, 0xae, 0x6b]),
                "the tracks come along"
            );
            assert!(part.windows(4).any(|w| w == [0x1f, 0x43, 0xb6, 0x75]));
        }
    }
    #[test]
    fn wav_and_mp3_parts() {
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        let fmt = [1u8, 0, 1, 0, 0x80, 0x3e, 0, 0, 0, 0x7d, 0, 0, 2, 0, 16, 0];
        wav.extend((fmt.len() as u32).to_le_bytes());
        wav.extend(fmt);
        wav.extend(b"data");
        wav.extend(1000u32.to_le_bytes());
        wav.extend(vec![1u8; 1000]);
        let parts = split(&wav, 300).unwrap();
        assert!(parts
            .iter()
            .all(|p| p.len() <= 300 && p.starts_with(b"RIFF")));
        let samples: usize = parts.iter().map(|p| p.len() - 44).sum();
        assert_eq!(samples, 1000);
        let mut mp3 = vec![];
        for _ in 0..10 {
            mp3.extend([0xff, 0xfb, 0x90, 0x64]);
            mp3.extend(vec![0u8; 96]);
        }
        let parts = split(&mp3, 250).unwrap();
        assert!(parts
            .iter()
            .all(|p| p.len() <= 250 && p.starts_with(&[0xff, 0xfb])));
        assert_eq!(parts.concat(), mp3);
        assert_eq!(split(b"something else entirely, quite long", 10), None);
    }
}
