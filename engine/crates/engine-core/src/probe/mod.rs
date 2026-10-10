pub mod pipewire;

use protocol::{BluetoothInfo, Transport};

use crate::output::StreamInfo;

/// Codec details for a Bluetooth output. `codec: None` means "not reported by the OS". Never guess.
pub fn bluetooth_for(stream: &StreamInfo) -> Option<BluetoothInfo> {
    if stream.transport != Transport::Bluetooth { return None; }
    #[cfg(target_os = "linux")]
    if let Some(info) = pipewire::query(&stream.device_name) { return Some(info); }
    Some(BluetoothInfo::default())
}
