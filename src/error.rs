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
    ImportConflict {
        class: &'static str,
        id: String,
    },
    StaleRevision {
        page_id: String,
        expected: i64,
        found: i64,
    },
    MigrationRequired {
        found: i32,
    },
    MentionsNotExported,
    WrongUniverse {
        expected: String,
        found: String,
    },
    InvalidDocument {
        message: String,
    },
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
    InputTooLarge,
    InvalidTextRange,
    InvalidMentionRange,
    SplitScalar,
    SplitGrapheme,
    OverlappingMentions,
    DuplicateMentionId,
    MentionIdConflict,
    CoordinateOverflow,
    InvalidRevision,
    RevisionExhausted,
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
            Self::ImportConflict { class, id } => {
                write!(formatter, "import conflict {class} {id}")
            }
            Self::StaleRevision {
                page_id,
                expected,
                found,
            } => write!(
                formatter,
                "stale page {page_id}: expected revision {expected}, found {found}"
            ),
            Self::MigrationRequired { found } => {
                write!(formatter, "schema version {found} needs a migration")
            }
            Self::MentionsNotExported => formatter.write_str(
                "a page with mentions cannot be exported before the JSON format 2 exchange exists",
            ),
            Self::WrongUniverse { expected, found } => {
                write!(
                    formatter,
                    "wrong universe expected {expected} found {found}"
                )
            }
            Self::InvalidDocument { message } => write!(formatter, "invalid document: {message}"),
            Self::Storage { message } => write!(formatter, "storage: {message}"),
            Self::Io { message } => write!(formatter, "io: {message}"),
        }
    }
}

impl std::error::Error for Error {}
