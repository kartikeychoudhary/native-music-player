//! Tag and audio-property reading with `lofty` (spec §3.9 metadata list).
use std::path::Path;

use lofty::file::FileType;
use lofty::picture::PictureType;
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag};
use protocol::{Codec, ReplayGainInfo};

/// File extensions the scanner indexes (matches the app's `AUDIO_EXTENSIONS`, minus the "alac" pseudo-extension).
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "m4b", "mp4", "aac", "flac", "wav", "aif", "aiff", "aifc", "ogg", "oga", "opus", "dsf", "dff"];

pub fn container_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

pub fn is_audio(path: &Path) -> bool {
    AUDIO_EXTENSIONS.contains(&container_of(path).as_str())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioProps {
    pub duration_ms: Option<u64>,
    pub codec: Option<Codec>,
    pub container: String,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u16>,
    pub channels: Option<u16>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackTags {
    pub title: Option<String>,
    /// Native multi-value artist fields, in tag order. Never split heuristically.
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artists: Vec<String>,
    pub composer: Option<String>,
    pub track_no: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_no: Option<u32>,
    pub disc_total: Option<u32>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub replay_gain: ReplayGainInfo,
    pub mb_recording_id: Option<String>,
    pub mb_release_id: Option<String>,
    pub mb_artist_id: Option<String>,
    pub has_lyrics: bool,
    pub props: AudioProps,
    /// Embedded front cover (or the first picture when none is marked front).
    pub picture: Option<Vec<u8>>,
}

impl TrackTags {
    /// A record for a file `lofty` can't read but the decoder can (DSF/DFF): properties only.
    pub fn from_props(props: AudioProps, path: &Path) -> Self {
        TrackTags { props, has_lyrics: path.with_extension("lrc").is_file(), ..Default::default() }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("can't read tags: {0}")]
pub struct TagError(pub String);

/// Library-level codec from the container type. AAC is reported as AAC-LC here; playback's signal
/// path reports HE-AAC precisely. FLAC-in-MP4 (rare) is labelled ALAC because both carry a bit depth.
fn codec_for(ft: FileType, bit_depth: Option<u8>) -> Option<Codec> {
    Some(match ft {
        FileType::Mpeg => Codec::Mp3,
        FileType::Aac => Codec::AacLc,
        FileType::Mp4 if bit_depth.is_some() => Codec::Alac,
        FileType::Mp4 => Codec::AacLc,
        FileType::Flac => Codec::Flac,
        FileType::Wav | FileType::Aiff => Codec::Pcm,
        FileType::Vorbis => Codec::Vorbis,
        FileType::Opus => Codec::Opus,
        _ => return None,
    })
}

/// "-6.50 dB" → -6.5; "0.988" → 0.988.
pub fn parse_gain(s: &str) -> Option<f32> {
    let t = s.trim();
    let t = t.strip_suffix("dB").or_else(|| t.strip_suffix("db")).unwrap_or(t).trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// The first standalone four-digit run in a date string: "1999-05-01" → 1999, "05/1999" → 1999.
pub fn parse_year(s: &str) -> Option<i32> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(3))
        .find(|&i| {
            b[i..i + 4].iter().all(u8::is_ascii_digit)
                && (i == 0 || !b[i - 1].is_ascii_digit())
                && b.get(i + 4).is_none_or(|c| !c.is_ascii_digit())
        })
        .and_then(|i| s[i..i + 4].parse().ok())
        .filter(|y| (1000..=9999).contains(y))
}

fn texts(tag: &Tag, key: ItemKey) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for v in tag.get_strings(key).map(str::trim).filter(|s| !s.is_empty()) {
        if !out.iter().any(|o| o == v) { out.push(v.to_string()); }
    }
    out
}

fn text(tag: &Tag, key: ItemKey) -> Option<String> {
    tag.get_string(key).map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

fn fill(out: &mut TrackTags, tag: &Tag) {
    out.title = text(tag, ItemKey::TrackTitle);
    // An explicit multi-value ARTISTS field (Picard writes one) is more precise than ARTIST.
    out.artists = texts(tag, ItemKey::TrackArtists);
    if out.artists.is_empty() { out.artists = texts(tag, ItemKey::TrackArtist); }
    out.album = text(tag, ItemKey::AlbumTitle);
    out.album_artists = texts(tag, ItemKey::AlbumArtist);
    let composers = texts(tag, ItemKey::Composer);
    out.composer = (!composers.is_empty()).then(|| composers.join(", "));
    out.track_no = tag.track();
    out.track_total = tag.track_total();
    out.disc_no = tag.disk();
    out.disc_total = tag.disk_total();
    out.year = tag.date().map(|d| d.year as i32).or_else(|| {
        [ItemKey::RecordingDate, ItemKey::Year, ItemKey::OriginalReleaseDate].into_iter().find_map(|k| tag.get_string(k).and_then(parse_year))
    });
    out.genres = texts(tag, ItemKey::Genre);
    out.replay_gain = ReplayGainInfo {
        track_gain: text(tag, ItemKey::ReplayGainTrackGain).and_then(|s| parse_gain(&s)),
        track_peak: text(tag, ItemKey::ReplayGainTrackPeak).and_then(|s| parse_gain(&s)),
        album_gain: text(tag, ItemKey::ReplayGainAlbumGain).and_then(|s| parse_gain(&s)),
        album_peak: text(tag, ItemKey::ReplayGainAlbumPeak).and_then(|s| parse_gain(&s)),
    };
    out.mb_recording_id = text(tag, ItemKey::MusicBrainzRecordingId);
    out.mb_release_id = text(tag, ItemKey::MusicBrainzReleaseId);
    out.mb_artist_id = text(tag, ItemKey::MusicBrainzArtistId);
    out.has_lyrics |= text(tag, ItemKey::Lyrics).is_some() || text(tag, ItemKey::UnsyncLyrics).is_some();
    let pics = tag.pictures();
    out.picture = pics.iter().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| pics.first()).map(|p| p.data().to_vec());
}

pub fn read(path: &Path) -> Result<TrackTags, TagError> {
    let tf = lofty::read_from_path(path).map_err(|e| TagError(e.to_string()))?;
    let p = tf.properties();
    let props = AudioProps {
        duration_ms: Some(p.duration().as_millis() as u64).filter(|d| *d > 0),
        codec: codec_for(tf.file_type(), p.bit_depth()),
        container: container_of(path),
        bitrate_kbps: p.audio_bitrate().or(p.overall_bitrate()).filter(|b| *b > 0),
        sample_rate: p.sample_rate().filter(|r| *r > 0),
        bit_depth: p.bit_depth().map(u16::from),
        channels: p.channels().map(u16::from),
    };
    let mut out = TrackTags::from_props(props, path);
    if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) { fill(&mut out, tag); }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_parsing() {
        assert_eq!(parse_gain("-6.50 dB"), Some(-6.5));
        assert_eq!(parse_gain("+1.2 db"), Some(1.2));
        assert_eq!(parse_gain("0.988"), Some(0.988));
        assert_eq!(parse_gain("loud"), None);
        assert_eq!(parse_gain("NaN"), None);
    }

    #[test]
    fn year_parsing() {
        assert_eq!(parse_year("1999-05-01"), Some(1999));
        assert_eq!(parse_year("05/2003"), Some(2003));
        assert_eq!(parse_year("12345"), None);
        assert_eq!(parse_year("99"), None);
    }

    #[test]
    fn audio_extension_filter() {
        assert!(is_audio(Path::new("/a/B.FLAC")));
        assert!(is_audio(Path::new("x.dsf")));
        assert!(!is_audio(Path::new("cover.jpg")));
        assert!(!is_audio(Path::new("noext")));
    }
}
