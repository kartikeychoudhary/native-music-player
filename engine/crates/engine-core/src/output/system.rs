use protocol::{BluetoothInfo, DeviceInfo, Transport};

use super::cpal_out::CpalBackend;
use super::format::OutputFormat;
use super::{DeviceCaps, OutputBackend, OutputError, OutputStream, RingSource, StreamInfo};

pub struct SystemBackend { shared: CpalBackend }

impl SystemBackend {
    pub fn new() -> Self { Self { shared: CpalBackend } }
}

impl Default for SystemBackend { fn default() -> Self { Self::new() } }

// One implementation per OS keeps every build free of unreachable code.
#[cfg(windows)]
fn exclusive_caps(_: &SystemBackend, d: Option<&str>) -> Result<DeviceCaps, OutputError> { super::wasapi_excl::caps(d) }
#[cfg(windows)]
fn exclusive_open(d: Option<&str>, f: OutputFormat, s: RingSource) -> Result<Box<dyn OutputStream>, OutputError> { super::wasapi_excl::open(d, f, s) }

#[cfg(target_os = "macos")]
fn exclusive_caps(_: &SystemBackend, d: Option<&str>) -> Result<DeviceCaps, OutputError> { super::coreaudio_hog::caps(d) }
#[cfg(target_os = "macos")]
fn exclusive_open(d: Option<&str>, f: OutputFormat, s: RingSource) -> Result<Box<dyn OutputStream>, OutputError> { super::coreaudio_hog::open(d, f, s) }

#[cfg(target_os = "linux")]
fn exclusive_caps(b: &SystemBackend, d: Option<&str>) -> Result<DeviceCaps, OutputError> {
    let transport = b.devices().ok().and_then(|ds| ds.into_iter().find(|x| match d { Some(id) => x.id == id, None => x.is_default })).map_or(Transport::Other, |x| x.transport);
    if transport == Transport::Bluetooth { return Err(OutputError::Other("Bluetooth goes through PipeWire".into())); }
    super::alsa_hw::caps(d)
}
#[cfg(target_os = "linux")]
fn exclusive_open(d: Option<&str>, f: OutputFormat, s: RingSource) -> Result<Box<dyn OutputStream>, OutputError> { super::alsa_hw::open(d, f, s) }

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn exclusive_caps(_: &SystemBackend, _: Option<&str>) -> Result<DeviceCaps, OutputError> { Err(OutputError::Other("exclusive mode not supported on this OS".into())) }
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn exclusive_open(_: Option<&str>, _: OutputFormat, _: RingSource) -> Result<Box<dyn OutputStream>, OutputError> { Err(OutputError::Other("exclusive mode not supported on this OS".into())) }

impl OutputBackend for SystemBackend {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError> {
        self.shared.list(|t| if cfg!(target_os = "linux") { t != Transport::Bluetooth } else { cfg!(any(windows, target_os = "macos")) })
    }

    fn caps(&self, device: Option<&str>, exclusive: bool) -> Result<DeviceCaps, OutputError> {
        if exclusive { exclusive_caps(self, device) } else { self.shared.caps(device, false) }
    }

    fn open(&self, device: Option<&str>, format: OutputFormat, exclusive: bool, source: RingSource) -> Result<Box<dyn OutputStream>, OutputError> {
        if exclusive { exclusive_open(device, format, source) } else { self.shared.open(device, format, false, source) }
    }

    fn bluetooth(&self, stream: &StreamInfo) -> Option<BluetoothInfo> { crate::probe::bluetooth_for(stream) }
}
