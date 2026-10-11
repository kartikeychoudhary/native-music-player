//! macOS exclusive-mode backend: takes CoreAudio hog mode on the device, pins its sample rate and
//! physical integer format, then plays an f32 cpal stream whose callback converts the ring's
//! integers to f32 exactly (<= 24-bit). With nothing else mixing, CoreAudio converts back losslessly.
use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::macos_helpers::{
    find_matching_physical_format, get_available_sample_rates, get_default_device_id, get_device_id_from_name,
    get_device_transport_type, get_hogging_pid, get_supported_physical_stream_formats, set_device_physical_stream_format,
    set_device_sample_rate, toggle_hog_mode,
};
use coreaudio::audio_unit::{SampleFormat as CaSample, StreamFormat};
use cpal::traits::DeviceTrait;
use protocol::{ShareMode, Transport};

use super::cpal_out::{self, CpalStream};
use super::format::{DeviceSample, OutputFormat};
use super::{DeviceCaps, OutputError, OutputStream, RingSource, StreamInfo, COMMON_RATES};

// kAudioDeviceTransportType* four-char codes.
const TRANSPORT_BLUETOOTH: u32 = u32::from_be_bytes(*b"blue");
const TRANSPORT_BLUETOOTH_LE: u32 = u32::from_be_bytes(*b"blea");
const TRANSPORT_USB: u32 = u32::from_be_bytes(*b"usb ");
const TRANSPORT_BUILTIN: u32 = u32::from_be_bytes(*b"bltn");

fn ca_err(e: impl std::fmt::Debug) -> OutputError { OutputError::DeviceUnavailable(format!("{e:?}")) }

fn resolve(id: Option<&str>) -> Result<(u32, String), OutputError> {
    let name = cpal_out::device_name(id).ok_or_else(|| OutputError::DeviceUnavailable(id.unwrap_or("default").into()))?;
    let dev = match id {
        None => get_default_device_id(false),
        Some(_) => get_device_id_from_name(&name, false),
    };
    dev.map(|d| (d, name)).ok_or_else(|| OutputError::DeviceUnavailable("CoreAudio device not found".into()))
}

fn transport(dev: u32) -> Transport {
    match get_device_transport_type(dev).unwrap_or(0) {
        TRANSPORT_BLUETOOTH | TRANSPORT_BLUETOOTH_LE => Transport::Bluetooth,
        TRANSPORT_USB => Transport::Usb,
        TRANSPORT_BUILTIN => Transport::Builtin,
        _ => Transport::Other,
    }
}

pub fn caps(id: Option<&str>) -> Result<DeviceCaps, OutputError> {
    let (dev, _) = resolve(id)?;
    let ranges = get_available_sample_rates(dev).map_err(ca_err)?;
    let rates = COMMON_RATES
        .into_iter()
        .filter(|&r| ranges.iter().any(|v| f64::from(r) >= v.mMinimum && f64::from(r) <= v.mMaximum))
        .collect::<Vec<_>>();
    // I32 is not offered: an f32 stream cannot carry 32-bit integers exactly.
    let mut samples = vec![DeviceSample::F32];
    let mut max_channels = 2u16;
    for f in get_supported_physical_stream_formats(dev).map_err(ca_err)? {
        max_channels = max_channels.max(f.mFormat.mChannelsPerFrame as u16);
        let s = match f.mFormat.mBitsPerChannel {
            16 => Some(DeviceSample::I16),
            24 => Some(DeviceSample::I24),
            _ => None,
        };
        if let Some(s) = s.filter(|s| !samples.contains(s)) { samples.push(s); }
    }
    if rates.is_empty() { return Err(OutputError::Other("no usable sample rates".into())); }
    Ok(DeviceCaps { rates, max_channels, samples, exclusive: true })
}

pub struct HogStream {
    inner: CpalStream,
    info: StreamInfo,
    dev: u32,
}

impl OutputStream for HogStream {
    fn info(&self) -> &StreamInfo { &self.info }
    fn start(&mut self) -> Result<(), OutputError> { self.inner.start() }
}

impl Drop for HogStream {
    fn drop(&mut self) {
        if get_hogging_pid(self.dev).ok() == Some(std::process::id() as i32) { let _ = toggle_hog_mode(self.dev); }
    }
}

pub fn open(id: Option<&str>, fmt: OutputFormat, source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
    let (dev, name) = resolve(id)?;
    let me = std::process::id() as i32;
    // `toggle_hog_mode` releases hog mode if this process already owns it, so only toggle when it is free.
    match get_hogging_pid(dev) {
        Ok(pid) if pid == me => {}
        Ok(pid) if pid != -1 => return Err(OutputError::ExclusiveDenied(format!("device hogged by process {pid}"))),
        _ => {
            if toggle_hog_mode(dev).map_err(ca_err)? != me { return Err(OutputError::ExclusiveDenied("hog mode refused".into())); }
        }
    }
    let release = || { let _ = toggle_hog_mode(dev); };
    if let Err(e) = set_device_sample_rate(dev, f64::from(fmt.sample_rate)) {
        release();
        return Err(OutputError::UnsupportedFormat(format!("device sample rate: {e:?}")));
    }
    let (ca_sample, flags) = match fmt.sample {
        DeviceSample::I16 => (CaSample::I16, LinearPcmFlags::IS_SIGNED_INTEGER | LinearPcmFlags::IS_PACKED),
        DeviceSample::I24 => (CaSample::I24, LinearPcmFlags::IS_SIGNED_INTEGER | LinearPcmFlags::IS_PACKED),
        DeviceSample::F32 | DeviceSample::I32 => (CaSample::F32, LinearPcmFlags::IS_FLOAT | LinearPcmFlags::IS_PACKED),
    };
    let wanted = StreamFormat { sample_rate: f64::from(fmt.sample_rate), sample_format: ca_sample, flags, channels: u32::from(fmt.channels) };
    if let Some(asbd) = find_matching_physical_format(dev, wanted) { let _ = set_device_physical_stream_format(dev, asbd); }
    let cpal_dev = match cpal_out::find_device(&cpal_out::host(), id) {
        Ok(d) => d,
        Err(e) => { release(); return Err(e); }
    };
    let float_fmt = OutputFormat { sample: DeviceSample::F32, ..fmt };
    let inner = match cpal_out::open_stream(&cpal_dev, float_fmt, fmt.sample, source, ShareMode::Exclusive) {
        Ok(s) => s,
        Err(e) => { release(); return Err(e); }
    };
    let device_id = cpal_dev.id().map(|i| i.to_string()).unwrap_or_default();
    Ok(Box::new(HogStream {
        inner,
        info: StreamInfo { device_id, device_name: name, transport: transport(dev), share: ShareMode::Exclusive, format: fmt },
        dev,
    }))
}
