//! Exclusive-mode backend; implemented in a later task.
use super::format::OutputFormat;
use super::{DeviceCaps, OutputError, OutputStream, RingSource};

pub fn caps(_device: Option<&str>) -> Result<DeviceCaps, OutputError> { Err(OutputError::Other("not implemented yet".into())) }
pub fn open(_device: Option<&str>, _f: OutputFormat, _s: RingSource) -> Result<Box<dyn OutputStream>, OutputError> { Err(OutputError::Other("not implemented yet".into())) }
