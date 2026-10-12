//! Wire types shared by the engine and the Electron app. No logic lives here.
pub mod dsp;
pub mod errors;
pub mod library;
pub mod lyrics;
pub mod rpc;
pub mod types;

pub use dsp::*;
pub use errors::ErrorCode;
pub use library::*;
pub use lyrics::*;
pub use types::*;

pub const PROTOCOL_VERSION: u32 = 3;

/// Binary frames interleaved with JSON lines on the engine's stdout (spec §4.1): `FRAME_MARK`, a
/// u32 little-endian payload length, then the payload, whose first byte is the frame kind.
pub mod frames {
    pub const FRAME_MARK: u8 = 0x1E;
    /// Payload: `[FRAME_SPECTRUM, bin0 … bin63]`, each bin 0–255 for −80…0 dBFS.
    pub const FRAME_SPECTRUM: u8 = 1;
    pub const SPECTRUM_BINS: usize = 64;
}
