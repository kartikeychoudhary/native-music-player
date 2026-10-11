use std::sync::mpsc;
use std::time::Duration;

use library::watch::Watcher;

#[test]
fn reports_created_files_after_the_debounce() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let mut w = Watcher::with_timeout(Duration::from_millis(200), move |paths| { let _ = tx.send(paths); }).unwrap();
    w.watch(dir.path()).unwrap();
    std::thread::sleep(Duration::from_millis(300)); // let the OS watch settle (FSEvents/inotify)
    std::fs::create_dir(dir.path().join("Album")).unwrap();
    std::fs::write(dir.path().join("Album").join("new.flac"), b"x").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut seen = vec![];
    while std::time::Instant::now() < deadline {
        if let Ok(batch) = rx.recv_timeout(Duration::from_millis(500)) { seen.extend(batch); }
        if seen.iter().any(|p: &std::path::PathBuf| p.file_name().is_some_and(|n| n == "new.flac" || n == "Album")) { break; }
    }
    assert!(seen.iter().any(|p| p.file_name().is_some_and(|n| n == "new.flac" || n == "Album")), "saw {seen:?}");
}

#[test]
fn unwatched_folders_are_silent() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let mut w = Watcher::with_timeout(Duration::from_millis(100), move |paths| { let _ = tx.send(paths); }).unwrap();
    w.watch(dir.path()).unwrap();
    w.unwatch(dir.path()).unwrap();
    std::fs::write(dir.path().join("x.flac"), b"x").unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(1500)).is_err());
}
