//! WASAPI exclusive-mode output, event-driven. All COM objects live on one render thread.
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use protocol::{ShareMode, Transport};
use wasapi::{calculate_period_100ns, initialize_mta, DeviceEnumerator, Direction, SampleType, StreamMode, WasapiError, WaveFormat};
use windows::Win32::Media::Audio::{AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED, AUDCLNT_E_UNSUPPORTED_FORMAT};

use super::format::{DeviceSample, OutputFormat};
use super::{cpal_out, DeviceCaps, OutputError, OutputStream, RingSource, StreamInfo, COMMON_RATES};

fn map_err(e: WasapiError) -> OutputError {
    if let WasapiError::Windows(w) = &e {
        let c = w.code();
        if c == AUDCLNT_E_DEVICE_IN_USE { return OutputError::ExclusiveDenied("device in use by another application".into()); }
        if c == AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED { return OutputError::ExclusiveDenied("exclusive mode disabled in Windows sound settings".into()); }
        if c == AUDCLNT_E_UNSUPPORTED_FORMAT { return OutputError::UnsupportedFormat("format rejected by device".into()); }
    }
    OutputError::DeviceUnavailable(e.to_string())
}

fn device(id: Option<&str>) -> Result<wasapi::Device, OutputError> {
    let _ = initialize_mta();
    let en = DeviceEnumerator::new().map_err(map_err)?;
    match id {
        None => en.get_default_device(&Direction::Render).map_err(map_err),
        Some(id) => en
            .get_device(id.strip_prefix("wasapi:").unwrap_or(id))
            .or_else(|_| {
                let name = cpal_out::device_name(Some(id)).ok_or_else(|| WasapiError::DeviceNotFound(id.into()))?;
                en.get_device_collection(&Direction::Render)?.get_device_with_name(&name)
            })
            .map_err(map_err),
    }
}

/// Candidate wave formats for a device sample type, most preferred first.
fn wave_formats(s: DeviceSample, rate: u32, ch: u16) -> Vec<WaveFormat> {
    let (r, c) = (rate as usize, ch as usize);
    match s {
        DeviceSample::I16 => vec![WaveFormat::new(16, 16, &SampleType::Int, r, c, None)],
        DeviceSample::I24 => vec![WaveFormat::new(24, 24, &SampleType::Int, r, c, None)],
        DeviceSample::I32 => vec![WaveFormat::new(32, 32, &SampleType::Int, r, c, None), WaveFormat::new(32, 24, &SampleType::Int, r, c, None)],
        DeviceSample::F32 => vec![WaveFormat::new(32, 32, &SampleType::Float, r, c, None)],
    }
}

fn same(a: &WaveFormat, b: &WaveFormat) -> bool {
    a.get_samplespersec() == b.get_samplespersec()
        && a.get_bitspersample() == b.get_bitspersample()
        && a.get_validbitspersample() == b.get_validbitspersample()
        && a.get_nchannels() == b.get_nchannels()
}

pub fn caps(id: Option<&str>) -> Result<DeviceCaps, OutputError> {
    let dev = device(id)?;
    let client = dev.get_iaudioclient().map_err(map_err)?;
    let channels = dev.get_device_format().map(|f| f.get_nchannels()).unwrap_or(2).max(2);
    let mut caps = DeviceCaps { rates: vec![], max_channels: channels, samples: vec![], exclusive: false };
    for rate in COMMON_RATES {
        for s in [DeviceSample::I16, DeviceSample::I24, DeviceSample::I32, DeviceSample::F32] {
            let ok = wave_formats(s, rate, 2).iter().any(|wf| client.is_supported_exclusive_with_quirks(wf).is_ok_and(|got| same(&got, wf)));
            if ok {
                if !caps.rates.contains(&rate) { caps.rates.push(rate); }
                if !caps.samples.contains(&s) { caps.samples.push(s); }
            }
        }
    }
    caps.exclusive = !caps.rates.is_empty();
    if !caps.exclusive { return Err(OutputError::Other("device accepts no exclusive formats".into())); }
    Ok(caps)
}

/// Transport as reported by cpal (it reads the PnP enumerator, so Bluetooth is detected reliably);
/// the friendly-name check only covers devices cpal cannot resolve.
fn resolve_transport(from_cpal: Option<Transport>, friendly_name: &str) -> Transport {
    from_cpal.unwrap_or_else(|| if friendly_name.to_ascii_lowercase().contains("bluetooth") { Transport::Bluetooth } else { Transport::Other })
}

pub struct WasapiStream { info: StreamInfo, alive: Arc<AtomicBool>, join: Option<JoinHandle<()>> }

impl OutputStream for WasapiStream {
    fn info(&self) -> &StreamInfo { &self.info }
    fn start(&mut self) -> Result<(), OutputError> { Ok(()) } // starts as soon as it is initialized
}

impl Drop for WasapiStream {
    fn drop(&mut self) {
        self.alive.store(false, Release);
        if let Some(j) = self.join.take() { let _ = j.join(); }
    }
}

pub fn open(id: Option<&str>, fmt: OutputFormat, mut source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
    let id_owned = id.map(str::to_owned);
    let alive = Arc::new(AtomicBool::new(true));
    let (init_tx, init_rx) = mpsc::channel::<Result<(String, String), OutputError>>();
    let alive2 = alive.clone();
    let join = std::thread::Builder::new()
        .name("aurora-wasapi".into())
        .spawn(move || {
            let setup = (|| -> Result<_, OutputError> {
                let dev = device(id_owned.as_deref())?;
                let name = dev.get_friendlyname().unwrap_or_default();
                let dev_id = dev.get_id().unwrap_or_default();
                let mut last_err = OutputError::UnsupportedFormat("no matching wave format".into());
                for wf in wave_formats(fmt.sample, fmt.sample_rate, fmt.channels) {
                    let mut client = dev.get_iaudioclient().map_err(map_err)?;
                    let (def, _min) = client.get_device_period().map_err(map_err)?;
                    let period = client.calculate_aligned_period_near(def, Some(128), &wf).map_err(map_err)?;
                    let mut result = client.initialize_client(&wf, &Direction::Render, &StreamMode::EventsExclusive { period_hns: period });
                    if let Err(WasapiError::Windows(w)) = &result
                        && w.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED
                    {
                        let frames = client.get_buffer_size().map_err(map_err)?;
                        let aligned = calculate_period_100ns(frames as i64, fmt.sample_rate as i64);
                        client = dev.get_iaudioclient().map_err(map_err)?;
                        result = client.initialize_client(&wf, &Direction::Render, &StreamMode::EventsExclusive { period_hns: aligned });
                    }
                    match result {
                        Ok(()) => return Ok((client, wf.get_blockalign() as usize, name, dev_id)),
                        Err(e) => last_err = map_err(e),
                    }
                }
                Err(last_err)
            })();
            let (client, bpf, name, dev_id) = match setup {
                Ok(v) => v,
                Err(e) => {
                    let _ = init_tx.send(Err(e));
                    return;
                }
            };
            let run = (|| -> Result<(), WasapiError> {
                let event = client.set_get_eventhandle()?;
                let render = client.get_audiorenderclient()?;
                let max_frames = client.get_buffer_size()? as usize;
                let mut buf = vec![0u8; max_frames * bpf];
                let frames = client.get_available_space_in_frames()? as usize;
                source.fill(&mut buf[..frames * bpf]);
                render.write_to_device(frames, &buf[..frames * bpf], None)?;
                client.start_stream()?;
                let _ = init_tx.send(Ok((name, dev_id)));
                while alive2.load(Acquire) {
                    if event.wait_for_event(1000).is_err() {
                        source.shared().device_lost.store(true, Release);
                        break;
                    }
                    let frames = client.get_available_space_in_frames()? as usize;
                    if frames == 0 { continue; }
                    source.fill(&mut buf[..frames * bpf]);
                    render.write_to_device(frames, &buf[..frames * bpf], None)?;
                }
                client.stop_stream()?;
                Ok(())
            })();
            if run.is_err() { source.shared().device_lost.store(true, Release); }
        })
        .map_err(|e| OutputError::Other(e.to_string()))?;

    match init_rx.recv() {
        Ok(Ok((name, dev_id))) => {
            let device_id = id.map(str::to_owned).unwrap_or_else(|| format!("wasapi:{dev_id}"));
            let transport = resolve_transport(cpal_out::transport(Some(&device_id)), &name);
            Ok(Box::new(WasapiStream {
                info: StreamInfo { device_id, device_name: name, transport, share: ShareMode::Exclusive, format: fmt },
                alive,
                join: Some(join),
            }))
        }
        Ok(Err(e)) => {
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            let _ = join.join();
            Err(OutputError::Other("render thread exited during setup".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpal_transport_wins_over_the_name() {
        assert_eq!(resolve_transport(Some(Transport::Bluetooth), "Headphones (WH-1000XM4)"), Transport::Bluetooth);
        assert_eq!(resolve_transport(Some(Transport::Usb), "x"), Transport::Usb);
    }

    #[test]
    fn name_heuristic_is_the_fallback() {
        assert_eq!(resolve_transport(None, "Bluetooth Speaker"), Transport::Bluetooth);
        assert_eq!(resolve_transport(None, "Speakers (Realtek)"), Transport::Other);
    }
}
