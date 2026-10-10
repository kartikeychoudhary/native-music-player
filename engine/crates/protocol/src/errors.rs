#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    TrackNotFound, FileMissing, UnsupportedFormat, CodecNotInBuild, DecodeFailed,
    DeviceUnavailable, ExclusiveDenied, DbError, InvalidArgument,
}

impl ErrorCode {
    pub fn code(self) -> i32 {
        match self {
            Self::TrackNotFound => -32001, Self::FileMissing => -32002, Self::UnsupportedFormat => -32003,
            Self::CodecNotInBuild => -32004, Self::DecodeFailed => -32005, Self::DeviceUnavailable => -32006,
            Self::ExclusiveDenied => -32007, Self::DbError => -32008, Self::InvalidArgument => -32009,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::TrackNotFound => "TRACK_NOT_FOUND", Self::FileMissing => "FILE_MISSING",
            Self::UnsupportedFormat => "UNSUPPORTED_FORMAT", Self::CodecNotInBuild => "CODEC_NOT_IN_BUILD",
            Self::DecodeFailed => "DECODE_FAILED", Self::DeviceUnavailable => "DEVICE_UNAVAILABLE",
            Self::ExclusiveDenied => "EXCLUSIVE_DENIED", Self::DbError => "DB_ERROR",
            Self::InvalidArgument => "INVALID_ARGUMENT",
        }
    }
}
