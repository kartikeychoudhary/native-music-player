//! Lyrics (spec §3.9): sidecar `.lrc`, embedded text tags and ID3 SYLT; synced or plain.
use std::path::Path;

use lofty::prelude::*;
use lofty::tag::ItemKey;
use protocol::{Lyrics, LyricLine, LyricWord};

/// UTF-8 (BOM stripped), or Latin-1 when the bytes aren't UTF-8 (common in older `.lrc` files).
pub fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// `mm:ss`, `mm:ss.f+` or `mm:ss:f+` → milliseconds.
fn parse_time(s: &str) -> Option<u32> {
    let (min, rest) = s.split_once(':')?;
    let (sec, frac) = match rest.find(['.', ':']) { Some(i) => (&rest[..i], Some(&rest[i + 1..])), None => (rest, None) };
    if min.is_empty() || !min.bytes().all(|b| b.is_ascii_digit()) || sec.len() != 2 || !sec.bytes().all(|b| b.is_ascii_digit()) { return None; }
    let (min, sec): (u32, u32) = (min.parse().ok()?, sec.parse().ok()?);
    if sec >= 60 { return None; }
    let ms = match frac {
        None => 0,
        Some(f) if !f.is_empty() && f.len() <= 3 && f.bytes().all(|b| b.is_ascii_digit()) => f.parse::<u32>().ok()? * 10u32.pow(3 - f.len() as u32),
        Some(_) => return None,
    };
    Some(min * 60_000 + sec * 1000 + ms)
}

/// Splits enhanced-LRC `<mm:ss.xx>` word tags out of a line's text. Text before the first tag is
/// kept only in the line text; each word runs from its tag to the next one.
fn words(text: &str) -> (String, Option<Vec<LyricWord>>) {
    let mut out: Vec<LyricWord> = Vec::new();
    let mut lead = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find('>').map(|c| open + c) else { break };
        let Some(t) = parse_time(&rest[open + 1..close]) else { break };
        match out.last_mut() { Some(w) => w.text.push_str(&rest[..open]), None => lead.push_str(&rest[..open]) }
        out.push(LyricWord { t_ms: t, text: String::new() });
        rest = &rest[close + 1..];
    }
    match out.last_mut() { Some(w) => w.text.push_str(rest), None => return (text.trim().to_string(), None) }
    let joined: String = std::iter::once(lead.as_str()).chain(out.iter().map(|w| w.text.as_str())).collect();
    (joined.trim().to_string(), Some(out))
}

pub fn parse_lrc(text: &str) -> Option<Lyrics> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut offset: i64 = 0;
    let mut lines: Vec<LyricLine> = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim_end_matches('\r').trim_start();
        let mut stamps = Vec::new();
        while let Some(tag) = rest.strip_prefix('[').and_then(|r| r.find(']').map(|c| (&r[..c], &r[c + 1..]))) {
            let (inner, after) = tag;
            if let Some(t) = parse_time(inner) {
                stamps.push(t);
            } else if let Some(v) = inner.strip_prefix("offset:") {
                offset = v.trim().parse().unwrap_or(0);
            } else if stamps.is_empty() {
                // A metadata tag ([ar:], [ti:] …) or a malformed one: the line isn't lyrics.
                rest = "";
                break;
            } else {
                break;
            }
            rest = after;
        }
        if stamps.is_empty() { continue; }
        let (line_text, line_words) = words(rest);
        for t in stamps {
            let shift = |t: u32| (t as i64 - offset).max(0) as u32;
            lines.push(LyricLine {
                t_ms: Some(shift(t)),
                text: line_text.clone(),
                words: line_words.as_ref().map(|ws| ws.iter().map(|w| LyricWord { t_ms: shift(w.t_ms), text: w.text.clone() }).collect()),
            });
        }
    }
    if lines.is_empty() { return None; }
    lines.sort_by_key(|l| l.t_ms);
    Some(Lyrics { synced: true, lines })
}

pub fn plain(text: &str) -> Lyrics {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let all: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r').trim_end()).collect();
    let start = all.iter().position(|l| !l.is_empty()).unwrap_or(all.len());
    let end = all.iter().rposition(|l| !l.is_empty()).map_or(start, |e| e + 1);
    Lyrics { synced: false, lines: all[start..end].iter().map(|l| LyricLine { t_ms: None, text: (*l).to_string(), words: None }).collect() }
}

fn sidecar(path: &Path) -> Option<String> {
    ["lrc", "LRC"].iter().find_map(|ext| std::fs::read(path.with_extension(ext)).ok()).map(|b| decode_text(&b))
}

fn embedded_text(path: &Path) -> Option<String> {
    let tf = lofty::read_from_path(path).ok()?;
    let tag = tf.primary_tag().or_else(|| tf.first_tag())?;
    [ItemKey::Lyrics, ItemKey::UnsyncLyrics].into_iter().find_map(|k| tag.get_string(k).map(str::to_string)).filter(|s| !s.trim().is_empty())
}

/// ID3v2 SYLT with millisecond timestamps (MPEG-frame timestamps are skipped).
fn sylt(path: &Path) -> Option<Lyrics> {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, SynchronizedTextFrame, TimestampFormat};
    use lofty::mpeg::MpegFile;
    if crate::tags::container_of(path) != "mp3" { return None; }
    let mp3 = MpegFile::read_from(&mut std::fs::File::open(path).ok()?, ParseOptions::new()).ok()?;
    for frame in mp3.id3v2()? {
        let Frame::Binary(bin) = frame else { continue };
        if frame.id().as_str() != "SYLT" { continue; }
        let Ok(s) = SynchronizedTextFrame::parse(&bin.data, frame.flags()) else { continue };
        if s.timestamp_format != TimestampFormat::MS { continue; }
        let mut lines: Vec<LyricLine> = s.content.into_iter().map(|(t, text)| LyricLine { t_ms: Some(t), text: text.trim().to_string(), words: None }).collect();
        if lines.is_empty() { continue; }
        lines.sort_by_key(|l| l.t_ms);
        return Some(Lyrics { synced: true, lines });
    }
    None
}

/// The first synced source (sidecar → SYLT → embedded text), else the first plain one.
pub fn for_file(path: &Path) -> Option<Lyrics> {
    let mut fallback: Option<Lyrics> = None;
    if let Some(t) = sidecar(path) {
        match parse_lrc(&t) { Some(l) => return Some(l), None => { fallback.get_or_insert_with(|| plain(&t)); } }
    }
    if let Some(l) = sylt(path) { return Some(l); }
    if let Some(t) = embedded_text(path) {
        match parse_lrc(&t) { Some(l) => return Some(l), None => { fallback.get_or_insert_with(|| plain(&t)); } }
    }
    fallback.filter(|l| !l.lines.is_empty())
}

impl crate::Library {
    pub fn lyrics(&self, track_id: i64) -> crate::Result<Option<Lyrics>> {
        use rusqlite::OptionalExtension;
        let path: Option<String> = self.conn().query_row("SELECT path FROM tracks WHERE id = ?1", [track_id], |r| r.get(0)).optional()?;
        let path = path.ok_or_else(|| crate::LibError::NotFound(format!("track {track_id}")))?;
        Ok(for_file(Path::new(&path)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times(l: &Lyrics) -> Vec<(Option<u32>, &str)> { l.lines.iter().map(|x| (x.t_ms, x.text.as_str())).collect() }

    #[test]
    fn lrc_basic_lines_and_repeated_tags() {
        let l = parse_lrc("[ar:Someone]\n[00:01.50]First\n[00:03.00][01:00.25]Chorus\n[00:02]Second\n").unwrap();
        assert!(l.synced);
        assert_eq!(times(&l), [(Some(1500), "First"), (Some(2000), "Second"), (Some(3000), "Chorus"), (Some(60_250), "Chorus")]);
    }

    #[test]
    fn lrc_fraction_widths_and_long_minutes() {
        let l = parse_lrc("[00:01.5]a\n[00:01.05]b\n[00:01.005]c\n[00:02:50]d\n[123:00.00]e").unwrap();
        assert_eq!(times(&l), [(Some(1005), "c"), (Some(1050), "b"), (Some(1500), "a"), (Some(2500), "d"), (Some(7_380_000), "e")]);
    }

    #[test]
    fn lrc_offset_moves_lines_earlier_and_never_below_zero() {
        let l = parse_lrc("[offset:+500]\n[00:00.20]early\n[00:02.00]later").unwrap();
        assert_eq!(times(&l), [(Some(0), "early"), (Some(1500), "later")]);
        let l = parse_lrc("[offset:-250]\n[00:01.00]x").unwrap();
        assert_eq!(times(&l), [(Some(1250), "x")]);
    }

    #[test]
    fn enhanced_lrc_gives_word_timing() {
        let l = parse_lrc("[00:10.00]<00:10.00>Hello <00:10.50>bright <00:11.20>world").unwrap();
        let line = &l.lines[0];
        assert_eq!(line.text, "Hello bright world");
        let words: Vec<(u32, &str)> = line.words.as_ref().unwrap().iter().map(|w| (w.t_ms, w.text.as_str())).collect();
        assert_eq!(words, [(10_000, "Hello "), (10_500, "bright "), (11_200, "world")]);
    }

    #[test]
    fn lrc_handles_real_world_quirks() {
        let text = "\u{feff}[ti:Song]\r\n[00:05.00]five\r\n[xx:yy]not a time\r\n[00:01.00]one\r\n\r\nno tag here\r\n[00:03.00]";
        let l = parse_lrc(text).unwrap();
        assert_eq!(times(&l), [(Some(1000), "one"), (Some(3000), ""), (Some(5000), "five")], "an empty timed line is a pause");
        assert_eq!(decode_text(&[b'[', b'0', b'0', b':', b'0', b'1', b']', 0xE9, b't', 0xE9]), "[00:01]été", "Latin-1 fallback");
        assert_eq!(decode_text("\u{feff}abc".as_bytes()), "abc");
    }

    #[test]
    fn lrc_with_no_timestamps_is_plain() {
        assert!(parse_lrc("[ar:x]\nJust words\nMore words").is_none());
        let p = plain("\n\nVerse one\nline two\n\nVerse two\n\n");
        assert!(!p.synced);
        assert_eq!(times(&p), [(None, "Verse one"), (None, "line two"), (None, ""), (None, "Verse two")]);
    }
}
