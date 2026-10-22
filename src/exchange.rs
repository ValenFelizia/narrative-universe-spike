use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, ValidationReason};
use crate::mentions::Mention;
use crate::model::{
    Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId, Entity, EntityId,
    EntityType, MentionId, Owner, Page, PageId, Provenance, TypeId, UniverseId, Value, ValueClass,
};
use crate::store::{ImportOutcome, PageSnapshot, Snapshot, Store};

pub const MAX_DOCUMENT_BYTES: usize = 10 * 1024 * 1024;

pub fn export_json(store: &mut Store) -> Result<Vec<u8>> {
    let snapshot = store.export_snapshot()?;
    let bytes = encode(&snapshot)?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::validation(
            ValidationReason::InputTooLarge,
            "json",
            None,
        ));
    }
    Ok(bytes)
}

pub fn read_limited(reader: impl std::io::Read) -> Result<Vec<u8>> {
    let mut limited = reader.take((MAX_DOCUMENT_BYTES as u64) + 1);
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes).map_err(|err| Error::Io {
        message: format!("read document: {err}"),
    })?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::validation(
            ValidationReason::InputTooLarge,
            "json",
            None,
        ));
    }
    Ok(bytes)
}

pub fn write_exclusive(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    write_exclusive_inner(path, bytes, false)
}

fn write_exclusive_inner(
    path: &std::path::Path,
    bytes: &[u8],
    fail_after_reserve: bool,
) -> Result<()> {
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(Error::validation(
                ValidationReason::PathExists,
                "path",
                None,
            ));
        }
        Err(err) => {
            return Err(Error::Io {
                message: format!("reserve {}: {err}", path.display()),
            });
        }
    };
    if fail_after_reserve {
        return Err(Error::Io {
            message: format!(
                "write {}: injected write failure; residual file {}",
                path.display(),
                path.display()
            ),
        });
    }
    if let Err(err) = file.write_all(bytes) {
        return Err(Error::Io {
            message: format!(
                "write {}: {err}; residual file {}",
                path.display(),
                path.display()
            ),
        });
    }
    Ok(())
}

pub fn import_json(store: &mut Store, bytes: &[u8]) -> Result<ImportOutcome> {
    let snapshot = decode(bytes)?;
    store.import_snapshot(snapshot)
}

pub fn import_new_json(path: &std::path::Path, bytes: &[u8]) -> Result<(Store, ImportOutcome)> {
    let snapshot = decode(bytes)?;
    Store::import_new(path, snapshot)
}

fn encode(snapshot: &Snapshot) -> Result<Vec<u8>> {
    let envelope = EnvelopeV2 {
        format_version: crate::FORMAT_VERSION,
        universe_id: snapshot.universe_id.as_str().to_string(),
        pages: snapshot
            .pages
            .iter()
            .map(PageDtoV2::from_snapshot)
            .collect(),
        types: snapshot.types.iter().map(TypeDto::from_type).collect(),
        entities: snapshot
            .entities
            .iter()
            .map(EntityDto::from_entity)
            .collect(),
        definitions: snapshot
            .definitions
            .iter()
            .map(DefinitionDto::from_definition)
            .collect(),
        assertions: snapshot
            .assertions
            .iter()
            .map(AssertionDto::from_assertion)
            .collect(),
    };
    serde_json::to_vec(&envelope).map_err(|err| Error::InvalidDocument {
        message: err.to_string(),
    })
}

/// Reads a document and sends it to the shape of its version.
///
/// A version 1 document has no occurrences, because the field did not exist
/// then. A version 1 page that carries the field anyway is malformed, never
/// an empty occurrence set.
fn decode(bytes: &[u8]) -> Result<Snapshot> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::validation(
            ValidationReason::InputTooLarge,
            "json",
            None,
        ));
    }
    let document: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|err| Error::InvalidDocument {
            message: err.to_string(),
        })?;
    let version = match document.get("format_version") {
        None => {
            return Err(Error::InvalidDocument {
                message: "the document has no format_version".to_string(),
            })
        }
        Some(value) => value.as_i64().ok_or_else(|| Error::InvalidDocument {
            message: "the format_version of the document is not an integer".to_string(),
        })?,
    };
    match version {
        1 => decode_v1(document),
        2 => decode_v2(document),
        other => Err(Error::UnsupportedVersion {
            found: i32::try_from(other).unwrap_or(i32::MAX),
        }),
    }
}

fn decode_v1(document: serde_json::Value) -> Result<Snapshot> {
    let envelope: EnvelopeV1 =
        serde_json::from_value(document).map_err(|err| Error::InvalidDocument {
            message: err.to_string(),
        })?;
    Ok(Snapshot {
        universe_id: UniverseId::parse(&envelope.universe_id)?,
        pages: envelope
            .pages
            .into_iter()
            .map(PageDtoV1::into_snapshot)
            .collect::<Result<Vec<_>>>()?,
        types: envelope
            .types
            .into_iter()
            .map(TypeDto::into_type)
            .collect::<Result<Vec<_>>>()?,
        entities: envelope
            .entities
            .into_iter()
            .map(EntityDto::into_entity)
            .collect::<Result<Vec<_>>>()?,
        definitions: envelope
            .definitions
            .into_iter()
            .map(DefinitionDto::into_definition)
            .collect::<Result<Vec<_>>>()?,
        assertions: envelope
            .assertions
            .into_iter()
            .map(AssertionDto::into_assertion)
            .collect::<Result<Vec<_>>>()?,
    })
}

fn decode_v2(document: serde_json::Value) -> Result<Snapshot> {
    let envelope: EnvelopeV2 =
        serde_json::from_value(document).map_err(|err| Error::InvalidDocument {
            message: err.to_string(),
        })?;
    Ok(Snapshot {
        universe_id: UniverseId::parse(&envelope.universe_id)?,
        pages: envelope
            .pages
            .into_iter()
            .map(PageDtoV2::into_snapshot)
            .collect::<Result<Vec<_>>>()?,
        types: envelope
            .types
            .into_iter()
            .map(TypeDto::into_type)
            .collect::<Result<Vec<_>>>()?,
        entities: envelope
            .entities
            .into_iter()
            .map(EntityDto::into_entity)
            .collect::<Result<Vec<_>>>()?,
        definitions: envelope
            .definitions
            .into_iter()
            .map(DefinitionDto::into_definition)
            .collect::<Result<Vec<_>>>()?,
        assertions: envelope
            .assertions
            .into_iter()
            .map(AssertionDto::into_assertion)
            .collect::<Result<Vec<_>>>()?,
    })
}

/// The version 2 envelope. Every page carries its occurrences, even when
/// there are none, and no local revision is ever written.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvelopeV2 {
    format_version: i32,
    universe_id: String,
    pages: Vec<PageDtoV2>,
    types: Vec<TypeDto>,
    entities: Vec<EntityDto>,
    definitions: Vec<DefinitionDto>,
    assertions: Vec<AssertionDto>,
}

/// The historical version 1 envelope. Its pages have no occurrence field.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvelopeV1 {
    format_version: i32,
    universe_id: String,
    pages: Vec<PageDtoV1>,
    types: Vec<TypeDto>,
    entities: Vec<EntityDto>,
    definitions: Vec<DefinitionDto>,
    assertions: Vec<AssertionDto>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PageDtoV1 {
    id: String,
    title: String,
    body: String,
}

impl PageDtoV1 {
    /// A version 1 page has no occurrences, so it is read as an empty set.
    fn into_snapshot(self) -> Result<PageSnapshot> {
        Ok(PageSnapshot {
            page: Page {
                id: PageId::parse(&self.id)?,
                title: self.title,
                body: self.body,
            },
            mentions: Vec::new(),
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PageDtoV2 {
    id: String,
    title: String,
    body: String,
    mentions: Vec<MentionDto>,
}

impl PageDtoV2 {
    fn from_snapshot(snapshot: &PageSnapshot) -> Self {
        Self {
            id: snapshot.page.id.as_str().to_string(),
            title: snapshot.page.title.clone(),
            body: snapshot.page.body.clone(),
            mentions: snapshot
                .mentions
                .iter()
                .map(MentionDto::from_mention)
                .collect(),
        }
    }

    fn into_snapshot(self) -> Result<PageSnapshot> {
        Ok(PageSnapshot {
            page: Page {
                id: PageId::parse(&self.id)?,
                title: self.title,
                body: self.body,
            },
            mentions: self
                .mentions
                .into_iter()
                .map(MentionDto::into_mention)
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MentionDto {
    id: String,
    entity_id: String,
    start_utf16: u32,
    end_utf16: u32,
}

impl MentionDto {
    fn from_mention(mention: &Mention) -> Self {
        Self {
            id: mention.id.as_str().to_string(),
            entity_id: mention.entity_id.as_str().to_string(),
            start_utf16: mention.start_utf16,
            end_utf16: mention.end_utf16,
        }
    }

    fn into_mention(self) -> Result<Mention> {
        Ok(Mention {
            id: MentionId::parse(&self.id).map_err(|_| {
                Error::validation(ValidationReason::InvalidId, "mention_id", Some(&self.id))
            })?,
            entity_id: EntityId::parse(&self.entity_id).map_err(|_| {
                Error::validation(
                    ValidationReason::InvalidId,
                    "entity_id",
                    Some(&self.entity_id),
                )
            })?,
            start_utf16: self.start_utf16,
            end_utf16: self.end_utf16,
        })
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TypeDto {
    id: String,
    label: String,
}

impl TypeDto {
    fn from_type(entity_type: &EntityType) -> Self {
        Self {
            id: entity_type.id.as_str().to_string(),
            label: entity_type.label.clone(),
        }
    }

    fn into_type(self) -> Result<EntityType> {
        Ok(EntityType {
            id: TypeId::parse(&self.id)?,
            label: self.label,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EntityDto {
    id: String,
    label: String,
    #[serde(default)]
    type_id: Option<String>,
    #[serde(default)]
    page_id: Option<String>,
}

impl EntityDto {
    fn from_entity(entity: &Entity) -> Self {
        Self {
            id: entity.id.as_str().to_string(),
            label: entity.label.clone(),
            type_id: entity.type_id.as_ref().map(|id| id.as_str().to_string()),
            page_id: entity.page_id.as_ref().map(|id| id.as_str().to_string()),
        }
    }

    fn into_entity(self) -> Result<Entity> {
        Ok(Entity {
            id: EntityId::parse(&self.id)?,
            label: self.label,
            type_id: optional_id(self.type_id, TypeId::parse)?,
            page_id: optional_id(self.page_id, PageId::parse)?,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum OwnerDto {
    #[serde(rename = "type")]
    Type { id: String },
    #[serde(rename = "entity")]
    Entity { id: String },
}

impl OwnerDto {
    fn from_owner(owner: &Owner) -> Self {
        match owner {
            Owner::Type(id) => Self::Type {
                id: id.as_str().to_string(),
            },
            Owner::Entity(id) => Self::Entity {
                id: id.as_str().to_string(),
            },
        }
    }

    fn into_owner(self) -> Result<Owner> {
        match self {
            Self::Type { id } => Ok(Owner::Type(TypeId::parse(&id)?)),
            Self::Entity { id } => Ok(Owner::Entity(EntityId::parse(&id)?)),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DefinitionDto {
    id: String,
    owner: OwnerDto,
    value_class: String,
    cardinality: String,
    archived: bool,
}

impl DefinitionDto {
    fn from_definition(definition: &Definition) -> Self {
        Self {
            id: definition.id.as_str().to_string(),
            owner: OwnerDto::from_owner(&definition.owner),
            value_class: definition.value_class.as_str().to_string(),
            cardinality: definition.cardinality.as_str().to_string(),
            archived: definition.archived,
        }
    }

    fn into_definition(self) -> Result<Definition> {
        Ok(Definition {
            id: DefinitionId::parse(&self.id)?,
            owner: self.owner.into_owner()?,
            value_class: parse_class(&self.value_class)?,
            cardinality: parse_cardinality(&self.cardinality)?,
            archived: self.archived,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum ValueDto {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "boolean")]
    Boolean { value: bool },
    #[serde(rename = "reference")]
    Reference { target_id: String },
    #[serde(rename = "unknown")]
    Unknown {},
}

impl ValueDto {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::Text(text) => Self::Text { text: text.clone() },
            Value::Boolean(value) => Self::Boolean { value: *value },
            Value::Reference(id) => Self::Reference {
                target_id: id.as_str().to_string(),
            },
            Value::Unknown => Self::Unknown {},
        }
    }

    fn into_value(self) -> Result<Value> {
        match self {
            Self::Text { text } => Ok(Value::Text(text)),
            Self::Boolean { value } => Ok(Value::Boolean(value)),
            Self::Reference { target_id } => Ok(Value::Reference(EntityId::parse(&target_id)?)),
            Self::Unknown {} => Ok(Value::Unknown),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceDto {
    #[serde(default)]
    external_ref: Option<String>,
    #[serde(default)]
    locator: Option<String>,
    #[serde(default)]
    fragment: Option<String>,
    #[serde(default)]
    source_revision: Option<String>,
}

impl ProvenanceDto {
    fn from_provenance(provenance: &Provenance) -> Self {
        Self {
            external_ref: provenance.external_ref.clone(),
            locator: provenance.locator.clone(),
            fragment: provenance.fragment.clone(),
            source_revision: provenance.source_revision.clone(),
        }
    }

    fn into_provenance(self) -> Provenance {
        Provenance {
            external_ref: self.external_ref,
            locator: self.locator,
            fragment: self.fragment,
            source_revision: self.source_revision,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AssertionDto {
    id: String,
    subject_id: String,
    definition_id: String,
    value: ValueDto,
    canon: String,
    #[serde(default)]
    source_status_raw: Option<String>,
    provenance: ProvenanceDto,
    #[serde(default)]
    scope_note: Option<String>,
    archived: bool,
}

impl AssertionDto {
    fn from_assertion(assertion: &Assertion) -> Self {
        Self {
            id: assertion.id.as_str().to_string(),
            subject_id: assertion.subject_id.as_str().to_string(),
            definition_id: assertion.definition_id.as_str().to_string(),
            value: ValueDto::from_value(&assertion.value),
            canon: assertion.canon.as_str().to_string(),
            source_status_raw: assertion.source_status_raw.clone(),
            provenance: ProvenanceDto::from_provenance(&assertion.provenance),
            scope_note: assertion.scope_note.clone(),
            archived: assertion.archived,
        }
    }

    fn into_assertion(self) -> Result<Assertion> {
        Ok(Assertion {
            id: AssertionId::parse(&self.id)?,
            subject_id: EntityId::parse(&self.subject_id)?,
            definition_id: DefinitionId::parse(&self.definition_id)?,
            value: self.value.into_value()?,
            canon: parse_canon(&self.canon)?,
            source_status_raw: self.source_status_raw,
            provenance: self.provenance.into_provenance(),
            scope_note: self.scope_note,
            archived: self.archived,
        })
    }
}

fn optional_id<T>(raw: Option<String>, parse: impl FnOnce(&str) -> Result<T>) -> Result<Option<T>> {
    raw.map(|raw| parse(&raw)).transpose()
}

fn parse_canon(raw: &str) -> Result<Canon> {
    match raw {
        "ACORDADO" => Ok(Canon::Acordado),
        "PROVISIONAL" => Ok(Canon::Provisional),
        "ABIERTO" => Ok(Canon::Abierto),
        "MISTERIO_INTENCIONAL" => Ok(Canon::MisterioIntencional),
        _ => Err(Error::InvalidDocument {
            message: format!("invalid canon {raw}"),
        }),
    }
}

fn parse_class(raw: &str) -> Result<ValueClass> {
    match raw {
        "text" => Ok(ValueClass::Text),
        "boolean" => Ok(ValueClass::Boolean),
        "reference" => Ok(ValueClass::Reference),
        _ => Err(Error::InvalidDocument {
            message: format!("invalid value class {raw}"),
        }),
    }
}

fn parse_cardinality(raw: &str) -> Result<Cardinality> {
    match raw {
        "singular" => Ok(Cardinality::Singular),
        "multiple" => Ok(Cardinality::Multiple),
        _ => Err(Error::InvalidDocument {
            message: format!("invalid cardinality {raw}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::write_exclusive_inner;

    #[test]
    fn failed_export_write_keeps_the_residual_and_other_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let kept = dir.path().join("keep-me.txt");
        std::fs::write(&kept, b"keep-me").expect("keep");
        let residual = dir.path().join("export.json");
        let err = write_exclusive_inner(&residual, b"{}", true).expect_err("write");
        let message = err.to_string();
        assert!(message.contains("residual file"), "{message}");
        assert!(
            message.contains(residual.to_str().expect("path")),
            "{message}"
        );
        assert!(residual.is_file());
        assert_eq!(std::fs::read(&kept).expect("kept"), b"keep-me");
    }
}
