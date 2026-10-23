use crate::error::{Error, Result, ValidationReason};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Id(String);

impl Id {
    pub(crate) fn parse(raw: &str) -> Result<Self> {
        let parsed = uuid::Uuid::parse_str(raw)
            .map_err(|_| Error::validation(ValidationReason::InvalidId, "id", Some(raw)))?;
        let canonical = parsed.as_hyphenated().to_string();
        if raw != canonical {
            return Err(Error::validation(
                ValidationReason::InvalidId,
                "id",
                Some(raw),
            ));
        }
        Ok(Self(canonical))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UniverseId(Id);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PageId(Id);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeId(Id);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntityId(Id);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DefinitionId(Id);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssertionId(Id);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MentionId(Id);

/// The revision of one page aggregate inside one database.
///
/// The token is database-local. It counts committed aggregate changes and is
/// not a portable identity, a timestamp, or a document revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageRevision(i64);

impl PageRevision {
    /// The revision of a page that never changed.
    pub const ZERO: Self = Self(0);

    /// Builds a revision from a stored value. A negative value is malformed.
    pub fn new(raw: i64) -> Result<Self> {
        if raw < 0 {
            return Err(Error::validation(
                ValidationReason::InvalidRevision,
                "revision",
                None,
            ));
        }
        Ok(Self(raw))
    }

    pub fn as_i64(self) -> i64 {
        self.0
    }

    /// The next revision, or an explicit failure when the counter is spent.
    pub fn checked_next(self) -> Result<Self> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| Error::validation(ValidationReason::RevisionExhausted, "revision", None))
    }
}

impl std::fmt::Display for PageRevision {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl UniverseId {
    pub fn parse(raw: &str) -> Result<Self> {
        Id::parse(raw).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl PageId {
    pub fn parse(raw: &str) -> Result<Self> {
        Id::parse(raw).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TypeId {
    pub fn parse(raw: &str) -> Result<Self> {
        Id::parse(raw).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl EntityId {
    pub fn parse(raw: &str) -> Result<Self> {
        Id::parse(raw).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl DefinitionId {
    pub fn parse(raw: &str) -> Result<Self> {
        Id::parse(raw).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl AssertionId {
    pub fn parse(raw: &str) -> Result<Self> {
        Id::parse(raw).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for UniverseId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::fmt::Display for PageId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::fmt::Display for TypeId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::fmt::Display for DefinitionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::fmt::Display for AssertionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl MentionId {
    pub fn parse(raw: &str) -> Result<Self> {
        Id::parse(raw).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for MentionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Canon {
    Acordado,
    Provisional,
    Abierto,
    MisterioIntencional,
}

impl Canon {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Acordado => "ACORDADO",
            Self::Provisional => "PROVISIONAL",
            Self::Abierto => "ABIERTO",
            Self::MisterioIntencional => "MISTERIO_INTENCIONAL",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            "ACORDADO" => Ok(Self::Acordado),
            "PROVISIONAL" => Ok(Self::Provisional),
            "ABIERTO" => Ok(Self::Abierto),
            "MISTERIO_INTENCIONAL" => Ok(Self::MisterioIntencional),
            _ => Err(Error::Storage {
                message: "invalid canon".to_string(),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueClass {
    Text,
    Boolean,
    Reference,
}

impl ValueClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Boolean => "boolean",
            Self::Reference => "reference",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            "text" => Ok(Self::Text),
            "boolean" => Ok(Self::Boolean),
            "reference" => Ok(Self::Reference),
            _ => Err(Error::Storage {
                message: "invalid value class".to_string(),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cardinality {
    Singular,
    Multiple,
}

impl Cardinality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Singular => "singular",
            Self::Multiple => "multiple",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw {
            "singular" => Ok(Self::Singular),
            "multiple" => Ok(Self::Multiple),
            _ => Err(Error::Storage {
                message: "invalid cardinality".to_string(),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owner {
    Type(TypeId),
    Entity(EntityId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Boolean(bool),
    Reference(EntityId),
    Unknown,
}

pub(crate) fn value_fits(class: ValueClass, value: &Value) -> bool {
    match value {
        Value::Unknown => true,
        Value::Text(_) => class == ValueClass::Text,
        Value::Boolean(_) => class == ValueClass::Boolean,
        Value::Reference(_) => class == ValueClass::Reference,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub external_ref: Option<String>,
    pub locator: Option<String>,
    pub fragment: Option<String>,
    pub source_revision: Option<String>,
}

impl Provenance {
    pub fn absent() -> Self {
        Self {
            external_ref: None,
            locator: None,
            fragment: None,
            source_revision: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub id: PageId,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityType {
    pub id: TypeId,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entity {
    pub id: EntityId,
    pub label: String,
    pub type_id: Option<TypeId>,
    pub page_id: Option<PageId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    pub id: DefinitionId,
    pub owner: Owner,
    pub value_class: ValueClass,
    pub cardinality: Cardinality,
    pub archived: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assertion {
    pub id: AssertionId,
    pub subject_id: EntityId,
    pub definition_id: DefinitionId,
    pub value: Value,
    pub canon: Canon,
    pub source_status_raw: Option<String>,
    pub provenance: Provenance,
    pub scope_note: Option<String>,
    pub archived: bool,
}
