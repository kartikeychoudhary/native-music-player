//! Spectrum analyzer thread: reads the current stream's tap and sends `PlayerEvent::Spectrum` at 30 fps.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use super::PlayerEvent;
use crate::dsp::spectrum::{Analyzer, FFT_SIZE, FPS};

pub struct TapStream { pub tap: rtrb::Consumer<f32>, pub rate: u32 }

/// Runs until `rx` disconnects (the player shut down). Each new stream replaces the previous tap.
pub fn spawn(events: Sender<PlayerEvent>) -> Sender<TapStream> {
    let (tx, rx) = crossbeam_channel::unbounded::<TapStream>();
    std::thread::Builder::new().name("aurora-viz".into()).spawn(move || run(rx, events)).expect("spawn viz thread");
    tx
}

fn run(rx: Receiver<TapStream>, events: Sender<PlayerEvent>) {
    let period = Duration::from_secs(1) / FPS;
    let mut current: Option<(TapStream, Analyzer)> = None;
    let mut window: VecDeque<f32> = VecDeque::with_capacity(FFT_SIZE);
    let mut buf = vec![0f32; FFT_SIZE];
    let mut fresh = false;
    let mut next = Instant::now();
    loop {
        match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok(s) => { let a = Analyzer::new(s.rate); current = Some((s, a)); window.clear(); continue; }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        next += period;
        if next < Instant::now() { next = Instant::now() + period; }
        let Some((stream, analyzer)) = current.as_mut() else { continue };
        while let Ok(v) = stream.tap.pop() {
            if window.len() == FFT_SIZE { window.pop_front(); }
            window.push_back(v);
            fresh = true;
        }
        // Frames only while new audio arrives: paused or viz off means no frames.
        if !fresh || window.len() < FFT_SIZE { continue; }
        fresh = false;
        for (d, s) in buf.iter_mut().zip(&window) { *d = *s; }
        let _ = events.send(PlayerEvent::Spectrum(analyzer.analyze(&buf)));
    }
}
