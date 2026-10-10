use std::sync::atomic::Ordering::*;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, ErrorKind, InterfaceType, SampleFormat, StreamConfig};
use protocol::{BluetoothInfo, DeviceInfo, ShareMode, Transport};

use super::format::{DeviceSample, OutputFormat};
use super::{DeviceCaps, OutputBackend, OutputError, OutputStream, RingSource, StreamInfo, COMMON_RATES};

pub struct CpalBackend;

pub(crate) fn host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    if let Ok(h) = cpal::host_from_id(cpal::HostId::PipeWire) { return h; }
    cpal::default_host()
}

pub(crate) fn find_device(host: &cpal::Host, id: Option<&str>) -> Result<cpal::Device, OutputError> {
    match id {
        Some(id) => {
            let parsed: cpal::DeviceId = id.parse().map_err(|_| OutputError::DeviceUnavailable(id.into()))?;
            host.device_by_id(&parsed).ok_or_else(|| OutputError::DeviceUnavailable(id.into()))
        }
        None => host.default_output_device().ok_or_else(|| OutputError::DeviceUnavailable("no default output device".into())),
    }
}

/// Used by the exclusive-mode backends (later tasks) to name the device they open.
#[allow(dead_code)]
pub(crate) fn device_name(id: Option<&str>) -> Option<String> {
    find_device(&host(), id).ok()?.description().ok().map(|d| d.name().to_string())
}

pub(crate) fn transport_of(t: InterfaceType) -> Transport {
    match t {
        InterfaceType::Bluetooth => Transport::Bluetooth,
        InterfaceType::Usb => Transport::Usb,
        InterfaceType::BuiltIn => Transport::Builtin,
        InterfaceType::Hdmi | InterfaceType::DisplayPort => Transport::Hdmi,
        _ => Transport::Other,
    }
}

fn sample_of(f: SampleFormat) -> Option<DeviceSample> {
    match f { SampleFormat::I16 => Some(DeviceSample::I16), SampleFormat::I32 => Some(DeviceSample::I32), SampleFormat::F32 => Some(DeviceSample::F32), _ => None }
}

fn describe(dev: &cpal::Device) -> (String, String, Transport) {
    let id = dev.id().map(|i| i.to_string()).unwrap_or_default();
    let (name, transport) = dev.description().map(|d| (d.name().to_string(), transport_of(d.interface_type()))).unwrap_or_else(|_| (id.clone(), Transport::Other));
    (id, name, transport)
}

fn shared_caps(dev: &cpal::Device) -> Result<DeviceCaps, OutputError> {
    let configs: Vec<_> = dev.supported_output_configs().map_err(|e| OutputError::DeviceUnavailable(e.to_string()))?.collect();
    let mut caps = DeviceCaps { rates: vec![], max_channels: 0, samples: vec![], exclusive: false };
    for c in &configs {
        caps.max_channels = caps.max_channels.max(c.channels());
        if let Some(s) = sample_of(c.sample_format()).filter(|s| !caps.samples.contains(s)) { caps.samples.push(s); }
        for r in COMMON_RATES { if r >= c.min_sample_rate() && r <= c.max_sample_rate() && !caps.rates.contains(&r) { caps.rates.push(r); } }
    }
    caps.rates.sort_unstable();
    if caps.samples.is_empty() || caps.rates.is_empty() { return Err(OutputError::UnsupportedFormat("device reports no usable PCM configs".into())); }
    Ok(caps)
}

impl CpalBackend {
    pub(crate) fn list(&self, exclusive_capable: impl Fn(Transport) -> bool) -> Result<Vec<DeviceInfo>, OutputError> {
        let host = host();
        let default_id = host.default_output_device().and_then(|d| d.id().ok()).map(|i| i.to_string());
        let devices = host.output_devices().map_err(|e| OutputError::Other(e.to_string()))?;
        Ok(devices.map(|d| {
            let (id, name, transport) = describe(&d);
            let rates = shared_caps(&d).map(|c| c.rates).unwrap_or_default();
            DeviceInfo { is_default: Some(&id) == default_id.as_ref(), id, name, transport, sample_rates: rates, exclusive_capable: exclusive_capable(transport) }
        }).collect())
    }
}

pub struct CpalStream { stream: cpal::Stream, info: StreamInfo }

impl OutputStream for CpalStream {
    fn info(&self) -> &StreamInfo { &self.info }
    fn start(&mut self) -> Result<(), OutputError> { self.stream.play().map_err(|e| OutputError::Other(e.to_string())) }
}

/// Opens a shared-mode stream. When the device format is f32 but `ring_sample` is an integer
/// format (used by the macOS hog backend), the callback converts ring bytes to f32; otherwise
/// ring bytes are copied raw.
pub(crate) fn open_stream(dev: &cpal::Device, fmt: OutputFormat, ring_sample: DeviceSample, mut source: RingSource, share: ShareMode) -> Result<CpalStream, OutputError> {
    let (id, name, transport) = describe(dev);
    let config = StreamConfig { channels: fmt.channels, sample_rate: fmt.sample_rate, buffer_size: BufferSize::Default };
    let shared = source.shared().clone();
    let on_err = move |e: cpal::Error| {
        if matches!(e.kind(), ErrorKind::DeviceNotAvailable | ErrorKind::DeviceChanged) { shared.device_lost.store(true, Release); }
    };
    let stream = if fmt.sample == DeviceSample::F32 && ring_sample != DeviceSample::F32 {
        let bpf = ring_sample.bytes() * fmt.channels as usize;
        let mut scratch = vec![0u8; 65_536 * bpf];
        dev.build_output_stream_raw(config, SampleFormat::F32, move |data: &mut cpal::Data, _: &cpal::OutputCallbackInfo| {
            let Some(out) = data.as_slice_mut::<f32>() else { return };
            let need = (out.len() * ring_sample.bytes()).min(scratch.len());
            source.fill(&mut scratch[..need]);
            bytes_to_f32_slice(ring_sample, &scratch[..need], out);
        }, on_err, None)
    } else {
        let sf = match fmt.sample {
            DeviceSample::I16 => SampleFormat::I16,
            DeviceSample::I32 => SampleFormat::I32,
            DeviceSample::F32 => SampleFormat::F32,
            DeviceSample::I24 => return Err(OutputError::UnsupportedFormat("packed 24-bit isn't available in shared mode".into())),
        };
        dev.build_output_stream_raw(config, sf, move |data: &mut cpal::Data, _: &cpal::OutputCallbackInfo| { source.fill(data.bytes_mut()); }, on_err, None)
    }
    .map_err(|e| OutputError::UnsupportedFormat(e.to_string()))?;
    Ok(CpalStream { stream, info: StreamInfo { device_id: id, device_name: name, transport, share, format: fmt } })
}

fn bytes_to_f32_slice(s: DeviceSample, src: &[u8], dst: &mut [f32]) {
    let n = (src.len() / s.bytes()).min(dst.len());
    super::format::bytes_to_f32(s, &src[..n * s.bytes()], &mut dst[..n]);
    dst[n..].fill(0.0);
}

impl OutputBackend for CpalBackend {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError> { self.list(|_| false) }
    fn caps(&self, device: Option<&str>, exclusive: bool) -> Result<DeviceCaps, OutputError> {
        if exclusive { return Err(OutputError::Other("cpal backend is shared-mode only".into())); }
        shared_caps(&find_device(&host(), device)?)
    }
    fn open(&self, device: Option<&str>, format: OutputFormat, exclusive: bool, source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
        if exclusive { return Err(OutputError::Other("cpal backend is shared-mode only".into())); }
        Ok(Box::new(open_stream(&find_device(&host(), device)?, format, format.sample, source, ShareMode::Shared)?))
    }
    fn bluetooth(&self, stream: &StreamInfo) -> Option<BluetoothInfo> { crate::probe::bluetooth_for(stream) }
}
