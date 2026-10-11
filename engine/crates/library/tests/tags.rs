mod common;
use common::{tagged, Tags};
use library::tags;
use lofty::picture::PictureType;
use lofty::tag::ItemKey;
use protocol::Codec;

#[test]
fn reads_basic_fields_and_properties() {
    let dir = tempfile::tempdir().unwrap();
    let p = tagged("flac24_96k.flac", &dir.path().join("a.flac"), &Tags {
        title: Some("Opening"), artists: &["Alpha"], album: Some("First Light"), album_artist: Some("Alpha"),
        track: Some(3), disc: Some(1), date: Some("1999-05-01"), genres: &["Ambient"], ..Default::default()
    });
    let t = tags::read(&p).unwrap();
    assert_eq!(t.title.as_deref(), Some("Opening"));
    assert_eq!(t.artists, vec!["Alpha"]);
    assert_eq!(t.album.as_deref(), Some("First Light"));
    assert_eq!(t.album_artists, vec!["Alpha"]);
    assert_eq!((t.track_no, t.disc_no, t.year), (Some(3), Some(1), Some(1999)));
    assert_eq!(t.genres, vec!["Ambient"]);
    assert_eq!(t.props.codec, Some(Codec::Flac));
    assert_eq!(t.props.container, "flac");
    assert_eq!(t.props.sample_rate, Some(96000));
    assert_eq!(t.props.bit_depth, Some(24));
    assert_eq!(t.props.channels, Some(2));
    assert!(t.props.duration_ms.unwrap() > 500);
}

#[test]
fn multi_value_artists_and_genres_use_native_fields_only() {
    let dir = tempfile::tempdir().unwrap();
    let p = tagged("flac16_44k.flac", &dir.path().join("m.flac"), &Tags {
        artists: &["Alpha", "Beta & Gamma"], genres: &["Jazz", "Funk"], ..Default::default()
    });
    let t = tags::read(&p).unwrap();
    assert_eq!(t.artists, vec!["Alpha", "Beta & Gamma"], "no splitting on '&'");
    assert_eq!(t.genres, vec!["Jazz", "Funk"]);
}

#[test]
fn front_cover_wins_over_other_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let p = tagged("flac16_44k.flac", &dir.path().join("p.flac"), &Tags {
        pictures: &[(PictureType::Other, b"other"), (PictureType::CoverFront, b"front")], ..Default::default()
    });
    assert_eq!(tags::read(&p).unwrap().picture.as_deref(), Some(&b"front"[..]));
}

#[test]
fn replaygain_musicbrainz_and_composer() {
    let dir = tempfile::tempdir().unwrap();
    let p = tagged("flac16_44k.flac", &dir.path().join("r.flac"), &Tags {
        extra: &[
            (ItemKey::ReplayGainTrackGain, "-6.50 dB"), (ItemKey::ReplayGainTrackPeak, "0.988"),
            (ItemKey::MusicBrainzRecordingId, "rec-1"), (ItemKey::Composer, "C. Writer"), (ItemKey::Lyrics, "la la"),
        ],
        ..Default::default()
    });
    let t = tags::read(&p).unwrap();
    assert_eq!(t.replay_gain.track_gain, Some(-6.5));
    assert_eq!(t.replay_gain.track_peak, Some(0.988));
    assert_eq!(t.mb_recording_id.as_deref(), Some("rec-1"));
    assert_eq!(t.composer.as_deref(), Some("C. Writer"));
    assert!(t.has_lyrics);
}

#[test]
fn untagged_file_still_has_properties() {
    let t = tags::read(&common::fixture("pcm16_44k.wav")).unwrap();
    assert!(t.title.is_none() && t.artists.is_empty());
    assert_eq!(t.props.codec, Some(Codec::Pcm));
    assert_eq!(t.props.container, "wav");
}

#[test]
fn mp4_codec_distinguishes_alac_from_aac() {
    assert_eq!(tags::read(&common::fixture("alac16_44k.m4a")).unwrap().props.codec, Some(Codec::Alac));
    assert_eq!(tags::read(&common::fixture("aac_lc_256.m4a")).unwrap().props.codec, Some(Codec::AacLc));
    assert_eq!(tags::read(&common::fixture("mp3_320.mp3")).unwrap().props.codec, Some(Codec::Mp3));
    assert_eq!(tags::read(&common::fixture("opus_160.opus")).unwrap().props.codec, Some(Codec::Opus));
}

#[test]
fn sidecar_lrc_sets_has_lyrics() {
    let dir = tempfile::tempdir().unwrap();
    let p = tagged("flac16_44k.flac", &dir.path().join("song.flac"), &Tags::default());
    std::fs::write(dir.path().join("song.lrc"), "[00:01.00]hi").unwrap();
    assert!(tags::read(&p).unwrap().has_lyrics);
}

#[test]
fn empty_and_dsd_files_are_errors_for_lofty() {
    assert!(tags::read(&common::fixture("empty.mp3")).is_err());
    assert!(tags::read(&common::fixture("dsd64.dsf")).is_err(), "DSD tags are read through the fallback probe");
}
