mod common;
use common::{fixture, Engine};
use serde_json::{json, Value};

fn music_dir(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for f in files { std::fs::copy(fixture(f), dir.path().join(f)).unwrap(); }
    dir
}

fn with_library(data: &std::path::Path) -> Engine {
    Engine::spawn_with(&["rpc", "--data-dir", &data.to_string_lossy()], &[])
}

fn wait_scan_done(e: &mut Engine) -> Value { e.notified("library.scanProgress", |p| p["phase"] == "done") }

#[test]
fn library_methods_need_a_data_dir() {
    let mut e = Engine::spawn();
    for (m, p) in [("library.folders", json!({})), ("playlists.list", json!({})), ("queue.set", json!({ "trackIds": [1] }))] {
        let v = e.call(m, p);
        assert_eq!(v["error"]["code"], -32008, "{m}: {v}");
        assert!(v["error"]["message"].as_str().unwrap().contains("--data-dir"));
    }
}

#[test]
fn library_methods_report_why_the_library_failed_to_open() {
    let data = tempfile::tempdir().unwrap();
    let conn = library::db::connect(&data.path().join("library.db")).unwrap();
    conn.execute_batch("PRAGMA user_version = 99").unwrap();
    drop(conn);
    let mut e = with_library(data.path());
    let notice = e.notified("engine.error", |p| p["code"] == -32008);
    assert!(notice["message"].as_str().unwrap().contains("newer Aurora"), "{notice}");
    let v = e.call("library.folders", json!({}));
    assert_eq!(v["error"]["code"], -32008, "{v}");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(msg.starts_with("library unavailable: ") && msg.contains("newer Aurora") && !msg.contains("--data-dir"), "{msg}");
    let v = e.call("queue.set", json!({ "trackIds": [1] }));
    assert!(v["error"]["message"].as_str().unwrap().contains("newer Aurora"), "{v}");
}

#[test]
fn add_folder_scans_lists_and_plays_by_track_id() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac", "short_b.flac"]);
    let mut e = with_library(data.path());
    assert_eq!(e.next(std::time::Duration::from_secs(10))["method"], "engine.ready");
    let folder = e.ok("library.addFolder", json!({ "path": music.path() }));
    assert!(folder["id"].as_i64().unwrap() > 0);
    e.notified("library.changed", |p| p["scopes"].as_array().unwrap().iter().any(|s| s == "folders"));
    let done = wait_scan_done(&mut e);
    assert_eq!(done["added"], 2);
    e.notified("library.changed", |p| p["scopes"].as_array().unwrap().iter().any(|s| s == "tracks"));
    let tracks = e.ok("library.tracks", json!({ "sort": "title" }));
    assert_eq!(tracks["total"], 2);
    let ids: Vec<i64> = tracks["items"].as_array().unwrap().iter().map(|t| t["id"].as_i64().unwrap()).collect();
    assert_eq!(tracks["items"][0]["title"], "short_a");

    e.ok("queue.set", json!({ "trackIds": ids, "startIndex": 1 }));
    let q = e.notified("queue.changed", |p| p["items"].as_array().is_some_and(|i| i.len() == 2));
    assert_eq!(q["items"][1]["trackId"], ids[1]);
    assert_eq!(q["current"], 1);
    e.ok("player.play", json!({}));
    let st = e.notified("player.state", |p| p["status"] == "playing");
    assert_eq!(st["track"]["trackId"], ids[1]);
    assert_eq!(st["track"]["title"], "short_b");
}

#[test]
fn queue_set_validates_ids_and_start_index() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac"]);
    let mut e = with_library(data.path());
    e.ok("library.addFolder", json!({ "path": music.path() }));
    wait_scan_done(&mut e);
    let id = e.ok("library.tracks", json!({}))["items"][0]["id"].as_i64().unwrap();
    assert_eq!(e.call("queue.set", json!({ "trackIds": [id, 999_999] }))["error"]["code"], -32001);
    assert_eq!(e.call("queue.set", json!({ "trackIds": [id], "startIndex": 1 }))["error"]["code"], -32009);
    assert_eq!(e.call("queue.move", json!({ "from": 0, "to": 5 }))["error"]["code"], -32009);
}

#[test]
fn add_folder_errors_are_typed() {
    let data = tempfile::tempdir().unwrap();
    let mut e = with_library(data.path());
    let v = e.call("library.addFolder", json!({ "path": data.path().join("nope") }));
    assert_eq!(v["error"]["code"], -32009);
    assert_eq!(v["error"]["data"]["name"], "INVALID_ARGUMENT");
    assert_eq!(e.call("library.album", json!({ "id": 42 }))["error"]["code"], -32001);
    assert_eq!(e.call("library.nothing", json!({}))["error"]["code"], -32601);
}

fn scanned_engine(data: &std::path::Path, music: &std::path::Path) -> (Engine, Vec<i64>) {
    let mut e = with_library(data);
    e.ok("library.addFolder", json!({ "path": music }));
    wait_scan_done(&mut e);
    let tracks = e.ok("library.tracks", json!({ "sort": "title" }));
    let ids = tracks["items"].as_array().unwrap().iter().map(|t| t["id"].as_i64().unwrap()).collect();
    (e, ids)
}

fn titles(q: &Value) -> Vec<String> { q["items"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap().to_string()).collect() }

#[test]
fn queue_edits_over_rpc() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["gap_a.flac", "short_a.flac", "short_b.flac"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path()); // title order: gap_a, short_a, short_b
    e.ok("queue.set", json!({ "trackIds": [ids[0]] }));
    e.ok("queue.add", json!({ "trackIds": [ids[1]] }));
    e.ok("queue.insertNext", json!({ "trackIds": [ids[2]] }));
    let q = e.notified("queue.changed", |p| p["items"].as_array().is_some_and(|i| i.len() == 3));
    assert_eq!(titles(&q), ["gap_a", "short_b", "short_a"]);
    // `seen` still holds the earlier snapshots; clear it so each wait sees only the event for the edit just made.
    e.seen.clear();
    e.ok("queue.move", json!({ "from": 2, "to": 1 }));
    let q = e.notified("queue.changed", |p| p["items"][1]["title"] == "short_a");
    assert_eq!(titles(&q), ["gap_a", "short_a", "short_b"]);
    let remove = q["items"][2]["id"].clone();
    e.seen.clear();
    e.ok("queue.remove", json!({ "itemIds": [remove] }));
    let q = e.notified("queue.changed", |p| p["items"].as_array().is_some_and(|i| i.len() == 2));
    assert_eq!(titles(&q), ["gap_a", "short_a"]);
    e.ok("queue.setRepeat", json!({ "repeat": "all" }));
    e.notified("player.state", |p| p["repeat"] == "all");
    e.ok("queue.setShuffle", json!({ "shuffle": true }));
    e.notified("player.state", |p| p["shuffle"] == true);
    e.ok("queue.clear", json!({}));
    e.notified("queue.changed", |p| p["items"].as_array().is_some_and(|i| i.is_empty()));
}

#[test]
fn session_restores_after_restart() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["gap_a.flac", "gap_b.flac", "short_a.flac"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path()); // gap_a, gap_b, short_a
    e.ok("queue.set", json!({ "trackIds": ids, "startIndex": 1 }));
    e.ok("player.play", json!({}));
    e.notified("player.state", |p| p["status"] == "playing");
    e.ok("player.pause", json!({}));
    e.notified("player.state", |p| p["status"] == "paused");
    e.ok("player.seek", json!({ "ms": 1_200 }));
    e.notified("player.state", |p| p["status"] == "paused" && p["positionMs"] == 1_200);
    assert!(e.close().success());

    let mut e = with_library(data.path());
    e.notified("player.state", |p| p["status"] == "paused"); // the restore's own state event
    let st = e.ok("player.state", json!({}));
    assert_eq!(st["status"], "paused");
    assert_eq!(st["queueIndex"], 1);
    assert_eq!(st["positionMs"], 1_200);
    assert_eq!(st["track"]["trackId"], ids[1]);
    assert_eq!(st["track"]["title"], "gap_b");
    assert_eq!(e.ok("queue.get", json!({}))["items"].as_array().unwrap().len(), 3);
    e.ok("player.play", json!({}));
    let st = e.notified("player.state", |p| p["status"] == "playing");
    assert!(st["positionMs"].as_u64().unwrap() >= 1_200, "resumes where it left off: {st}");
}

#[test]
fn restored_queue_skips_missing_file() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac", "short_b.flac"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path());
    e.ok("queue.set", json!({ "trackIds": ids }));
    assert!(e.close().success());
    std::fs::remove_file(music.path().join("short_a.flac")).unwrap();

    let mut e = with_library(data.path());
    e.notified("player.state", |p| p["status"] == "paused");
    e.ok("player.play", json!({}));
    let err = e.notified("engine.error", |p| p["name"] == "FILE_MISSING");
    assert!(err["path"].as_str().unwrap().ends_with("short_a.flac"));
    let st = e.notified("player.state", |p| p["status"] == "playing");
    assert_eq!(st["track"]["title"], "short_b");
}

#[test]
fn a_finished_track_is_recorded_in_history() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path());
    e.ok("queue.set", json!({ "trackIds": ids }));
    e.ok("player.play", json!({}));
    e.notified("library.changed", |p| p["scopes"].as_array().unwrap().iter().any(|s| s == "history"));
    let recent = e.ok("library.recent", json!({ "kind": "played" }));
    assert_eq!(recent["tracks"][0]["id"], ids[0]);
    assert_eq!(recent["tracks"][0]["playCount"], 1);
    assert_eq!(e.ok("library.recent", json!({ "kind": "mostPlayed" }))["tracks"].as_array().unwrap().len(), 1);
}

#[test]
fn playlists_over_rpc_round_trip_through_m3u() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac", "short_b.flac"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path());
    let p = e.ok("playlists.create", json!({ "name": "Mix" }));
    e.notified("library.changed", |p| p["scopes"].as_array().unwrap().iter().any(|s| s == "playlists"));
    let id = p["id"].clone();
    e.ok("playlists.addTracks", json!({ "id": id, "trackIds": ids }));
    e.ok("playlists.reorder", json!({ "id": id, "from": 0, "to": 1 }));
    let detail = e.ok("playlists.get", json!({ "id": id }));
    assert_eq!(detail["items"][0]["track"]["title"], "short_b");
    let out = music.path().join("Mix.m3u8");
    e.ok("playlists.exportM3u", json!({ "id": id, "path": out }));
    let imported = e.ok("playlists.importM3u", json!({ "path": out }));
    assert_eq!(imported["skipped"], 0);
    assert_eq!(imported["playlist"]["trackCount"], 2);
    e.ok("playlists.delete", json!({ "id": id }));
    assert_eq!(e.ok("playlists.list", json!({})).as_array().unwrap().len(), 1, "only the imported playlist remains");
}

#[test]
fn watcher_picks_up_new_files_and_remove_folder_forgets_them() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["short_a.flac"]);
    let (mut e, _) = scanned_engine(data.path(), music.path());
    e.seen.clear();
    std::fs::copy(fixture("short_b.flac"), music.path().join("short_b.flac")).unwrap();
    let done = e.notified("library.scanProgress", |p| p["phase"] == "done" && p["added"] == 1);
    assert_eq!(done["added"], 1);
    assert_eq!(e.ok("library.tracks", json!({}))["total"], 2);
    let folder = e.ok("library.folders", json!({}))[0]["id"].clone();
    e.ok("library.removeFolder", json!({ "id": folder }));
    assert_eq!(e.ok("library.tracks", json!({}))["total"], 0);
    assert!(e.ok("library.folders", json!({})).as_array().unwrap().is_empty());
}

#[test]
fn track_detail_includes_probed_gapless_info() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["gap_a.m4a"]);
    let (mut e, ids) = scanned_engine(data.path(), music.path());
    let d = e.ok("library.track", json!({ "id": ids[0] }));
    assert_eq!(d["track"]["codec"], "aac-lc");
    assert!(d["gapless"]["delay"].as_u64().unwrap() > 0, "{d}");
    assert!(d["sizeBytes"].as_u64().unwrap() > 0);
}

#[test]
fn unicode_folder_paths_work_end_to_end() {
    let data = tempfile::tempdir().unwrap();
    let music = tempfile::tempdir().unwrap();
    let dir = music.path().join("Beyoncé").join("日本語");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(fixture("short_a.flac"), dir.join("♫ song.flac")).unwrap();
    let (mut e, ids) = scanned_engine(data.path(), music.path());
    assert_eq!(e.ok("library.search", json!({ "text": "song" }))["tracks"][0]["id"], ids[0]);
    e.ok("queue.set", json!({ "trackIds": ids }));
    e.ok("player.play", json!({}));
    let st = e.notified("player.state", |p| p["status"] == "playing");
    assert!(st["track"]["path"].as_str().unwrap().contains("日本語"));
}

#[test]
fn removing_a_folder_during_its_scan_ends_the_scan_promptly_and_quietly() {
    let data = tempfile::tempdir().unwrap();
    let music = tempfile::tempdir().unwrap();
    // Enough files that the scan is still reading when the remove arrives. Hard links keep this cheap.
    for i in 0..3000 {
        let dest = music.path().join(format!("t{i:04}.flac"));
        if std::fs::hard_link(fixture("short_a.flac"), &dest).is_err() { std::fs::copy(fixture("short_a.flac"), &dest).unwrap(); }
    }
    let mut e = with_library(data.path());
    let folder = e.ok("library.addFolder", json!({ "path": music.path() }))["id"].clone();
    e.notified("library.scanProgress", |p| p["phase"] == "reading" && p["total"].as_u64() > Some(0));
    e.seen.clear();
    let start = std::time::Instant::now();
    e.ok("library.removeFolder", json!({ "id": folder }));
    let took = start.elapsed();
    assert!(took < std::time::Duration::from_secs(1), "removeFolder took {took:?}");
    let done = e.notified("library.scanProgress", |p| p["phase"] == "done");
    assert!(done["processed"].as_u64() < done["total"].as_u64(), "the scan was cut short: {done}");
    // Give a late "scan failed" error time to arrive, then check none did.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(e.ok("library.folders", json!({})).as_array().unwrap().is_empty());
    assert_eq!(e.ok("library.tracks", json!({}))["total"], 0);
    let errors: Vec<&Value> = e.all.iter().filter(|v| v["method"] == "engine.error").collect();
    assert!(errors.is_empty(), "no engine.error expected: {errors:?}");
}

#[test]
fn lyrics_come_from_the_library() {
    let data = tempfile::tempdir().unwrap();
    let music = music_dir(&["flac16_44k.flac"]);
    std::fs::write(music.path().join("flac16_44k.lrc"), "[00:00.50]Hello\n[00:01.50]World").unwrap();
    let mut e = with_library(data.path());
    e.ok("library.addFolder", json!({ "path": music.path() }));
    wait_scan_done(&mut e);
    let id = e.ok("library.tracks", json!({}))["items"][0]["id"].clone();
    let l = e.ok("lyrics.get", json!({ "trackId": id }));
    assert_eq!(l, json!({ "synced": true, "lines": [{ "tMs": 500, "text": "Hello" }, { "tMs": 1500, "text": "World" }] }));
    assert_eq!(e.call("lyrics.get", json!({ "trackId": 999_999 }))["error"]["code"], -32001);
    let mut bare = Engine::spawn();
    assert_eq!(bare.call("lyrics.get", json!({ "trackId": 1 }))["error"]["code"], -32008);
}
