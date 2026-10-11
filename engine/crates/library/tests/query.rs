mod common;
use common::{demo_tree, scanned};
use protocol::{ListFilter, ListQuery, SortKey};

fn q(sort: Option<SortKey>, desc: bool, offset: u32, limit: u32, filter: ListFilter) -> ListQuery {
    ListQuery { filter: Some(filter), sort, desc: Some(desc), offset: Some(offset), limit: Some(limit) }
}
fn titles<T>(items: &[T], f: impl Fn(&T) -> &str) -> Vec<String> { items.iter().map(|i| f(i).to_string()).collect() }

#[test]
fn albums_page_by_title_by_default() {
    let s = scanned(demo_tree);
    let p = s.lib.albums(&ListQuery { limit: Some(2), ..Default::default() }).unwrap();
    assert_eq!(p.total, 4);
    assert_eq!(titles(&p.items, |a| &a.title), ["Bridges", "First Light"]);
    let p = s.lib.albums(&ListQuery { offset: Some(2), limit: Some(2), ..Default::default() }).unwrap();
    assert_eq!(titles(&p.items, |a| &a.title), ["Night", "Zenith"]);
}

#[test]
fn albums_sort_by_artist_uses_sort_name_then_year() {
    let s = scanned(demo_tree);
    let p = s.lib.albums(&q(Some(SortKey::Artist), false, 0, 10, Default::default())).unwrap();
    assert_eq!(titles(&p.items, |a| &a.title), ["Zenith", "First Light", "Bridges", "Night"], "'The Gamma' sorts as 'gamma'");
    assert_eq!(p.items[1].artist, "Alpha");
}

#[test]
fn albums_sort_by_year_desc_puts_unknown_last() {
    let s = scanned(demo_tree);
    let p = s.lib.albums(&q(Some(SortKey::Year), true, 0, 10, Default::default())).unwrap();
    assert_eq!(titles(&p.items, |a| &a.title), ["Bridges", "First Light", "Zenith", "Night"]);
}

#[test]
fn albums_filter_by_artist_includes_appearances_and_by_genre() {
    let s = scanned(demo_tree);
    let beta: i64 = s.lib.conn().query_row("SELECT id FROM artists WHERE name = 'Beta'", [], |r| r.get(0)).unwrap();
    let p = s.lib.albums(&q(None, false, 0, 10, ListFilter { artist_id: Some(beta), ..Default::default() })).unwrap();
    assert_eq!(titles(&p.items, |a| &a.title), ["Bridges", "First Light"]);
    let ambient: i64 = s.lib.conn().query_row("SELECT id FROM genres WHERE name = 'Ambient'", [], |r| r.get(0)).unwrap();
    let p = s.lib.albums(&q(None, false, 0, 10, ListFilter { genre_id: Some(ambient), ..Default::default() })).unwrap();
    assert_eq!(p.total, 2);
}

#[test]
fn album_detail_orders_by_disc_then_track() {
    let s = scanned(demo_tree);
    let id = s.lib.albums(&ListQuery::default()).unwrap().items.into_iter().find(|a| a.title == "First Light").unwrap().id;
    let d = s.lib.album(id).unwrap();
    assert_eq!(titles(&d.tracks, |t| &t.title), ["Dawn", "Noon", "Encore"]);
    assert_eq!(d.album.track_count, 3);
    assert_eq!(d.album.year, Some(2001));
    assert_eq!(d.album.max_sample_rate, Some(96000));
    assert_eq!(d.album.max_bit_depth, Some(24));
    assert_eq!(d.album.codec, Some(protocol::Codec::Flac));
    assert_eq!(titles(&d.genres, |g| &g.name), ["Ambient"]);
}

#[test]
fn artist_detail_lists_albums_newest_first_and_top_tracks_by_plays() {
    let s = scanned(demo_tree);
    let alpha: i64 = s.lib.conn().query_row("SELECT id FROM artists WHERE name = 'Alpha'", [], |r| r.get(0)).unwrap();
    s.lib.conn().execute("UPDATE tracks SET play_count = 5 WHERE title = 'Peak'", []).unwrap();
    let d = s.lib.artist(alpha).unwrap();
    assert_eq!(d.artist.name, "Alpha");
    assert_eq!(d.artist.album_count, 2);
    assert_eq!(titles(&d.albums, |a| &a.title), ["First Light", "Zenith"]);
    assert_eq!(d.top_tracks[0].title, "Peak");
    assert_eq!(d.top_tracks.len(), 4);
}

#[test]
fn tracks_filter_sort_and_text() {
    let s = scanned(demo_tree);
    let all = s.lib.tracks(&q(Some(SortKey::Title), false, 0, 100, Default::default())).unwrap();
    assert_eq!(titles(&all.items, |t| &t.title), ["Dawn", "Dusk", "Encore", "Noon", "Peak", "Span"]);
    let p = s.lib.tracks(&q(None, false, 0, 100, ListFilter { text: Some("da".into()), ..Default::default() })).unwrap();
    assert_eq!(titles(&p.items, |t| &t.title), ["Dawn"]);
    let p = s.lib.tracks(&q(Some(SortKey::Duration), true, 0, 1, Default::default())).unwrap();
    assert_eq!(p.total, 6);
    assert_eq!(p.items.len(), 1);
    let noon = &all.items[3];
    assert_eq!(noon.artist, "Alpha, Beta");
    assert!(noon.artist_id.is_some() && noon.album_id.is_some());
}

#[test]
fn tracks_by_ids_preserves_order_and_rejects_unknown() {
    let s = scanned(demo_tree);
    let (a, b) = (s.id_of("Span"), s.id_of("Dawn"));
    let rows = s.lib.tracks_by_ids(&[a, b, a]).unwrap();
    assert_eq!(titles(&rows, |t| &t.title), ["Span", "Dawn", "Span"]);
    assert!(matches!(s.lib.tracks_by_ids(&[a, 999_999]), Err(library::LibError::NotFound(_))));
    assert_eq!(s.lib.track_id_for_path(&rows[0].path), Some(a));
}

#[test]
fn track_detail_has_credits_and_file_facts() {
    let s = scanned(demo_tree);
    let d = s.lib.track(s.id_of("Noon")).unwrap();
    assert_eq!(titles(&d.artists, |a| &a.name), ["Alpha", "Beta"]);
    assert_eq!(titles(&d.genres, |g| &g.name), ["Ambient"]);
    assert!(d.size_bytes > 0);
    assert!(d.mtime > 1_000_000_000_000, "milliseconds");
    assert!(d.gapless.is_none());
}

#[test]
fn missing_tracks_are_hidden_from_lists_but_resolvable() {
    let s = scanned(demo_tree);
    let dusk = s.id_of("Dusk");
    std::fs::remove_file(s.root().join("The Gamma/Night/01.mp3")).unwrap();
    s.rescan();
    assert_eq!(s.lib.albums(&ListQuery::default()).unwrap().total, 3);
    assert_eq!(s.lib.tracks(&ListQuery::default()).unwrap().total, 5);
    assert_eq!(s.lib.artists(&ListQuery::default()).unwrap().total, 2);
    assert!(s.lib.track(dusk).unwrap().track.missing);
}

#[test]
fn artist_and_genre_lists() {
    let s = scanned(demo_tree);
    let a = s.lib.artists(&ListQuery::default()).unwrap();
    assert_eq!(titles(&a.items, |x| &x.name), ["Alpha", "Beta", "The Gamma"]);
    let g = s.lib.genres(&ListQuery::default()).unwrap();
    assert_eq!(titles(&g.items, |x| &x.name), ["Ambient", "Jazz", "Rock"]);
    assert_eq!((g.items[0].album_count, g.items[0].track_count), (2, 4));
    assert_eq!(s.lib.genre(g.items[0].id).unwrap().name, "Ambient");
}

#[test]
fn artist_genre_filter_ignores_missing_tracks() {
    let s = scanned(demo_tree);
    let rock: i64 = s.lib.conn().query_row("SELECT id FROM genres WHERE name = 'Rock'", [], |r| r.get(0)).unwrap();
    let by_rock = || q(None, false, 0, 10, ListFilter { genre_id: Some(rock), ..Default::default() });
    assert_eq!(titles(&s.lib.artists(&by_rock()).unwrap().items, |a| &a.name), ["Alpha"]);
    // Alpha stays visible through its other tracks, but its only Rock track (Peak) goes missing.
    std::fs::remove_file(s.root().join("Alpha/Zenith/01.flac")).unwrap();
    s.rescan();
    assert_eq!(s.lib.artists(&ListQuery::default()).unwrap().total, 3);
    assert_eq!(s.lib.artists(&by_rock()).unwrap().total, 0);
}

#[test]
fn oversized_limits_are_clamped_not_errors() {
    let s = scanned(demo_tree);
    assert_eq!(s.lib.tracks(&ListQuery { limit: Some(50_000), ..Default::default() }).unwrap().items.len(), 6);
    assert_eq!(s.lib.tracks(&ListQuery { limit: Some(0), ..Default::default() }).unwrap().items.len(), 1);
}
