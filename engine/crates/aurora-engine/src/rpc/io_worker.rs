//! Runs library calls that touch audio files (`lyrics.get`, `library.track`) off the stdin thread, so
//! a slow disk or network share can't hold up player commands. A reply is written when its job is
//! done; clients match responses by id, so it may overtake earlier requests' replies.

use std::thread::JoinHandle;

use crossbeam_channel::{unbounded, Sender};
use serde_json::Value;

const SLOW_METHODS: [&str; 2] = ["lyrics.get", "library.track"];

pub fn is_slow(method: &str) -> bool { SLOW_METHODS.contains(&method) }

pub struct Job { pub id: Value, pub method: String, pub params: Value }

pub struct IoWorker { tx: Option<Sender<Job>>, handle: Option<JoinHandle<()>> }

impl IoWorker {
    /// `handle` runs each job on the worker thread and writes its response.
    pub fn spawn(handle: impl Fn(Job) + Send + 'static) -> Self {
        let (tx, rx) = unbounded::<Job>();
        let h = std::thread::Builder::new().name("aurora-io".into()).spawn(move || { for job in rx { handle(job) } }).expect("spawn io worker");
        Self { tx: Some(tx), handle: Some(h) }
    }

    pub fn submit(&self, job: Job) { if let Some(tx) = &self.tx { let _ = tx.send(job); } }

    /// Lets queued jobs finish (their replies are still written), then stops the thread.
    pub fn shutdown(&mut self) {
        self.tx.take();
        if let Some(h) = self.handle.take() { let _ = h.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn only_file_touching_library_calls_are_slow() {
        assert!(is_slow("lyrics.get") && is_slow("library.track"));
        assert!(!is_slow("library.tracks") && !is_slow("player.state") && !is_slow("playlists.get"));
    }

    #[test]
    fn shutdown_finishes_queued_jobs() {
        let done = Arc::new(Mutex::new(Vec::new()));
        let d = done.clone();
        let mut w = IoWorker::spawn(move |j| d.lock().unwrap().push(j.id));
        for i in 0..3 { w.submit(Job { id: i.into(), method: "lyrics.get".into(), params: Value::Null }); }
        w.shutdown();
        assert_eq!(*done.lock().unwrap(), vec![Value::from(0), 1.into(), 2.into()]);
    }
}
