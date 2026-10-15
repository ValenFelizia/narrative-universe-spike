mod error;
mod model;
mod store;

pub use error::{Error, Result, ValidationReason};
pub use model::{
    Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId, Entity, EntityId,
    EntityType, Owner, Page, PageId, Provenance, TypeId, UniverseId, Value, ValueClass,
};
pub use store::{Counts, Store};

pub const SCHEMA_VERSION: i32 = 1;
pub const FORMAT_VERSION: i32 = 1;
