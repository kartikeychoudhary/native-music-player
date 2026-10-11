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
