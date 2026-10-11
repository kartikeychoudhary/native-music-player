//! Wire types shared by the engine and the Electron app. No logic lives here.
pub mod errors;
pub mod library;
pub mod rpc;
pub mod types;

pub use errors::ErrorCode;
pub use library::*;
pub use types::*;

pub const PROTOCOL_VERSION: u32 = 2;
