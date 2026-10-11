use protocol::ErrorCode;

#[derive(Debug, thiserror::Error)]
pub enum LibError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("library database is corrupt")]
    Corrupt,
    #[error("library database is from a newer Aurora (schema {found}, this build supports {supported})")]
    NewerSchema { found: i64, supported: i64 },
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0}")]
    InvalidArgument(String),
    #[error("file not found: {0}")]
    FileMissing(String),
    #[error("scan cancelled")]
    Cancelled,
}

impl LibError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound(_) => ErrorCode::TrackNotFound,
            Self::InvalidArgument(_) => ErrorCode::InvalidArgument,
            Self::FileMissing(_) => ErrorCode::FileMissing,
            _ => ErrorCode::DbError,
        }
    }

    /// True for errors that mean the database file itself is damaged (not, e.g., locked).
    pub fn is_corruption(&self) -> bool {
        match self {
            Self::Corrupt => true,
            Self::Db(rusqlite::Error::SqliteFailure(f, _)) => matches!(f.code, rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase),
            _ => false,
        }
    }
}

pub type Result<T, E = LibError> = std::result::Result<T, E>;
