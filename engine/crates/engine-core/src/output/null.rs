//! Test/headless output. Manual mode: the test drives the "callback" with `pull`.
//! Realtime mode: a thread pulls at (speed x real time). Both record every byte taken from the ring.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use protocol::{BluetoothInfo, DeviceInfo, ShareMode, Transport};

use super::format::{DeviceSample, OutputFormat};
use super::{DeviceCaps, OutputBackend, OutputError, OutputStream, RingSource, StreamInfo};

#[derive(Clone, Copy, Debug)]
pub enum NullMode { Manual, Realtime { speed: f64 } }

struct Inner {
    mode: NullMode,
    caps: Mutex<DeviceCaps>,
    transport: Mutex<Transport>,
    bluetooth: Mutex<Option<BluetoothInfo>>,
    recorded: Mutex<Vec<u8>>,
    opened: Mutex<Vec<(OutputFormat, bool)>>,
    source: Mutex<Option<RingSource>>,
    fail_exclusive: AtomicBool,
    /// Incremented per `open`; a stream only clears `source` if it is still the latest.
    generation: AtomicU64,
}

pub struct NullBackend { inner: Arc<Inner> }
#[derive(Clone)]
pub struct NullHandle { inner: Arc<Inner> }

impl NullBackend {
    pub fn new(mode: NullMode) -> (NullBackend, NullHandle) {
        let inner = Arc::new(Inner {
            mode,
            caps: Mutex::new(DeviceCaps {
                rates: vec![44_100, 48_000, 88_200, 96_000, 176_400, 192_000],
                max_channels: 2,
                samples: vec![DeviceSample::I16, DeviceSample::I24, DeviceSample::I32, DeviceSample::F32],
                exclusive: true,
            }),
            transport: Mutex::new(Transport::Other),
            bluetooth: Mutex::new(None),
            recorded: Mutex::new(Vec::new()),
            opened: Mutex::new(Vec::new()),
            source: Mutex::new(None),
            fail_exclusive: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        });
        (NullBackend { inner: inner.clone() }, NullHandle { inner })
    }
}

impl NullHandle {
    pub fn pull(&self, frames: usize) -> usize {
        let mut guard = self.inner.source.lock().unwrap();
        let Some(src) = guard.as_mut() else { return 0 };
        let bpf = src.bytes_per_frame();
        let mut buf = vec![0u8; frames * bpf];
        let n = src.fill(&mut buf);
        self.inner.recorded.lock().unwrap().extend_from_slice(&buf[..n]);
        n / bpf
    }
    pub fn recorded(&self) -> Vec<u8> { self.inner.recorded.lock().unwrap().clone() }
    pub fn clear_recorded(&self) { self.inner.recorded.lock().unwrap().clear(); }
    pub fn opened(&self) -> Vec<(OutputFormat, bool)> { self.inner.opened.lock().unwrap().clone() }
    pub fn set_caps(&self, c: DeviceCaps) { *self.inner.caps.lock().unwrap() = c; }
    pub fn set_transport(&self, t: Transport) { *self.inner.transport.lock().unwrap() = t; }
    pub fn set_bluetooth(&self, b: Option<BluetoothInfo>) { *self.inner.bluetooth.lock().unwrap() = b; }
    pub fn set_fail_exclusive(&self, on: bool) { self.inner.fail_exclusive.store(on, Relaxed); }
    pub fn simulate_device_lost(&self) {
        if let Some(src) = self.inner.source.lock().unwrap().as_ref() { src.shared().device_lost.store(true, Release); }
    }
}

pub struct NullStream { info: StreamInfo, alive: Arc<AtomicBool>, inner: Arc<Inner>, generation: u64 }

impl OutputStream for NullStream {
    fn info(&self) -> &StreamInfo { &self.info }
    fn start(&mut self) -> Result<(), OutputError> {
        if let NullMode::Realtime { speed } = self.inner.mode {
            let (alive, h, rate) = (self.alive.clone(), NullHandle { inner: self.inner.clone() }, self.info.format.sample_rate);
            std::thread::Builder::new().name("aurora-null-out".into()).spawn(move || {
                let per_tick = ((rate as f64 * 0.01 * speed) as usize).max(1);
                while alive.load(Relaxed) { h.pull(per_tick); std::thread::sleep(Duration::from_millis(10)); }
            }).map_err(|e| OutputError::Other(e.to_string()))?;
        }
        Ok(())
    }
}

impl Drop for NullStream {
    fn drop(&mut self) {
        self.alive.store(false, Relaxed);
        let mut source = self.inner.source.lock().unwrap();
        if self.inner.generation.load(Acquire) == self.generation { *source = None; }
    }
}

impl OutputBackend for NullBackend {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError> {
        let caps = self.inner.caps.lock().unwrap().clone();
        Ok(vec![DeviceInfo {
            id: "null".into(), name: "Null Output".into(), transport: *self.inner.transport.lock().unwrap(),
            is_default: true, sample_rates: caps.rates, exclusive_capable: caps.exclusive,
        }])
    }

    fn caps(&self, _device: Option<&str>, exclusive: bool) -> Result<DeviceCaps, OutputError> {
        let caps = self.inner.caps.lock().unwrap().clone();
        if exclusive && !caps.exclusive { return Err(OutputError::Other("exclusive mode not available".into())); }
        Ok(caps)
    }

    fn open(&self, _device: Option<&str>, format: OutputFormat, exclusive: bool, source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
        if exclusive && self.inner.fail_exclusive.load(Relaxed) { return Err(OutputError::ExclusiveDenied("device busy".into())); }
        self.inner.opened.lock().unwrap().push((format, exclusive));
        let mut slot = self.inner.source.lock().unwrap();
        let generation = self.inner.generation.fetch_add(1, AcqRel) + 1;
        *slot = Some(source);
        drop(slot);
        Ok(Box::new(NullStream {
            info: StreamInfo {
                device_id: "null".into(), device_name: "Null Output".into(), transport: *self.inner.transport.lock().unwrap(),
                share: if exclusive { ShareMode::Exclusive } else { ShareMode::Shared }, format,
            },
            alive: Arc::new(AtomicBool::new(true)),
            inner: self.inner.clone(),
            generation,
        }))
    }

    fn bluetooth(&self, stream: &StreamInfo) -> Option<BluetoothInfo> {
        (stream.transport == Transport::Bluetooth).then(|| self.inner.bluetooth.lock().unwrap().clone().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::ring::ring_pair;

    #[test]
    fn manual_pull_records_ring_bytes_and_tracks_opens() {
        let (b, h) = NullBackend::new(NullMode::Manual);
        let f = OutputFormat { sample_rate: 44_100, channels: 2, sample: DeviceSample::I16 };
        let (mut p, src, sh) = ring_pair(64, f);
        let mut s = b.open(None, f, true, src).unwrap();
        s.start().unwrap();
        sh.playing.store(true, Relaxed);
        let mut c = p.write_chunk(8).unwrap();
        c.as_mut_slices().0.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        c.commit_all();
        assert_eq!(h.pull(4), 2);
        assert_eq!(h.recorded(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(h.opened(), vec![(f, true)]);
    }

    #[test]
    fn exclusive_denial_can_be_simulated() {
        let (b, h) = NullBackend::new(NullMode::Manual);
        h.set_fail_exclusive(true);
        let f = OutputFormat { sample_rate: 44_100, channels: 2, sample: DeviceSample::I16 };
        let (_p, src, _) = ring_pair(64, f);
        assert!(matches!(b.open(None, f, true, src), Err(OutputError::ExclusiveDenied(_))));
    }

    #[test]
    fn dropping_a_replaced_stream_keeps_the_new_source() {
        let (b, h) = NullBackend::new(NullMode::Manual);
        let f = OutputFormat { sample_rate: 44_100, channels: 2, sample: DeviceSample::I16 };
        let (_pa, src_a, _sha) = ring_pair(64, f);
        let a = b.open(None, f, false, src_a).unwrap();
        let (mut pb, src_b, shb) = ring_pair(64, f);
        let _b = b.open(None, f, false, src_b).unwrap();
        drop(a);
        shb.playing.store(true, Relaxed);
        let mut c = pb.write_chunk(4).unwrap();
        c.as_mut_slices().0.copy_from_slice(&[9, 8, 7, 6]);
        c.commit_all();
        assert_eq!(h.pull(1), 1);
        assert_eq!(h.recorded(), vec![9, 8, 7, 6]);
    }
}
