//! Linux exclusive-mode backend: opens the ALSA `hw:` device directly (no mixing, no resampling).
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};
use protocol::{ShareMode, Transport};

pub use super::card_match::card_for;
use super::format::{DeviceSample, OutputFormat};
use super::{cpal_out, DeviceCaps, OutputError, OutputStream, RingSource, StreamInfo, COMMON_RATES};

/// `EBUSY` is 16 on every Linux architecture.
const EBUSY: i32 = 16;

fn cards() -> Vec<(i32, String, String)> {
    alsa::card::Iter::new()
        .filter_map(Result::ok)
        .map(|c| (c.get_index(), c.get_name().unwrap_or_default(), c.get_longname().unwrap_or_default()))
        .collect()
}

fn hw_name(id: Option<&str>) -> Result<(String, String), OutputError> {
    let name = cpal_out::device_name(id).ok_or_else(|| OutputError::DeviceUnavailable("device not found".into()))?;
    let card = card_for(&name, &cards()).ok_or_else(|| OutputError::Other(format!("no ALSA card matches \"{name}\"")))?;
    Ok((format!("hw:{card},0"), name))
}

fn open_pcm(hw: &str) -> Result<PCM, OutputError> {
    PCM::new(hw, Direction::Playback, false).map_err(|e| {
        if e.errno() == EBUSY {
            OutputError::ExclusiveDenied("device busy (held by PipeWire or another app)".into())
        } else {
            OutputError::DeviceUnavailable(e.to_string())
        }
    })
}

fn alsa_format(s: DeviceSample) -> Option<Format> {
    match s {
        DeviceSample::I16 => Some(Format::s16()),
        DeviceSample::I24 => Some(Format::s24_3()),
        DeviceSample::I32 => Some(Format::s32()),
        DeviceSample::F32 => None,
    }
}

pub fn caps(id: Option<&str>) -> Result<DeviceCaps, OutputError> {
    let (hw, _) = hw_name(id)?;
    let pcm = open_pcm(&hw)?;
    let hwp = HwParams::any(&pcm).map_err(|e| OutputError::Other(e.to_string()))?;
    let _ = hwp.set_rate_resample(false);
    let rates = COMMON_RATES.into_iter().filter(|&r| hwp.test_rate(r).is_ok()).collect::<Vec<_>>();
    let samples = [DeviceSample::I16, DeviceSample::I24, DeviceSample::I32]
        .into_iter()
        .filter(|&s| alsa_format(s).is_some_and(|f| hwp.test_format(f).is_ok()))
        .collect::<Vec<_>>();
    let max_channels = hwp.get_channels_max().unwrap_or(2).min(16) as u16;
    if rates.is_empty() || samples.is_empty() {
        return Err(OutputError::Other("no usable hw formats".into()));
    }
    Ok(DeviceCaps { rates, max_channels, samples, exclusive: true })
}

pub struct AlsaStream {
    info: StreamInfo,
    alive: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl OutputStream for AlsaStream {
    fn info(&self) -> &StreamInfo { &self.info }
    fn start(&mut self) -> Result<(), OutputError> { Ok(()) }
}

impl Drop for AlsaStream {
    fn drop(&mut self) {
        self.alive.store(false, Release);
        if let Some(j) = self.join.take() { let _ = j.join(); }
    }
}

/// Opens `hw` and applies the exact format; returns the PCM and its period size in frames.
fn setup(hw: &str, format: Format, fmt: OutputFormat) -> Result<(PCM, usize), OutputError> {
    let pcm = open_pcm(hw)?;
    {
        let hwp = HwParams::any(&pcm).map_err(|e| OutputError::Other(e.to_string()))?;
        let err = |e: alsa::Error| OutputError::UnsupportedFormat(e.to_string());
        hwp.set_rate_resample(false).map_err(err)?;
        hwp.set_access(Access::RWInterleaved).map_err(err)?;
        hwp.set_format(format).map_err(err)?;
        hwp.set_channels(u32::from(fmt.channels)).map_err(err)?;
        hwp.set_rate(fmt.sample_rate, ValueOr::Nearest).map_err(err)?;
        hwp.set_buffer_time_near(200_000, ValueOr::Nearest).map_err(err)?;
        pcm.hw_params(&hwp).map_err(err)?;
    }
    // `HwParams` has a destructor that borrows `pcm`; scope it so `pcm` can be moved out below.
    let period = {
        let cur = pcm.hw_params_current().map_err(|e| OutputError::Other(e.to_string()))?;
        if cur.get_rate().ok() != Some(fmt.sample_rate) {
            return Err(OutputError::UnsupportedFormat("device refused exact rate".into()));
        }
        cur.get_period_size().map_err(|e| OutputError::Other(e.to_string()))? as usize
    };
    Ok((pcm, period))
}

pub fn open(id: Option<&str>, fmt: OutputFormat, mut source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
    let (hw, name) = hw_name(id)?;
    let format = alsa_format(fmt.sample).ok_or_else(|| OutputError::UnsupportedFormat("float isn't used for ALSA hw".into()))?;
    let alive = Arc::new(AtomicBool::new(true));
    let alive2 = alive.clone();
    let (tx, rx) = mpsc::channel::<Result<(), OutputError>>();
    let bpf = fmt.bytes_per_frame();
    let join = std::thread::Builder::new()
        .name("aurora-alsa".into())
        .spawn(move || {
            let (pcm, period) = match setup(&hw, format, fmt) {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            let _ = tx.send(Ok(()));
            let io = pcm.io_bytes();
            let mut buf = vec![0u8; period * bpf];
            'play: while alive2.load(Acquire) {
                source.fill(&mut buf);
                // A blocking `writei` can accept fewer frames than offered, so write the whole period.
                let mut off = 0;
                while off < buf.len() {
                    match io.writei(&buf[off..]) {
                        Ok(frames) => off += frames * bpf,
                        Err(e) => {
                            if pcm.try_recover(e, true).is_err() {
                                source.shared().device_lost.store(true, Release);
                                break 'play;
                            }
                        }
                    }
                    if !alive2.load(Acquire) { break 'play; }
                }
            }
            // Play out buffered audio only at the natural end of the queue; on stop, seek, device switch or
            // track change it is stale, so discard it instead of blocking the player for ~200 ms.
            if source.shared().draining.load(Acquire) { let _ = pcm.drain(); } else { let _ = pcm.drop(); }
        })
        .map_err(|e| OutputError::Other(e.to_string()))?;
    match rx.recv() {
        Ok(Ok(())) => Ok(Box::new(AlsaStream {
            info: StreamInfo {
                device_id: id.unwrap_or("default").into(),
                device_name: name,
                transport: cpal_out::transport(id).unwrap_or(Transport::Other),
                share: ShareMode::Exclusive,
                format: fmt,
            },
            alive,
            join: Some(join),
        })),
        Ok(Err(e)) => {
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            let _ = join.join();
            Err(OutputError::Other("ALSA thread exited during setup".into()))
        }
    }
}
