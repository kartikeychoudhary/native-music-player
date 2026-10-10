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
    if size < body_start - start { return Ok(None); }
    Ok(Some(BoxHdr { typ, body_start, end: (start + size).min(limit) }))
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

/// Reads the timescale of mvhd/mdhd (both FullBoxes: v0 has 32-bit times, v1 64-bit).
fn read_timescale<R: Read + Seek>(r: &mut R, h: &BoxHdr) -> io::Result<u32> {
    r.seek(SeekFrom::Start(h.body_start))?;
    let version = read_u32(r)? >> 24;
    r.seek(SeekFrom::Current(if version == 1 { 16 } else { 8 }))?;
    read_u32(r)
}

pub fn read_mp4_gapless<R: Read + Seek>(r: &mut R) -> io::Result<Option<Mp4Gapless>> {
    let len = r.seek(SeekFrom::End(0))?;
    let mut moov = None;
    children(r, 0, len, |_, h| { if &h.typ == b"moov" { moov = Some((h.body_start, h.end)); } Ok(()) })?;
    let Some((ms, me)) = moov else { return Ok(None) };

    let mut movie_ts = 0u32;
    let mut smpb = None;
    let mut audio_elst: Option<(u64, u64, u32)> = None; // (media_time, movie_duration, media_ts)

    children(r, ms, me, |r, h| {
        match &h.typ {
            b"mvhd" => movie_ts = read_timescale(r, h)?,
            b"trak" => {
                let (mut is_audio, mut media_ts, mut edit) = (false, 0u32, None);
                children(r, h.body_start, h.end, |r, t| {
                    match &t.typ {
                        b"edts" => children(r, t.body_start, t.end, |r, e| {
                            if &e.typ == b"elst" { edit = read_first_elst(r, e)?; }
                            Ok(())
                        })?,
                        b"mdia" => children(r, t.body_start, t.end, |r, m| {
                            match &m.typ {
                                b"mdhd" => media_ts = read_timescale(r, m)?,
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
                    if let Some((mt, dur)) = edit { audio_elst = Some((mt, dur, media_ts)); }
                }
            }
            b"udta" => smpb = find_itunsmpb(r, h)?,
            _ => {}
        }
        Ok(())
    })?;

    if let Some(g) = smpb { return Ok(Some(g)); }
    Ok(audio_elst.map(|(media_time, movie_dur, media_ts)| Mp4Gapless::EditList {
        media_time,
        duration: if movie_ts == 0 { 0 } else { (movie_dur as u128 * media_ts as u128 / movie_ts as u128) as u64 },
        timescale: media_ts,
    }))
}

/// First edit with media_time >= 0 → (media_time, segment_duration in movie timescale).
fn read_first_elst<R: Read + Seek>(r: &mut R, h: &BoxHdr) -> io::Result<Option<(u64, u64)>> {
    r.seek(SeekFrom::Start(h.body_start))?;
    let version = read_u32(r)? >> 24;
    let count = read_u32(r)?;
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
                let (mut name, mut value) = (String::new(), String::new());
                children(r, item.body_start, item.end, |r, c| {
                    let n = (c.end - c.body_start) as usize;
                    let mut buf = vec![0u8; n];
                    r.seek(SeekFrom::Start(c.body_start))?;
                    r.read_exact(&mut buf)?;
                    match &c.typ {
                        b"name" if n > 4 => name = String::from_utf8_lossy(&buf[4..]).into_owned(),
                        b"data" if n > 8 => value = String::from_utf8_lossy(&buf[8..]).into_owned(),
                        _ => {}
                    }
                    Ok(())
                })?;
                if name == "iTunSMPB" { found = parse_itunsmpb(&value); }
                Ok(())
            })
        })
    })?;
    Ok(found)
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
    fn mdhd(ts: u32) -> Vec<u8> { let mut p = vec![0u8; 8]; p.extend(ts.to_be_bytes()); p.extend([0u8; 8]); bx(b"mdhd", &full(0, &p)) }
    fn hdlr(t: &[u8; 4]) -> Vec<u8> { let mut p = vec![0u8; 4]; p.extend(t); p.extend([0u8; 13]); bx(b"hdlr", &full(0, &p)) }
    fn elst(dur: u32, media_time: i32) -> Vec<u8> {
        let mut p = 1u32.to_be_bytes().to_vec();
        p.extend(dur.to_be_bytes()); p.extend(media_time.to_be_bytes()); p.extend(0x0001_0000u32.to_be_bytes());
        bx(b"elst", &full(0, &p))
    }

    fn movie(udta: Option<Vec<u8>>) -> Vec<u8> {
        let mdia = bx(b"mdia", &[mdhd(44100), hdlr(b"soun")].concat());
        let edts = bx(b"edts", &elst(1000 * 2, 1024)); // movie ts 1000 → 2 s
        let trak = bx(b"trak", &[edts, mdia].concat());
        let mut moov = [mvhd(1000), trak].concat();
        if let Some(u) = udta { moov.extend(u); }
        [bx(b"ftyp", b"M4A \0\0\0\0"), bx(b"moov", &moov)].concat()
    }

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
}
