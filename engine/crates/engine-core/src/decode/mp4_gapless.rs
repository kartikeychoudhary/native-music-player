//! Gapless info for MP4/M4A. Symphonia 0.6's isomp4 demuxer doesn't apply edit lists or iTunSMPB.
use std::io::{self, Read, Seek, SeekFrom};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mp4Gapless {
    ITunSmpb { delay: u64, padding: u64, length: u64 },
    EditList { media_time: u64, duration: u64, timescale: u32 },
}

impl Mp4Gapless {
    pub fn to_frames(&self, out_rate: u32) -> (u64, Option<u64>) {
        match *self {
            Self::ITunSmpb { delay, length, .. } => (delay, (length > 0).then_some(length)),
            Self::EditList { media_time, duration, timescale } => {
                let conv = |v: u64| (v as u128 * out_rate as u128 / timescale.max(1) as u128) as u64;
                (conv(media_time), (duration > 0).then(|| conv(duration)))
            }
        }
    }
}

pub fn parse_itunsmpb(s: &str) -> Option<Mp4Gapless> {
    let f: Vec<&str> = s.split_whitespace().collect();
    if f.len() < 4 { return None; }
    Some(Mp4Gapless::ITunSmpb {
        delay: u64::from_str_radix(f[1], 16).ok()?,
        padding: u64::from_str_radix(f[2], 16).ok()?,
        length: u64::from_str_radix(f[3], 16).ok()?,
    })
}

struct BoxHdr { typ: [u8; 4], body_start: u64, end: u64 }

fn read_hdr<R: Read + Seek>(r: &mut R, limit: u64) -> io::Result<Option<BoxHdr>> {
    let start = r.stream_position()?;
    if start + 8 > limit { return Ok(None); }
    let mut h = [0u8; 8];
    if r.read_exact(&mut h).is_err() { return Ok(None); }
    let mut size = u32::from_be_bytes(h[0..4].try_into().unwrap()) as u64;
    let typ: [u8; 4] = h[4..8].try_into().unwrap();
    let mut body_start = start + 8;
    if size == 1 {
        let mut big = [0u8; 8];
        r.read_exact(&mut big)?;
        size = u64::from_be_bytes(big);
        body_start += 8;
    } else if size == 0 {
        size = limit - start;
    }
    // A box that is smaller than its header or runs past its parent is malformed: stop walking.
    if size < body_start - start || start.saturating_add(size) > limit { return Ok(None); }
    Ok(Some(BoxHdr { typ, body_start, end: start + size }))
}

/// Calls `f` for every child box of the range [from, to).
fn children<R: Read + Seek>(r: &mut R, from: u64, to: u64, mut f: impl FnMut(&mut R, &BoxHdr) -> io::Result<()>) -> io::Result<()> {
    let mut pos = from;
    while pos + 8 <= to {
        r.seek(SeekFrom::Start(pos))?;
        let Some(h) = read_hdr(r, to)? else { break };
        f(r, &h)?;
        if h.end <= pos { break; }
        pos = h.end;
    }
    Ok(())
}

fn read_u32<R: Read>(r: &mut R) -> io::Result<u32> { let mut b = [0u8; 4]; r.read_exact(&mut b)?; Ok(u32::from_be_bytes(b)) }
fn read_u64<R: Read>(r: &mut R) -> io::Result<u64> { let mut b = [0u8; 8]; r.read_exact(&mut b)?; Ok(u64::from_be_bytes(b)) }

/// Reads (timescale, duration) of an mvhd/mdhd (both FullBoxes: v0 has 32-bit times, v1 64-bit).
fn read_timescale_and_duration<R: Read + Seek>(r: &mut R, h: &BoxHdr) -> io::Result<(u32, u64)> {
    r.seek(SeekFrom::Start(h.body_start))?;
    let version = read_u32(r)? >> 24;
    r.seek(SeekFrom::Current(if version == 1 { 16 } else { 8 }))?;
    let timescale = read_u32(r)?;
    let duration = if version == 1 { read_u64(r)? } else { read_u32(r)? as u64 };
    Ok((timescale, duration))
}

/// An audio track's first edit, with the track's media timescale and mdhd duration.
#[derive(Clone, Copy)]
struct AudioEdit { media_time: u64, movie_dur: u64, media_ts: u32, media_dur: u64 }

/// Converts an audio edit into frames at the media timescale. The elst duration is in movie ticks
/// (1 ms for ffmpeg), so when mdhd's duration agrees to within one movie tick, it gives the exact length.
fn edit_list_gapless(e: AudioEdit, movie_ts: u32) -> Option<Mp4Gapless> {
    if e.media_ts == 0 || movie_ts == 0 { return None; }
    let (ts, mts) = (movie_ts as u64, e.media_ts as u64);
    let elst_valid = (e.movie_dur as u128 * mts as u128 / ts as u128) as u64;
    let tick = (mts + ts - 1) / ts;
    let duration = match e.media_dur.checked_sub(e.media_time) {
        Some(exact) if exact > 0 && exact.abs_diff(elst_valid) <= tick => exact,
        _ => elst_valid,
    };
    Some(Mp4Gapless::EditList { media_time: e.media_time, duration, timescale: e.media_ts })
}

/// Gapless info for the file, or `None` if there is none or the container is malformed or truncated.
/// Only real I/O failures are returned as errors.
pub fn read_mp4_gapless<R: Read + Seek>(r: &mut R) -> io::Result<Option<Mp4Gapless>> {
    match read_gapless(r) {
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        other => other,
    }
}

fn read_gapless<R: Read + Seek>(r: &mut R) -> io::Result<Option<Mp4Gapless>> {
    let len = r.seek(SeekFrom::End(0))?;
    let mut moov = None;
    children(r, 0, len, |_, h| { if &h.typ == b"moov" { moov = Some((h.body_start, h.end)); } Ok(()) })?;
    let Some((ms, me)) = moov else { return Ok(None) };

    let mut movie_ts = 0u32;
    let mut smpb = None;
    let mut audio_elst: Option<AudioEdit> = None;

    children(r, ms, me, |r, h| {
        match &h.typ {
            b"mvhd" => movie_ts = read_timescale_and_duration(r, h)?.0,
            b"trak" => {
                let (mut is_audio, mut media_ts, mut media_dur, mut edit) = (false, 0u32, 0u64, None);
                children(r, h.body_start, h.end, |r, t| {
                    match &t.typ {
                        b"edts" => children(r, t.body_start, t.end, |r, e| {
                            if &e.typ == b"elst" { edit = read_first_elst(r, e)?; }
                            Ok(())
                        })?,
                        b"mdia" => children(r, t.body_start, t.end, |r, m| {
                            match &m.typ {
                                b"mdhd" => (media_ts, media_dur) = read_timescale_and_duration(r, m)?,
                                b"hdlr" => {
                                    r.seek(SeekFrom::Start(m.body_start + 8))?;
                                    let mut ty = [0u8; 4];
                                    r.read_exact(&mut ty)?;
                                    is_audio = &ty == b"soun";
                                }
                                _ => {}
                            }
                            Ok(())
                        })?,
                        _ => {}
                    }
                    Ok(())
                })?;
                if is_audio && audio_elst.is_none() {
                    if let Some((media_time, movie_dur)) = edit {
                        audio_elst = Some(AudioEdit { media_time, movie_dur, media_ts, media_dur });
                    }
                }
            }
            b"udta" => smpb = find_itunsmpb(r, h)?,
            _ => {}
        }
        Ok(())
    })?;

    if let Some(g) = smpb { return Ok(Some(g)); }
    Ok(audio_elst.and_then(|e| edit_list_gapless(e, movie_ts)))
}

/// First edit with media_time >= 0 → (media_time, segment_duration in movie timescale).
fn read_first_elst<R: Read + Seek>(r: &mut R, h: &BoxHdr) -> io::Result<Option<(u64, u64)>> {
    r.seek(SeekFrom::Start(h.body_start))?;
    let version = read_u32(r)? >> 24;
    let count = read_u32(r)? as u64;
    let entry_len = if version == 1 { 20 } else { 12 };
    if 8 + count * entry_len > h.end - h.body_start { return Ok(None); } // entries must fit in the box
    for _ in 0..count {
        let (dur, mt) = if version == 1 { (read_u64(r)?, read_u64(r)? as i64) } else { (read_u32(r)? as u64, read_u32(r)? as i32 as i64) };
        read_u32(r)?; // media_rate
        if mt >= 0 { return Ok(Some((mt as u64, dur))); }
    }
    Ok(None)
}

/// udta → meta → ilst → "----" with name "iTunSMPB" → data.
fn find_itunsmpb<R: Read + Seek>(r: &mut R, udta: &BoxHdr) -> io::Result<Option<Mp4Gapless>> {
    let mut found = None;
    children(r, udta.body_start, udta.end, |r, m| {
        if &m.typ != b"meta" { return Ok(()); }
        // ISO-style meta is a FullBox (4 zero bytes); QuickTime-style isn't.
        r.seek(SeekFrom::Start(m.body_start))?;
        let skip = if read_u32(r)? == 0 { 4 } else { 0 };
        children(r, m.body_start + skip, m.end, |r, i| {
            if &i.typ != b"ilst" { return Ok(()); }
            children(r, i.body_start, i.end, |r, item| {
                if &item.typ != b"----" { return Ok(()); }
                let (mut name_span, mut data_span) = (None, None);
                children(r, item.body_start, item.end, |_, c| {
                    match &c.typ {
                        b"name" => name_span = Some((c.body_start, c.end)),
                        b"data" => data_span = Some((c.body_start, c.end)),
                        _ => {}
                    }
                    Ok(())
                })?;
                // Only the data of an iTunSMPB item is read, and only if it is a sane size.
                let name = match name_span { Some(s) => read_text(r, s, 4)?, None => String::new() };
                if name == "iTunSMPB" {
                    if let Some(s) = data_span { found = parse_itunsmpb(&read_text(r, s, 8)?); }
                }
                Ok(())
            })
        })
    })?;
    Ok(found)
}

/// Largest metadata value buffered. An iTunSMPB string is about 120 bytes.
const MAX_META_VALUE: u64 = 4096;

/// UTF-8 text of a metadata child after its `skip`-byte prefix, or empty if it is absent or oversized.
fn read_text<R: Read + Seek>(r: &mut R, (start, end): (u64, u64), skip: u64) -> io::Result<String> {
    let n = end - start;
    if n <= skip || n > MAX_META_VALUE { return Ok(String::new()); }
    let mut buf = vec![0u8; n as usize];
    r.seek(SeekFrom::Start(start))?;
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf[skip as usize..]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn bx(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(typ);
        v.extend_from_slice(payload);
        v
    }
    fn full(version: u8, rest: &[u8]) -> Vec<u8> { let mut v = vec![version, 0, 0, 0]; v.extend_from_slice(rest); v }

    fn mvhd(ts: u32) -> Vec<u8> { let mut p = vec![0u8; 8]; p.extend(ts.to_be_bytes()); p.extend([0u8; 88]); bx(b"mvhd", &full(0, &p)) }
    fn mdhd(ts: u32, dur: u32) -> Vec<u8> {
        let mut p = vec![0u8; 8]; p.extend(ts.to_be_bytes()); p.extend(dur.to_be_bytes()); p.extend([0u8; 4]);
        bx(b"mdhd", &full(0, &p))
    }
    fn hdlr(t: &[u8; 4]) -> Vec<u8> { let mut p = vec![0u8; 4]; p.extend(t); p.extend([0u8; 13]); bx(b"hdlr", &full(0, &p)) }
    fn elst_entry(dur: u32, media_time: i32) -> Vec<u8> {
        let mut e = dur.to_be_bytes().to_vec(); e.extend(media_time.to_be_bytes()); e.extend(0x0001_0000u32.to_be_bytes()); e
    }
    fn elst_box(count: u32, entries: &[u8]) -> Vec<u8> {
        let mut p = count.to_be_bytes().to_vec(); p.extend(entries); bx(b"elst", &full(0, &p))
    }
    fn edts_box(dur: u32, media_time: i32) -> Vec<u8> { bx(b"edts", &elst_box(1, &elst_entry(dur, media_time))) }

    /// One audio track with the given edts box, movie timescale, media timescale and mdhd duration.
    fn movie_with(movie_ts: u32, edts: Vec<u8>, media_ts: u32, media_dur: u32, udta: Option<Vec<u8>>) -> Vec<u8> {
        let mdia = bx(b"mdia", &[mdhd(media_ts, media_dur), hdlr(b"soun")].concat());
        let trak = bx(b"trak", &[edts, mdia].concat());
        let mut moov = [mvhd(movie_ts), trak].concat();
        if let Some(u) = udta { moov.extend(u); }
        [bx(b"ftyp", b"M4A \0\0\0\0"), bx(b"moov", &moov)].concat()
    }

    /// Movie ts 1000, 2 s edit at media_time 1024, 44.1 kHz, mdhd duration = 1024 + 88200.
    fn movie(udta: Option<Vec<u8>>) -> Vec<u8> { movie_with(1000, edts_box(2000, 1024), 44100, 89224, udta) }

    /// udta → meta → ilst → "----" (iTunSMPB) with the given data box.
    fn itunsmpb_udta(data_box: Vec<u8>) -> Vec<u8> {
        let mean = bx(b"mean", &full(0, b"com.apple.iTunes"));
        let name = bx(b"name", &full(0, b"iTunSMPB"));
        let ilst = bx(b"ilst", &bx(b"----", &[mean, name, data_box].concat()));
        let meta = bx(b"meta", &full(0, &[hdlr(b"mdir"), ilst].concat()));
        bx(b"udta", &meta)
    }
    fn itunsmpb_data(text: &[u8]) -> Vec<u8> { bx(b"data", &[&[0u8, 0, 0, 1, 0, 0, 0, 0][..], text].concat()) }

    #[test]
    fn parses_itunsmpb_string() {
        let g = parse_itunsmpb(" 00000000 00000840 000001CA 00000000000D2C36 00000000").unwrap();
        assert_eq!(g, Mp4Gapless::ITunSmpb { delay: 0x840, padding: 0x1CA, length: 0xD2C36 });
        assert_eq!(g.to_frames(44100), (2112, Some(0xD2C36)));
    }

    #[test]
    fn reads_edit_list_and_converts_movie_to_media_timescale() {
        let g = read_mp4_gapless(&mut Cursor::new(movie(None))).unwrap().unwrap();
        assert_eq!(g, Mp4Gapless::EditList { media_time: 1024, duration: 88200, timescale: 44100 });
        assert_eq!(g.to_frames(44100), (1024, Some(88200)));
        assert_eq!(g.to_frames(88200), (2048, Some(176400))); // HE-AAC: SBR doubles the output rate
    }

    #[test]
    fn itunsmpb_takes_precedence() {
        let data = bx(b"data", &[0u8, 0, 0, 1, 0, 0, 0, 0].iter().copied().chain(b" 00000000 00000840 00000100 0000000000015888".iter().copied()).collect::<Vec<u8>>());
        let mean = bx(b"mean", &full(0, b"com.apple.iTunes"));
        let name = bx(b"name", &full(0, b"iTunSMPB"));
        let ilst = bx(b"ilst", &bx(b"----", &[mean, name, data].concat()));
        let meta = bx(b"meta", &full(0, &[hdlr(b"mdir"), ilst].concat()));
        let udta = bx(b"udta", &meta);
        let g = read_mp4_gapless(&mut Cursor::new(movie(Some(udta)))).unwrap().unwrap();
        assert_eq!(g, Mp4Gapless::ITunSmpb { delay: 0x840, padding: 0x100, length: 0x15888 });
    }

    #[test]
    fn no_moov_means_none() {
        assert_eq!(read_mp4_gapless(&mut Cursor::new(bx(b"ftyp", b"isom"))).unwrap(), None);
    }

    #[test]
    fn oversized_metadata_value_is_skipped_not_buffered() {
        // A 1 MiB iTunSMPB value is ignored (bounded reads); the edit list fallback applies.
        let mut text = b" 00000000 00000840 00000100 0000000000015888".to_vec();
        text.resize(1 << 20, b' ');
        let g = read_mp4_gapless(&mut Cursor::new(movie(Some(itunsmpb_udta(itunsmpb_data(&text)))))).unwrap().unwrap();
        assert_eq!(g, Mp4Gapless::EditList { media_time: 1024, duration: 88200, timescale: 44100 });
    }

    #[test]
    fn data_box_claiming_more_than_its_parent_is_ignored() {
        let mut data = itunsmpb_data(b" 00000000 00000840 00000100 0000000000015888");
        data[0..4].copy_from_slice(&0x7FFF_FFF0u32.to_be_bytes()); // declared size far beyond the file
        let g = read_mp4_gapless(&mut Cursor::new(movie(Some(itunsmpb_udta(data))))).unwrap().unwrap();
        assert_eq!(g, Mp4Gapless::EditList { media_time: 1024, duration: 88200, timescale: 44100 });
    }

    #[test]
    fn moov_truncated_mid_elst_is_none() {
        let bytes = movie(None);
        let at = bytes.windows(4).position(|w| w == b"elst").unwrap();
        let cut = bytes[..at + 10].to_vec(); // ends inside the elst entry count
        assert_eq!(read_mp4_gapless(&mut Cursor::new(cut)).unwrap(), None);
    }

    #[test]
    fn exact_length_from_mdhd_beats_movie_tick_rounding() {
        // elst says 2507 ms, which is 110568 samples; the track's real valid length is 110583.
        let bytes = movie_with(1000, edts_box(2507, 1024), 44100, 1024 + 110583, None);
        let g = read_mp4_gapless(&mut Cursor::new(bytes)).unwrap().unwrap();
        assert_eq!(g, Mp4Gapless::EditList { media_time: 1024, duration: 110583, timescale: 44100 });
    }

    #[test]
    fn mdhd_length_far_from_edit_list_is_ignored() {
        // Only the elst is trusted when mdhd disagrees by more than one movie tick.
        let bytes = movie_with(1000, edts_box(2000, 1024), 44100, 1024 + 300_000, None);
        let g = read_mp4_gapless(&mut Cursor::new(bytes)).unwrap().unwrap();
        assert_eq!(g, Mp4Gapless::EditList { media_time: 1024, duration: 88200, timescale: 44100 });
    }

    #[test]
    fn zero_media_timescale_ignores_edit_list() {
        let bytes = movie_with(1000, edts_box(2000, 1024), 0, 89224, None);
        assert_eq!(read_mp4_gapless(&mut Cursor::new(bytes)).unwrap(), None);
    }

    #[test]
    fn zero_movie_timescale_ignores_edit_list() {
        let bytes = movie_with(0, edts_box(2000, 1024), 44100, 89224, None);
        assert_eq!(read_mp4_gapless(&mut Cursor::new(bytes)).unwrap(), None);
    }

    #[test]
    fn largesize_overflowing_u64_is_none() {
        let mut big = 1u32.to_be_bytes().to_vec();
        big.extend(b"moov");
        big.extend(u64::MAX.to_be_bytes());
        let bytes = [bx(b"ftyp", b"isom"), big].concat();
        assert_eq!(read_mp4_gapless(&mut Cursor::new(bytes)).unwrap(), None);
    }

    #[test]
    fn elst_count_larger_than_box_is_none() {
        // Two entries declared but only one present; the parser must not read past the box.
        let edts = bx(b"edts", &elst_box(2, &elst_entry(1000, -1)));
        assert_eq!(read_mp4_gapless(&mut Cursor::new(movie_with(1000, edts, 44100, 89224, None))).unwrap(), None);
    }
}
