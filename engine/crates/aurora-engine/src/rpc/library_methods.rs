//! `library.*` and `playlists.*` (spec §4.3).
use std::path::Path;

use library::LibError;
use protocol::rpc::*;
use protocol::{ErrorCode, GaplessSummary, LibraryScope, ListQuery, TrackDetail};
use serde::Serialize;
use serde_json::{json, Value};

use super::dispatch::params;
use super::library_service::LibraryService;

pub const UNAVAILABLE: &str = "library unavailable: engine started without --data-dir";

pub fn lib_err(e: LibError) -> RpcError {
    let c = e.code();
    RpcError { code: c.code(), message: e.to_string(), data: Some(json!({ "name": c.name() })) }
}

pub fn unavailable() -> RpcError {
    RpcError { code: ErrorCode::DbError.code(), message: UNAVAILABLE.into(), data: Some(json!({ "name": ErrorCode::DbError.name() })) }
}

fn reply<T: Serialize>(r: Result<T, LibError>) -> Result<Value, RpcError> {
    r.map(|v| serde_json::to_value(v).expect("protocol types serialize")).map_err(lib_err)
}

/// Encoder delay/padding for the track-info dialog, probed from the file itself.
fn with_gapless(mut d: TrackDetail) -> TrackDetail {
    if !d.track.missing && let Ok(dec) = engine_core::decode::open(Path::new(&d.track.path)) {
        d.gapless = dec.info().gapless.as_ref().map(|g| GaplessSummary { delay: g.delay, padding: g.padding });
    }
    d
}

/// Handles `library.*` and `playlists.*`; `None` for any other method.
pub fn dispatch(svc: Option<&LibraryService>, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !(method.starts_with("library.") || method.starts_with("playlists.")) { return None; }
    Some(match svc { Some(svc) => handle(svc, method, p), None => Err(unavailable()) })
}

fn handle(svc: &LibraryService, method: &str, p: Value) -> Result<Value, RpcError> {
    let lib = &svc.lib;
    let mutate = |r: Result<Value, RpcError>| {
        if r.is_ok() { svc.notify_changed(&[LibraryScope::Playlists]); }
        r
    };
    match method {
        "library.folders" => reply(lib.folders()),
        "library.addFolder" => { let p: PathParams = params(p)?; reply(svc.add_folder(Path::new(&p.path))) }
        "library.removeFolder" => { let p: IdParams = params(p)?; reply(svc.remove_folder(p.id)) }
        "library.rescan" => { let p: RescanParams = params(p)?; reply(svc.rescan(p.folder_id)) }
        "library.albums" => { let q: ListQuery = params(p)?; reply(lib.albums(&q)) }
        "library.artists" => { let q: ListQuery = params(p)?; reply(lib.artists(&q)) }
        "library.genres" => { let q: ListQuery = params(p)?; reply(lib.genres(&q)) }
        "library.tracks" => { let q: ListQuery = params(p)?; reply(lib.tracks(&q)) }
        "library.album" => { let p: IdParams = params(p)?; reply(lib.album(p.id)) }
        "library.artist" => { let p: IdParams = params(p)?; reply(lib.artist(p.id)) }
        "library.genre" => { let p: IdParams = params(p)?; reply(lib.genre(p.id)) }
        "library.track" => { let p: IdParams = params(p)?; reply(lib.track(p.id).map(with_gapless)) }
        "library.search" => { let p: SearchParams = params(p)?; reply(lib.search(&p.text, p.limit.unwrap_or(20))) }
        "library.artwork" => { let p: ArtworkParams = params(p)?; reply(lib.artwork_info(p.id, p.size)) }
        "library.recent" => { let p: RecentParams = params(p)?; reply(lib.recent(p.kind, p.limit.unwrap_or(20))) }
        "playlists.list" => reply(lib.playlists()),
        "playlists.get" => { let p: IdParams = params(p)?; reply(lib.playlist(p.id)) }
        "playlists.create" => { let p: NameParams = params(p)?; mutate(reply(lib.create_playlist(&p.name))) }
        "playlists.rename" => { let p: RenameParams = params(p)?; mutate(reply(lib.rename_playlist(p.id, &p.name))) }
        "playlists.delete" => { let p: IdParams = params(p)?; mutate(reply(lib.delete_playlist(p.id))) }
        "playlists.addTracks" => { let p: PlaylistAddParams = params(p)?; mutate(reply(lib.add_to_playlist(p.id, &p.track_ids, p.index))) }
        "playlists.removeItems" => { let p: PlaylistRemoveParams = params(p)?; mutate(reply(lib.remove_from_playlist(p.id, &p.item_ids))) }
        "playlists.reorder" => { let p: PlaylistReorderParams = params(p)?; mutate(reply(lib.reorder_playlist(p.id, p.from, p.to))) }
        "playlists.importM3u" => { let p: PathParams = params(p)?; mutate(reply(lib.import_m3u(Path::new(&p.path)))) }
        "playlists.exportM3u" => { let p: ExportParams = params(p)?; reply(lib.export_m3u(p.id, Path::new(&p.path))) }
        other => Err(RpcError { code: -32601, message: format!("method not found: {other}"), data: None }),
    }
}
