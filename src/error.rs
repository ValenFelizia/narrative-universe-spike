use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Validation {
        reason: ValidationReason,
        field: &'static str,
        id: Option<String>,
    },
    UnsupportedVersion {
        found: i32,
    },
    Busy,
    Storage {
        message: String,
    },
    Io {
        message: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationReason {
    InvalidId,
    PathExists,
    PathMissing,
    NotADatabase,
    MissingMetadata,
    ForeignDatabase,
    NotFound,
    DuplicateId,
    UnknownSubject,
    UnknownEntity,
    UnknownTarget,
    UnknownDefinition,
    UnknownType,
    UnknownPage,
    NotApplicable,
    ValueMismatch,
    ArchivedDefinition,
}

impl Error {
    pub(crate) fn validation(
        reason: ValidationReason,
        field: &'static str,
        id: Option<&str>,
    ) -> Self {
        Self::Validation {
            reason,
            field,
            id: id.map(str::to_string),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation { reason, field, id } => match id {
                Some(id) => write!(formatter, "validation {reason:?} on {field} for {id}"),
                None => write!(formatter, "validation {reason:?} on {field}"),
            },
            Self::UnsupportedVersion { found } => {
                write!(formatter, "unsupported schema version {found}")
            }
            Self::Busy => formatter.write_str("database is busy"),
            Self::Storage { message } => write!(formatter, "storage: {message}"),
            Self::Io { message } => write!(formatter, "io: {message}"),
        }
    }
}

impl std::error::Error for Error {}
