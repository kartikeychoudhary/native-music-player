mod common;
use common::{demo_tree, scanned, tagged, Tags};
use protocol::RecentKind;

fn names<T>(items: &[T], f: impl Fn(&T) -> &str) -> Vec<String> { items.iter().map(|i| f(i).to_string()).collect() }

#[test]
fn prefix_search_groups_tracks_albums_artists() {
    let s = scanned(demo_tree);
    let r = s.lib.search("dus", 20).unwrap();
    assert_eq!(names(&r.tracks, |t| &t.title), ["Dusk"]);
    let r = s.lib.search("alp", 20).unwrap();
    assert_eq!(names(&r.artists, |a| &a.name), ["Alpha"]);
    assert_eq!(names(&r.albums, |a| &a.title), ["First Light", "Zenith"], "Zenith has no album-artist tag but groups under Alpha");
    assert_eq!(r.tracks.len(), 4);
}

#[test]
fn title_matches_rank_above_artist_matches() {
    let s = scanned(|root| {
        tagged("flac16_44k.flac", &root.join("a.flac"), &Tags { title: Some("Other"), artists: &["Nova"], album: Some("X"), ..Default::default() });
        tagged("flac16_44k.flac", &root.join("b.flac"), &Tags { title: Some("Nova"), artists: &["Someone"], album: Some("Y"), ..Default::default() });
    });
    let r = s.lib.search("nova", 20).unwrap();
    assert_eq!(names(&r.tracks, |t| &t.title), ["Nova", "Other"]);
}

#[test]
fn missing_tracks_crowding_the_top_matches_do_not_hide_live_ones() {
    let s = scanned(|root| {
        for i in 0..6 {
            tagged("flac16_44k.flac", &root.join(format!("n{i}.flac")), &Tags { title: Some("Nova"), artists: &["Someone"], album: Some("Y"), ..Default::default() });
        }
        tagged("flac16_44k.flac", &root.join("o.flac"), &Tags { title: Some("Other"), artists: &["Nova"], album: Some("X"), ..Default::default() });
    });
    // Rows stay in the FTS table; with limit 1 the first window (4 candidates) is all missing tracks.
    s.lib.conn().execute("UPDATE tracks SET missing_since = 1 WHERE title = 'Nova'", []).unwrap();
    let r = s.lib.search("nova", 1).unwrap();
    assert_eq!(names(&r.tracks, |t| &t.title), ["Other"]);
}

#[test]
fn diacritics_fold_in_tracks_and_artists() {
    let s = scanned(|root| {
        tagged("flac16_44k.flac", &root.join("c.flac"), &Tags { title: Some("Café Society"), artists: &["Zoë Keating"], album: Some("Into the Trees"), ..Default::default() });
    });
    let r = s.lib.search("cafe", 20).unwrap();
    assert_eq!(r.tracks.len(), 1);
    let r = s.lib.search("zoe kea", 20).unwrap();
    assert_eq!(names(&r.artists, |a| &a.name), ["Zoë Keating"]);
}

#[test]
fn hostile_short_and_empty_queries_return_nothing_without_errors() {
    let s = scanned(demo_tree);
    for q in ["", " ", "a", "\"*", "\" OR 1=1 --", "NEAR(", "{title}:x"] {
        let r = s.lib.search(q, 20).unwrap_or_else(|e| panic!("{q:?}: {e}"));
        if q.trim().chars().count() < 2 { assert!(r.tracks.is_empty() && r.albums.is_empty() && r.artists.is_empty(), "{q:?}"); }
    }
}

#[test]
fn missing_tracks_are_not_found() {
    let s = scanned(demo_tree);
    std::fs::remove_file(s.root().join("The Gamma/Night/01.mp3")).unwrap();
    s.rescan();
    let r = s.lib.search("dusk", 20).unwrap();
    assert!(r.tracks.is_empty() && r.albums.is_empty());
}

#[test]
fn play_history_drives_recent_and_most_played() {
    let s = scanned(demo_tree);
    let (dawn, peak, span) = (s.id_of("Dawn"), s.id_of("Peak"), s.id_of("Span"));
    s.lib.record_play(dawn, 100).unwrap();
    s.lib.record_play(peak, 200).unwrap();
    s.lib.record_play(dawn, 300).unwrap();
    let played = s.lib.recent(RecentKind::Played, 10).unwrap().tracks;
    assert_eq!(names(&played, |t| &t.title), ["Dawn", "Peak"]);
    let most = s.lib.recent(RecentKind::MostPlayed, 10).unwrap().tracks;
    assert_eq!(names(&most, |t| &t.title), ["Dawn", "Peak"]);
    assert_eq!(most[0].play_count, 2);
    assert_eq!(most[0].last_played_at, Some(300));
    assert!(matches!(s.lib.record_play(999_999, 1), Err(library::LibError::NotFound(_))));
    let _ = span;
}

#[test]
fn recently_added_albums_newest_first() {
    let s = scanned(demo_tree);
    s.lib.conn().execute("UPDATE albums SET added_at = CASE title WHEN 'Night' THEN 50 WHEN 'Zenith' THEN 40 ELSE 10 END", []).unwrap();
    let r = s.lib.recent(RecentKind::Added, 2).unwrap();
    assert_eq!(names(&r.albums, |a| &a.title), ["Night", "Zenith"]);
    assert!(r.tracks.is_empty());
}

#[test]
fn unplayable_marks_set_and_clear() {
    let s = scanned(demo_tree);
    let path = s.lib.tracks_by_ids(&[s.id_of("Span")]).unwrap()[0].path.clone();
    assert!(s.lib.set_unplayable(&path, Some("DECODE_FAILED")).unwrap());
    assert_eq!(s.lib.track(s.id_of("Span")).unwrap().track.unplayable.as_deref(), Some("DECODE_FAILED"));
    assert!(s.lib.set_unplayable(&path, None).unwrap());
    assert!(s.lib.track(s.id_of("Span")).unwrap().track.unplayable.is_none());
    assert!(!s.lib.set_unplayable("/not/in/library.flac", Some("X")).unwrap());
}
