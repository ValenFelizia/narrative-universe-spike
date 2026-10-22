mod error;
mod exchange;
mod mentions;
mod model;
mod store;

pub use error::{Error, Result, ValidationReason};
pub use exchange::{
    export_json, import_json, import_new_json, read_limited, write_exclusive, MAX_DOCUMENT_BYTES,
};
pub use mentions::{apply_mention_edit, validate_mentions, AnnotatedBody, Mention, MentionEdit};
pub use model::{
    Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId, Entity, EntityId,
    EntityType, MentionId, Owner, Page, PageId, PageRevision, Provenance, TypeId, UniverseId,
    Value, ValueClass,
};
pub use store::{
    Counts, ImportOutcome, MentionOccurrence, PageDocument, PageEdit, PageSnapshot,
    PossibleIncompatibility, Snapshot, Store,
};

pub const SCHEMA_VERSION: i32 = 2;
pub const FORMAT_VERSION: i32 = 2;
