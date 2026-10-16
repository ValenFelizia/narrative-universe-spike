mod error;
mod exchange;
mod model;
mod store;

pub use error::{Error, Result, ValidationReason};
pub use exchange::{
    export_json, import_json, import_new_json, read_limited, write_exclusive, MAX_DOCUMENT_BYTES,
};
pub use model::{
    Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId, Entity, EntityId,
    EntityType, Owner, Page, PageId, Provenance, TypeId, UniverseId, Value, ValueClass,
};
pub use store::{Counts, ImportOutcome, PossibleIncompatibility, Snapshot, Store};

pub const SCHEMA_VERSION: i32 = 1;
pub const FORMAT_VERSION: i32 = 1;
