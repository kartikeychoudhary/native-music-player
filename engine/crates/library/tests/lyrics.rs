mod common;
use common::{scanned, tagged, Tags};
use lofty::tag::ItemKey;

#[test]
fn sidecar_lrc_wins_and_embedded_text_is_the_fallback() {
    let s = scanned(|root| {
        tagged("flac16_44k.flac", &root.join("a.flac"), &Tags { title: Some("A"), extra: &[(ItemKey::Lyrics, "plain embedded")], ..Default::default() });
        std::fs::write(root.join("a.lrc"), "[00:01.00]from sidecar").unwrap();
        tagged("flac16_44k.flac", &root.join("b.flac"), &Tags { title: Some("B"), extra: &[(ItemKey::Lyrics, "Line one\nLine two")], ..Default::default() });
        tagged("flac16_44k.flac", &root.join("c.flac"), &Tags { title: Some("C"), extra: &[(ItemKey::Lyrics, "[00:02.00]synced embedded")], ..Default::default() });
        std::fs::write(root.join("c.lrc"), "just plain sidecar").unwrap();
        tagged("flac16_44k.flac", &root.join("d.flac"), &Tags { title: Some("D"), ..Default::default() });
    });
    let a = s.lib.lyrics(s.id_of("A")).unwrap().unwrap();
    assert!(a.synced && a.lines[0].text == "from sidecar");
    let b = s.lib.lyrics(s.id_of("B")).unwrap().unwrap();
    assert!(!b.synced && b.lines.len() == 2);
    let c = s.lib.lyrics(s.id_of("C")).unwrap().unwrap();
    assert!(c.synced && c.lines[0].text == "synced embedded", "a synced source beats an earlier plain one");
    assert_eq!(s.lib.lyrics(s.id_of("D")).unwrap(), None);
    assert!(matches!(s.lib.lyrics(999_999), Err(library::LibError::NotFound(_))));
}

#[test]
fn id3_sylt_frames_are_synced_lyrics() {
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{BinaryFrame, Frame, FrameId, Id3v2Tag, SyncTextContentType, SynchronizedTextFrame, TimestampFormat};
    use lofty::mpeg::MpegFile;
    use lofty::TextEncoding;
    let s = scanned(|root| {
        let path = tagged("mp3_320.mp3", &root.join("s.mp3"), &Tags { title: Some("S"), ..Default::default() });
        let sylt = SynchronizedTextFrame::new(TextEncoding::UTF8, *b"eng", TimestampFormat::MS, SyncTextContentType::Lyrics, None, vec![(2500, "two".into()), (1000, "one".into())]);
        let mut mp3 = MpegFile::read_from(&mut std::fs::File::open(&path).unwrap(), ParseOptions::new()).unwrap();
        let mut tag = mp3.id3v2().cloned().unwrap_or_else(Id3v2Tag::new);
        tag.insert(Frame::Binary(BinaryFrame::new(FrameId::Valid("SYLT".into()), sylt.as_bytes(WriteOptions::default()).unwrap())));
        mp3.set_id3v2(tag);
        mp3.save_to_path(&path, WriteOptions::default()).unwrap();
    });
    let l = s.lib.lyrics(s.id_of("S")).unwrap().unwrap();
    assert!(l.synced);
    assert_eq!(l.lines.iter().map(|x| (x.t_ms, x.text.as_str())).collect::<Vec<_>>(), [(Some(1000), "one"), (Some(2500), "two")]);
}
