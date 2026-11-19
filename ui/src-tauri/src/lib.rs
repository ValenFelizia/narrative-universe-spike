use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Mutex;

use narrative_universe::{
    apply_mention_edit, validate_codex_label, Entity, EntityId, EntityType, Error, Mention,
    MentionEdit, MentionId, Page, PageEdit, PageId, PageRevision, PageSummary, Store, TypeId,
    ValidationReason,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

struct DesktopState {
    store: Mutex<Option<ActiveUniverse>>,
    close_request: Mutex<Option<String>>,
}

struct ActiveUniverse {
    token: String,
    store: Store,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenedUniverse {
    token: String,
    pages: Vec<PageIndexEntry>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
enum BridgeError {
    EmptyPath,
    DatabaseMissing,
    InvalidDatabase,
    UnsupportedDatabase,
    UniverseNotOpen,
    UniverseChanged,
    PageMissing,
    EntityMissing,
    TypeMissing,
    ContextTooLarge,
    ReadFailed,
    StateUnavailable,
    Stale,
    Busy,
    InvalidInput,
    DuplicateId,
    ReadOnly,
    /// The real reducer disagrees with the client prediction. Nothing was
    /// written. Names every mention id whose final state differs.
    MentionMismatch(Vec<String>),
    /// The real reducer produced another body than the client predicted.
    /// Nothing was written.
    BodyMismatch,
    Storage,
    Io,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PageIndexEntry {
    id: String,
    title: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PageContent {
    id: String,
    title: String,
    body: String,
    revision: Option<i64>,
    mentions: Vec<WireMention>,
}

/// One occurrence on the wire. Read, save assertion, and acknowledgement all
/// use this one shape.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireMention {
    id: String,
    entity_id: String,
    start_utf16: u32,
    end_utf16: u32,
}

impl From<&Mention> for WireMention {
    fn from(mention: &Mention) -> Self {
        Self {
            id: mention.id.as_str().to_owned(),
            entity_id: mention.entity_id.as_str().to_owned(),
            start_utf16: mention.start_utf16,
            end_utf16: mention.end_utf16,
        }
    }
}

fn wire_mentions(mentions: &[Mention]) -> Vec<WireMention> {
    mentions.iter().map(WireMention::from).collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntityIndexEntry {
    id: String,
    label: String,
    type_label: Option<String>,
    primary_page_title: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TypeIndexEntry {
    id: String,
    label: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct CodexEntityEntry {
    id: String,
    label: String,
    type_id: Option<String>,
    type_label: Option<String>,
    primary_page_title: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct CodexIndex {
    types: Vec<TypeIndexEntry>,
    entities: Vec<CodexEntityEntry>,
}

const MAX_JS_REVISION: i64 = 9_007_199_254_740_991;

/// One ordered save operation, in the coordinates of the body the previous
/// operation produced.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum SaveOp {
    Replace {
        start_utf16: u32,
        end_utf16: u32,
        inserted_text: String,
    },
    Link {
        mention_id: String,
        entity_id: String,
        start_utf16: u32,
        end_utf16: u32,
    },
    Unlink {
        mention_id: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveRequest {
    universe_token: String,
    page_id: String,
    session_id: String,
    request_id: u64,
    expected_revision: i64,
    title: Option<String>,
    edits: Vec<SaveOp>,
    expected_body: String,
    expected_mentions: Vec<WireMention>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveResponse {
    page_id: String,
    session_id: String,
    request_id: u64,
    title: String,
    body: String,
    mentions: Vec<WireMention>,
    revision: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveFailure {
    page_id: String,
    session_id: String,
    request_id: u64,
    #[serde(flatten)]
    kind: BridgeError,
}

fn save_error(error: Error) -> BridgeError {
    match error {
        Error::StaleRevision { .. } => BridgeError::Stale,
        Error::Busy => BridgeError::Busy,
        Error::Validation {
            reason: ValidationReason::NotFound | ValidationReason::UnknownPage,
            ..
        } => BridgeError::PageMissing,
        Error::Validation {
            reason: ValidationReason::DuplicateId,
            ..
        } => BridgeError::DuplicateId,
        Error::Validation { .. } | Error::InvalidDocument { .. } => BridgeError::InvalidInput,
        Error::Io { .. } => BridgeError::Io,
        _ => BridgeError::Storage,
    }
}

fn map_open_error(error: Error) -> BridgeError {
    match error {
        Error::Validation {
            reason: ValidationReason::PathMissing,
            ..
        } => BridgeError::DatabaseMissing,
        Error::Validation {
            reason: ValidationReason::NotADatabase | ValidationReason::ForeignDatabase,
            ..
        } => BridgeError::InvalidDatabase,
        Error::UnsupportedVersion { .. }
        | Error::MigrationRequired { .. }
        | Error::UnsupportedDatabase { .. } => BridgeError::UnsupportedDatabase,
        _ => BridgeError::ReadFailed,
    }
}

fn map_read_error(error: Error, page_read: bool) -> BridgeError {
    match error {
        Error::Validation {
            reason: ValidationReason::NotFound | ValidationReason::UnknownPage,
            ..
        } if page_read => BridgeError::PageMissing,
        _ => BridgeError::ReadFailed,
    }
}

#[tauri::command(async)]
fn open_universe(
    database_path: String,
    state: State<'_, DesktopState>,
) -> Result<OpenedUniverse, BridgeError> {
    if database_path.trim().is_empty() {
        return Err(BridgeError::EmptyPath);
    }
    let (store, pages) = prepare_universe(Path::new(&database_path))?;
    let mut active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    let token = uuid::Uuid::new_v4().to_string();
    *active = Some(ActiveUniverse {
        token: token.clone(),
        store,
    });
    Ok(OpenedUniverse { token, pages })
}

fn prepare_universe(path: &Path) -> Result<(Store, Vec<PageIndexEntry>), BridgeError> {
    let store = Store::open(path).map_err(map_open_error)?;
    let pages = index_entries(&store)?;
    Ok((store, pages))
}

fn index_entries(store: &Store) -> Result<Vec<PageIndexEntry>, BridgeError> {
    let pages: Vec<PageSummary> = store
        .page_index()
        .map_err(|error| map_read_error(error, false))?;
    Ok(pages
        .into_iter()
        .map(|page| PageIndexEntry {
            id: page.id.as_str().to_string(),
            title: page.title,
        })
        .collect())
}

#[tauri::command(async)]
fn page_index(
    universe_token: String,
    state: State<'_, DesktopState>,
) -> Result<Vec<PageIndexEntry>, BridgeError> {
    let active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    index_entries(active_store(&active, &universe_token)?)
}

fn active_store<'a>(
    active: &'a Option<ActiveUniverse>,
    token: &str,
) -> Result<&'a Store, BridgeError> {
    let active = active.as_ref().ok_or(BridgeError::UniverseNotOpen)?;
    if active.token != token {
        return Err(BridgeError::UniverseChanged);
    }
    Ok(&active.store)
}

#[tauri::command(async)]
fn read_page(
    page_id: String,
    universe_token: String,
    state: State<'_, DesktopState>,
) -> Result<PageContent, BridgeError> {
    let active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    page_content(active_store(&active, &universe_token)?, &page_id)
}

fn page_content(store: &Store, page_id: &str) -> Result<PageContent, BridgeError> {
    let id = PageId::parse(page_id).map_err(|_| BridgeError::PageMissing)?;
    let document = store
        .page_document(&id)
        .map_err(|error| map_read_error(error, true))?;
    Ok(PageContent {
        id: document.id.as_str().to_string(),
        title: document.title,
        body: document.body.body,
        revision: (document.revision.as_i64() <= MAX_JS_REVISION)
            .then_some(document.revision.as_i64()),
        mentions: wire_mentions(&document.body.mentions),
    })
}

#[tauri::command(async)]
fn entity_index(
    universe_token: String,
    state: State<'_, DesktopState>,
) -> Result<Vec<EntityIndexEntry>, BridgeError> {
    let active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    entity_entries(active_store(&active, &universe_token)?)
}

fn entity_entries(store: &Store) -> Result<Vec<EntityIndexEntry>, BridgeError> {
    let entities = store
        .entity_index()
        .map_err(|error| map_read_error(error, false))?;
    Ok(entities
        .into_iter()
        .map(|entity| EntityIndexEntry {
            id: entity.id.as_str().to_owned(),
            label: entity.label,
            type_label: entity.type_label,
            primary_page_title: entity.primary_page_title,
        })
        .collect())
}

fn codex_entries(store: &Store) -> Result<CodexIndex, BridgeError> {
    let index = store.codex_index().map_err(|error| match error {
        Error::Busy => BridgeError::Busy,
        Error::Io { .. } => BridgeError::Io,
        other => map_read_error(other, false),
    })?;
    Ok(CodexIndex {
        types: index
            .types
            .into_iter()
            .map(|entity_type| TypeIndexEntry {
                id: entity_type.id.as_str().to_owned(),
                label: entity_type.label,
            })
            .collect(),
        entities: index
            .entities
            .into_iter()
            .map(|entity| CodexEntityEntry {
                id: entity.id.as_str().to_owned(),
                label: entity.label,
                type_id: entity.type_id.map(|id| id.as_str().to_owned()),
                type_label: entity.type_label,
                primary_page_title: entity.primary_page_title,
            })
            .collect(),
    })
}

fn codex_write_error(error: Error) -> BridgeError {
    match error {
        Error::Busy => BridgeError::Busy,
        Error::Validation {
            reason: ValidationReason::DuplicateId,
            ..
        } => BridgeError::DuplicateId,
        Error::Validation {
            reason: ValidationReason::UnknownType,
            ..
        } => BridgeError::TypeMissing,
        Error::Validation { .. } | Error::InvalidDocument { .. } => BridgeError::InvalidInput,
        Error::Io { .. } => BridgeError::Io,
        _ => BridgeError::Storage,
    }
}

#[tauri::command(async)]
fn codex_index(
    universe_token: String,
    state: State<'_, DesktopState>,
) -> Result<CodexIndex, BridgeError> {
    let active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    codex_entries(active_store(&active, &universe_token)?)
}

fn create_type_active(
    active: &mut Option<ActiveUniverse>,
    token: &str,
    type_id: &str,
    label: &str,
) -> Result<TypeIndexEntry, BridgeError> {
    let universe = active.as_mut().ok_or(BridgeError::UniverseNotOpen)?;
    if universe.token != token {
        return Err(BridgeError::UniverseChanged);
    }
    let id = TypeId::parse(type_id).map_err(|_| BridgeError::InvalidInput)?;
    let valid_label = validate_codex_label(label).map_err(|_| BridgeError::InvalidInput)?;
    let created = universe
        .store
        .create_entity_type(EntityType {
            id,
            label: valid_label.to_owned(),
        })
        .map_err(codex_write_error)?;
    Ok(TypeIndexEntry {
        id: created.id.as_str().to_owned(),
        label: created.label,
    })
}

#[tauri::command(async)]
fn create_entity_type(
    type_id: String,
    label: String,
    universe_token: String,
    state: State<'_, DesktopState>,
) -> Result<TypeIndexEntry, BridgeError> {
    let mut active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    create_type_active(&mut active, &universe_token, &type_id, &label)
}

fn create_entity_active(
    active: &mut Option<ActiveUniverse>,
    token: &str,
    entity_id: &str,
    label: &str,
    type_id: Option<&str>,
) -> Result<CodexEntityEntry, BridgeError> {
    let universe = active.as_mut().ok_or(BridgeError::UniverseNotOpen)?;
    if universe.token != token {
        return Err(BridgeError::UniverseChanged);
    }
    let id = EntityId::parse(entity_id).map_err(|_| BridgeError::InvalidInput)?;
    let valid_label = validate_codex_label(label).map_err(|_| BridgeError::InvalidInput)?;
    let parsed_type_id = type_id
        .map(TypeId::parse)
        .transpose()
        .map_err(|_| BridgeError::InvalidInput)?;
    let created = universe
        .store
        .create_entity(Entity {
            id,
            label: valid_label.to_owned(),
            type_id: parsed_type_id,
            page_id: None,
        })
        .map_err(codex_write_error)?;
    let type_label = created
        .type_id
        .as_ref()
        .and_then(|tid| universe.store.entity_type(tid).ok())
        .map(|entity_type| entity_type.label);
    Ok(CodexEntityEntry {
        id: created.id.as_str().to_owned(),
        label: created.label,
        type_id: created.type_id.map(|tid| tid.as_str().to_owned()),
        type_label,
        primary_page_title: None,
    })
}

#[tauri::command(async)]
fn create_entity(
    entity_id: String,
    label: String,
    type_id: Option<String>,
    universe_token: String,
    state: State<'_, DesktopState>,
) -> Result<CodexEntityEntry, BridgeError> {
    let mut active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    create_entity_active(
        &mut active,
        &universe_token,
        &entity_id,
        &label,
        type_id.as_deref(),
    )
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RevealIdentity {
    id: String,
    label: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RevealValue {
    Text { text: String },
    Boolean { value: bool },
    Reference { target: RevealIdentity },
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RevealSource {
    external_ref: Option<String>,
    locator: Option<String>,
    fragment: Option<String>,
    source_revision: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RevealClaim {
    id: String,
    definition_id: String,
    property_label: Option<String>,
    value_class: String,
    cardinality: String,
    subject: RevealIdentity,
    value: RevealValue,
    canon: String,
    source_status_raw: Option<String>,
    provenance: RevealSource,
    scope_note: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntityReveal {
    entity: RevealIdentity,
    type_label: Option<String>,
    outgoing: Vec<RevealClaim>,
    incoming: Vec<RevealClaim>,
}

fn reveal_identity(entity: narrative_universe::Entity) -> RevealIdentity {
    RevealIdentity {
        id: entity.id.as_str().to_owned(),
        label: entity.label,
    }
}

fn reveal_claim(claim: narrative_universe::RevealClaim) -> Result<RevealClaim, BridgeError> {
    use narrative_universe::Value;
    let a = claim.assertion;
    let value = match a.value {
        Value::Text(text) => RevealValue::Text { text },
        Value::Boolean(value) => RevealValue::Boolean { value },
        Value::Unknown => RevealValue::Unknown,
        Value::Reference(_) => RevealValue::Reference {
            target: reveal_identity(claim.target.ok_or(BridgeError::ReadFailed)?),
        },
    };
    Ok(RevealClaim {
        id: a.id.as_str().to_owned(),
        definition_id: claim.definition.id.as_str().to_owned(),
        property_label: claim.definition.label.map(|l| l.as_str().to_owned()),
        value_class: claim.definition.value_class.as_str().to_owned(),
        cardinality: claim.definition.cardinality.as_str().to_owned(),
        subject: reveal_identity(claim.subject),
        value,
        canon: a.canon.as_str().to_owned(),
        source_status_raw: a.source_status_raw,
        scope_note: a.scope_note,
        provenance: RevealSource {
            external_ref: a.provenance.external_ref,
            locator: a.provenance.locator,
            fragment: a.provenance.fragment,
            source_revision: a.provenance.source_revision,
        },
    })
}

fn reveal_active(
    active: &Option<ActiveUniverse>,
    token: &str,
    entity_id: &str,
) -> Result<EntityReveal, BridgeError> {
    let store = active_store(active, token)?;
    let id = EntityId::parse(entity_id).map_err(|_| BridgeError::EntityMissing)?;
    let reveal = store.entity_reveal(&id).map_err(|error| match error {
        Error::Validation {
            reason: ValidationReason::NotFound,
            ..
        } => BridgeError::EntityMissing,
        Error::Busy => BridgeError::Busy,
        Error::Io { .. } => BridgeError::Io,
        other => map_read_error(other, false),
    })?;
    Ok(EntityReveal {
        entity: reveal_identity(reveal.entity),
        type_label: reveal.type_label,
        outgoing: reveal
            .outgoing
            .into_iter()
            .map(reveal_claim)
            .collect::<Result<_, _>>()?,
        incoming: reveal
            .incoming
            .into_iter()
            .map(reveal_claim)
            .collect::<Result<_, _>>()?,
    })
}

#[tauri::command(async)]
fn entity_reveal(
    universe_token: String,
    entity_id: String,
    state: State<'_, DesktopState>,
) -> Result<EntityReveal, BridgeError> {
    let active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    reveal_active(&active, &universe_token, &entity_id)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContextDefinition {
    id: String,
    owner_kind: &'static str,
    owner_id: String,
    property_label: Option<String>,
    value_class: String,
    cardinality: String,
    archived: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContextClaim {
    id: String,
    value: RevealValue,
    canon: String,
    source_status_raw: Option<String>,
    provenance: RevealSource,
    scope_note: Option<String>,
    archived: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContextOutgoingGroup {
    definition: ContextDefinition,
    claims: Vec<ContextClaim>,
    has_archived_claims: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContextIncomingGroup {
    subject: RevealIdentity,
    definition: ContextDefinition,
    claims: Vec<ContextClaim>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntityContext {
    entity: RevealIdentity,
    type_label: Option<String>,
    outgoing_groups: Vec<ContextOutgoingGroup>,
    incoming_groups: Vec<ContextIncomingGroup>,
    archived_outgoing_groups: Vec<ContextOutgoingGroup>,
    archived_incoming_groups: Vec<ContextIncomingGroup>,
}

fn context_definition(definition: narrative_universe::Definition) -> ContextDefinition {
    let (owner_kind, owner_id) = match definition.owner {
        narrative_universe::Owner::Type(type_id) => ("type", type_id.as_str().to_owned()),
        narrative_universe::Owner::Entity(entity_id) => ("entity", entity_id.as_str().to_owned()),
    };
    ContextDefinition {
        id: definition.id.as_str().to_owned(),
        owner_kind,
        owner_id,
        property_label: definition.label.map(|l| l.as_str().to_owned()),
        value_class: definition.value_class.as_str().to_owned(),
        cardinality: definition.cardinality.as_str().to_owned(),
        archived: definition.archived,
    }
}

fn context_claim(claim: narrative_universe::ContextClaim) -> Result<ContextClaim, BridgeError> {
    use narrative_universe::Value;
    let a = claim.assertion;
    let value = match a.value {
        Value::Text(text) => RevealValue::Text { text },
        Value::Boolean(value) => RevealValue::Boolean { value },
        Value::Unknown => RevealValue::Unknown,
        Value::Reference(_) => RevealValue::Reference {
            target: reveal_identity(claim.target.ok_or(BridgeError::ReadFailed)?),
        },
    };
    Ok(ContextClaim {
        id: a.id.as_str().to_owned(),
        value,
        canon: a.canon.as_str().to_owned(),
        source_status_raw: a.source_status_raw,
        provenance: RevealSource {
            external_ref: a.provenance.external_ref,
            locator: a.provenance.locator,
            fragment: a.provenance.fragment,
            source_revision: a.provenance.source_revision,
        },
        scope_note: a.scope_note,
        archived: a.archived,
    })
}

fn context_outgoing_group(
    group: narrative_universe::ContextOutgoingGroup,
) -> Result<ContextOutgoingGroup, BridgeError> {
    Ok(ContextOutgoingGroup {
        definition: context_definition(group.definition),
        claims: group
            .claims
            .into_iter()
            .map(context_claim)
            .collect::<Result<_, _>>()?,
        has_archived_claims: group.has_archived_claims,
    })
}

fn context_incoming_group(
    group: narrative_universe::ContextIncomingGroup,
) -> Result<ContextIncomingGroup, BridgeError> {
    Ok(ContextIncomingGroup {
        subject: reveal_identity(group.subject),
        definition: context_definition(group.definition),
        claims: group
            .claims
            .into_iter()
            .map(context_claim)
            .collect::<Result<_, _>>()?,
    })
}

fn context_active(
    active: &Option<ActiveUniverse>,
    token: &str,
    entity_id: &str,
) -> Result<EntityContext, BridgeError> {
    let store = active_store(active, token)?;
    let id = EntityId::parse(entity_id).map_err(|_| BridgeError::EntityMissing)?;
    let context = store.entity_context(&id).map_err(|error| match error {
        Error::Validation {
            reason: ValidationReason::NotFound,
            ..
        } => BridgeError::EntityMissing,
        Error::Validation {
            reason: ValidationReason::ContextTooLarge,
            ..
        } => BridgeError::ContextTooLarge,
        Error::Busy => BridgeError::Busy,
        Error::Io { .. } => BridgeError::Io,
        other => map_read_error(other, false),
    })?;
    let dto = EntityContext {
        entity: reveal_identity(context.entity),
        type_label: context.type_label,
        outgoing_groups: context
            .outgoing_groups
            .into_iter()
            .map(context_outgoing_group)
            .collect::<Result<_, _>>()?,
        incoming_groups: context
            .incoming_groups
            .into_iter()
            .map(context_incoming_group)
            .collect::<Result<_, _>>()?,
        archived_outgoing_groups: context
            .archived_outgoing_groups
            .into_iter()
            .map(context_outgoing_group)
            .collect::<Result<_, _>>()?,
        archived_incoming_groups: context
            .archived_incoming_groups
            .into_iter()
            .map(context_incoming_group)
            .collect::<Result<_, _>>()?,
    };
    if context_dto_wire_bytes(&dto).is_none_or(|len| len > narrative_universe::MAX_CONTEXT_BYTES) {
        return Err(BridgeError::ContextTooLarge);
    }
    Ok(dto)
}

fn json_str_wire_bytes(s: &str) -> Option<usize> {
    let mut total = 2usize;
    for &b in s.as_bytes() {
        let step = match b {
            b'"' | b'\\' | b'\x08' | b'\x09' | b'\x0a' | b'\x0c' | b'\x0d' => 2,
            0x00..=0x1f => 6,
            _ => 1,
        };
        total = total.checked_add(step)?;
    }
    Some(total)
}

fn json_opt_str_wire_bytes(s: Option<&str>) -> Option<usize> {
    match s {
        Some(text) => json_str_wire_bytes(text),
        None => Some(4),
    }
}

fn json_array_wire_bytes<T>(
    items: &[T],
    item_bytes: impl Fn(&T) -> Option<usize>,
) -> Option<usize> {
    let mut total = 2usize; // "[]"
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            total = total.checked_add(1)?;
        }
        total = total.checked_add(item_bytes(item)?)?;
    }
    Some(total)
}

fn reveal_identity_wire_bytes(identity: &RevealIdentity) -> Option<usize> {
    // {"id":<id>,"label":<label>}
    16usize
        .checked_add(json_str_wire_bytes(&identity.id)?)?
        .checked_add(json_str_wire_bytes(&identity.label)?)
}

fn context_definition_wire_bytes(def: &ContextDefinition) -> Option<usize> {
    // {"id":,"ownerKind":,"ownerId":,"propertyLabel":,"valueClass":,"cardinality":,"archived":}
    let archived = if def.archived { 4 } else { 5 };
    89usize
        .checked_add(json_str_wire_bytes(&def.id)?)?
        .checked_add(json_str_wire_bytes(def.owner_kind)?)?
        .checked_add(json_str_wire_bytes(&def.owner_id)?)?
        .checked_add(json_opt_str_wire_bytes(def.property_label.as_deref())?)?
        .checked_add(json_str_wire_bytes(&def.value_class)?)?
        .checked_add(json_str_wire_bytes(&def.cardinality)?)?
        .checked_add(archived)
}

fn context_claim_wire_bytes(claim: &ContextClaim) -> Option<usize> {
    // {"id":,"value":,"canon":,"sourceStatusRaw":,"provenance":{"externalRef":,"locator":,"fragment":,"sourceRevision":},"scopeNote":,"archived":}
    let value = match &claim.value {
        RevealValue::Text { text } => 23usize.checked_add(json_str_wire_bytes(text)?)?,
        RevealValue::Boolean { value } => 27usize + if *value { 4 } else { 5 },
        RevealValue::Reference { target } => {
            30usize.checked_add(reveal_identity_wire_bytes(target)?)?
        }
        RevealValue::Unknown => 18usize,
    };
    let archived = if claim.archived { 4 } else { 5 };
    140usize
        .checked_add(json_str_wire_bytes(&claim.id)?)?
        .checked_add(value)?
        .checked_add(json_str_wire_bytes(&claim.canon)?)?
        .checked_add(json_opt_str_wire_bytes(claim.source_status_raw.as_deref())?)?
        .checked_add(json_opt_str_wire_bytes(
            claim.provenance.external_ref.as_deref(),
        )?)?
        .checked_add(json_opt_str_wire_bytes(
            claim.provenance.locator.as_deref(),
        )?)?
        .checked_add(json_opt_str_wire_bytes(
            claim.provenance.fragment.as_deref(),
        )?)?
        .checked_add(json_opt_str_wire_bytes(
            claim.provenance.source_revision.as_deref(),
        )?)?
        .checked_add(json_opt_str_wire_bytes(claim.scope_note.as_deref())?)?
        .checked_add(archived)
}

fn context_outgoing_group_wire_bytes(group: &ContextOutgoingGroup) -> Option<usize> {
    // {"definition":,"claims":[],"hasArchivedClaims":}
    let has_arch = if group.has_archived_claims { 4 } else { 5 };
    46usize
        .checked_add(context_definition_wire_bytes(&group.definition)?)?
        .checked_add(json_array_wire_bytes(
            &group.claims,
            context_claim_wire_bytes,
        )?)?
        .checked_add(has_arch)
}

fn context_incoming_group_wire_bytes(group: &ContextIncomingGroup) -> Option<usize> {
    // {"subject":,"definition":,"claims":[]}
    36usize
        .checked_add(reveal_identity_wire_bytes(&group.subject)?)?
        .checked_add(context_definition_wire_bytes(&group.definition)?)?
        .checked_add(json_array_wire_bytes(
            &group.claims,
            context_claim_wire_bytes,
        )?)
}

fn context_dto_wire_bytes(dto: &EntityContext) -> Option<usize> {
    // {"entity":,"typeLabel":,"outgoingGroups":[],"incomingGroups":[],"archivedOutgoingGroups":[],"archivedIncomingGroups":[]}
    112usize
        .checked_add(reveal_identity_wire_bytes(&dto.entity)?)?
        .checked_add(json_opt_str_wire_bytes(dto.type_label.as_deref())?)?
        .checked_add(json_array_wire_bytes(
            &dto.outgoing_groups,
            context_outgoing_group_wire_bytes,
        )?)?
        .checked_add(json_array_wire_bytes(
            &dto.incoming_groups,
            context_incoming_group_wire_bytes,
        )?)?
        .checked_add(json_array_wire_bytes(
            &dto.archived_outgoing_groups,
            context_outgoing_group_wire_bytes,
        )?)?
        .checked_add(json_array_wire_bytes(
            &dto.archived_incoming_groups,
            context_incoming_group_wire_bytes,
        )?)
}

#[tauri::command(async)]
fn entity_context(
    universe_token: String,
    entity_id: String,
    state: State<'_, DesktopState>,
) -> Result<EntityContext, BridgeError> {
    let active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    context_active(&active, &universe_token, &entity_id)
}

#[tauri::command(async)]
fn save_page(
    request: SaveRequest,
    state: State<'_, DesktopState>,
) -> Result<SaveResponse, SaveFailure> {
    let identity = (
        request.page_id.clone(),
        request.session_id.clone(),
        request.request_id,
    );
    let result = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)
        .and_then(|mut active| save_active(&mut active, request));
    result.map_err(|kind| SaveFailure {
        page_id: identity.0,
        session_id: identity.1,
        request_id: identity.2,
        kind,
    })
}

fn save_active(
    active: &mut Option<ActiveUniverse>,
    request: SaveRequest,
) -> Result<SaveResponse, BridgeError> {
    let universe = active.as_mut().ok_or(BridgeError::UniverseNotOpen)?;
    if universe.token != request.universe_token {
        return Err(BridgeError::UniverseChanged);
    }
    save_to_store(&mut universe.store, request)
}

fn save_to_store(store: &mut Store, request: SaveRequest) -> Result<SaveResponse, BridgeError> {
    if request.expected_revision < 0
        || request.expected_revision >= MAX_JS_REVISION
        || request.request_id > MAX_JS_REVISION as u64
    {
        return Err(BridgeError::InvalidInput);
    }
    let id = PageId::parse(&request.page_id).map_err(|_| BridgeError::InvalidInput)?;
    let revision =
        PageRevision::new(request.expected_revision).map_err(|_| BridgeError::InvalidInput)?;
    let current = store.page_document(&id).map_err(save_error)?;
    if current.revision != revision {
        return Err(BridgeError::Stale);
    }
    if current.body.body.contains(['\r', '\0']) || current.title.contains(['\r', '\0']) {
        return Err(BridgeError::ReadOnly);
    }
    if request
        .title
        .as_ref()
        .is_some_and(|title| title.contains(['\r', '\0']))
        || request.expected_body.contains(['\r', '\0'])
    {
        return Err(BridgeError::InvalidInput);
    }
    let mut edits: Vec<PageEdit> = Vec::with_capacity(request.edits.len() + 1);
    if let Some(title) = request.title {
        edits.push(PageEdit::SetTitle { title });
    }
    // An id stays bound to one entity for the stored page and the whole
    // batch. The reducer alone cannot see this once the id has been dropped
    // earlier in the batch.
    let mut bound: HashMap<String, String> = current
        .body
        .mentions
        .iter()
        .map(|held| {
            (
                held.id.as_str().to_owned(),
                held.entity_id.as_str().to_owned(),
            )
        })
        .collect();
    for op in request.edits {
        let edit = match op {
            SaveOp::Replace {
                start_utf16,
                end_utf16,
                inserted_text,
            } => {
                if inserted_text.contains(['\r', '\0']) {
                    return Err(BridgeError::InvalidInput);
                }
                MentionEdit::ReplaceText {
                    start_utf16,
                    end_utf16,
                    inserted_text,
                }
            }
            SaveOp::Link {
                mention_id,
                entity_id,
                start_utf16,
                end_utf16,
            } => {
                let mention = parse_mention(&mention_id, &entity_id, start_utf16, end_utf16)?;
                match bound.get(mention.id.as_str()) {
                    Some(entity) if entity != mention.entity_id.as_str() => {
                        return Err(BridgeError::InvalidInput);
                    }
                    Some(_) => {}
                    None => {
                        bound.insert(
                            mention.id.as_str().to_owned(),
                            mention.entity_id.as_str().to_owned(),
                        );
                    }
                }
                MentionEdit::Link { mention }
            }
            SaveOp::Unlink { mention_id } => MentionEdit::Unlink {
                mention_id: MentionId::parse(&mention_id).map_err(|_| BridgeError::InvalidInput)?,
            },
        };
        edits.push(PageEdit::Mention(edit));
    }
    let expected = request
        .expected_mentions
        .iter()
        .map(|held| parse_mention(&held.id, &held.entity_id, held.start_utf16, held.end_utf16))
        .collect::<Result<Vec<Mention>, BridgeError>>()?;

    // Run the real reducer over the aggregate this request was based on and
    // refuse any disagreement before anything is written.
    let mut predicted = current.body;
    for edit in &edits {
        if let PageEdit::Mention(operation) = edit {
            predicted = apply_mention_edit(&predicted, operation)
                .map_err(save_error)?
                .0;
        }
    }
    let differing = mention_differences(&predicted.mentions, &expected);
    if !differing.is_empty() {
        return Err(BridgeError::MentionMismatch(differing));
    }
    if predicted.body != request.expected_body {
        return Err(BridgeError::BodyMismatch);
    }

    // The store re-reads and re-checks the revision inside its own write
    // transaction, so a writer that moved the page in between makes this
    // stale instead of committing a prediction made from an old aggregate.
    let saved = store
        .save_page_edits(&id, revision, &edits)
        .map_err(save_error)?;
    Ok(SaveResponse {
        page_id: saved.id.as_str().to_owned(),
        session_id: request.session_id,
        request_id: request.request_id,
        title: saved.title,
        body: saved.body.body,
        mentions: wire_mentions(&saved.body.mentions),
        revision: saved.revision.as_i64(),
    })
}

fn parse_mention(
    id: &str,
    entity_id: &str,
    start_utf16: u32,
    end_utf16: u32,
) -> Result<Mention, BridgeError> {
    Ok(Mention {
        id: MentionId::parse(id).map_err(|_| BridgeError::InvalidInput)?,
        entity_id: EntityId::parse(entity_id).map_err(|_| BridgeError::InvalidInput)?,
        start_utf16,
        end_utf16,
    })
}

/// Ids whose final state differs between the reducer and the client: missing
/// on one side, different entity or range, or repeated in the expectation.
fn mention_differences(predicted: &[Mention], expected: &[Mention]) -> Vec<String> {
    let key = |held: &Mention| {
        (
            held.id.as_str().to_owned(),
            held.entity_id.as_str().to_owned(),
            held.start_utf16,
            held.end_utf16,
        )
    };
    let left: BTreeSet<_> = predicted.iter().map(key).collect();
    let right: BTreeSet<_> = expected.iter().map(key).collect();
    let mut ids: BTreeSet<String> = left
        .symmetric_difference(&right)
        .map(|(id, ..)| id.clone())
        .collect();
    let mut seen = BTreeSet::new();
    for held in expected {
        if !seen.insert(held.id.as_str()) {
            ids.insert(held.id.as_str().to_owned());
        }
    }
    ids.into_iter().collect()
}

#[tauri::command(async)]
fn create_page(
    page_id: String,
    universe_token: String,
    state: State<'_, DesktopState>,
) -> Result<PageContent, BridgeError> {
    let mut active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    create_active(&mut active, &universe_token, &page_id)
}
fn create_active(
    active: &mut Option<ActiveUniverse>,
    token: &str,
    page_id: &str,
) -> Result<PageContent, BridgeError> {
    let universe = active.as_mut().ok_or(BridgeError::UniverseNotOpen)?;
    if universe.token != token {
        return Err(BridgeError::UniverseChanged);
    }
    let id = PageId::parse(page_id).map_err(|_| BridgeError::InvalidInput)?;
    let page = universe
        .store
        .create_page(Page {
            id,
            title: "Untitled document".into(),
            body: String::new(),
        })
        .map_err(save_error)?;
    Ok(PageContent {
        id: page.id.as_str().to_owned(),
        title: page.title,
        body: page.body,
        revision: Some(0),
        mentions: Vec::new(),
    })
}

fn consume_close_request(state: &DesktopState, token: &str) -> Result<(), BridgeError> {
    let mut pending = state
        .close_request
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    if pending.as_deref() != Some(token) {
        return Err(BridgeError::InvalidInput);
    }
    *pending = None;
    Ok(())
}

#[tauri::command]
fn cancel_close(token: String, state: State<'_, DesktopState>) -> Result<(), BridgeError> {
    consume_close_request(&state, &token)
}

#[tauri::command]
fn close_window(
    token: String,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<(), BridgeError> {
    consume_close_request(&state, &token)?;
    window.destroy().map_err(|_| BridgeError::StateUnavailable)
}

#[tauri::command]
fn request_window_close(window: tauri::WebviewWindow) -> Result<(), BridgeError> {
    window.close().map_err(|_| BridgeError::StateUnavailable)
}

pub fn run() {
    tauri::Builder::default()
        .manage(DesktopState {
            store: Mutex::new(None),
            close_request: Mutex::new(None),
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let state = window.state::<DesktopState>();
                if let Ok(mut pending) = state.close_request.lock() {
                    if pending.is_none() {
                        let token = uuid::Uuid::new_v4().to_string();
                        *pending = Some(token.clone());
                        if window.emit("request-close", token).is_err() {
                            *pending = None;
                        }
                    }
                };
            }
        })
        .invoke_handler(tauri::generate_handler![
            open_universe,
            page_index,
            read_page,
            entity_index,
            codex_index,
            create_entity_type,
            create_entity,
            entity_reveal,
            entity_context,
            save_page,
            create_page,
            request_window_close,
            cancel_close,
            close_window
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Narrative Universe desktop shell");
}

#[cfg(test)]
mod tests {
    use super::{
        active_store, codex_entries, consume_close_request, create_active, create_entity_active,
        create_type_active, entity_entries, map_open_error, map_read_error, page_content,
        prepare_universe, save_active, save_to_store, wire_mentions, ActiveUniverse, BridgeError,
        CodexEntityEntry, DesktopState, SaveFailure, SaveOp, SaveRequest, SaveResponse,
        TypeIndexEntry, WireMention,
    };
    use narrative_universe::{
        apply_mention_edit, Entity, EntityId, EntityType, Error, Mention, MentionEdit, MentionId,
        Page, PageEdit, PageId, PageRevision, Store, TypeId, UniverseId, ValidationReason,
    };
    use std::path::Path;

    const UNIVERSE: &str = "11111111-1111-4111-8111-111111111111";
    const PAGE: &str = "22222222-2222-4222-8222-222222222222";
    const OTHER_PAGE: &str = "22222222-2222-4222-8222-222222222223";
    const PLACE: &str = "33333333-3333-4333-8333-333333333333";
    const HOUSE: &str = "33333333-3333-4333-8333-333333333334";
    const M1: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const M2: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    const SCENE: &str = "Elara crossed into Valdoria.";

    fn replace(start_utf16: u32, end_utf16: u32, inserted_text: &str) -> SaveOp {
        SaveOp::Replace {
            start_utf16,
            end_utf16,
            inserted_text: inserted_text.into(),
        }
    }

    fn link(id: &str, entity: &str, start_utf16: u32, end_utf16: u32) -> SaveOp {
        SaveOp::Link {
            mention_id: id.into(),
            entity_id: entity.into(),
            start_utf16,
            end_utf16,
        }
    }

    fn wire(id: &str, entity: &str, start_utf16: u32, end_utf16: u32) -> WireMention {
        WireMention {
            id: id.into(),
            entity_id: entity.into(),
            start_utf16,
            end_utf16,
        }
    }

    fn mention(id: &str, entity: &str, start_utf16: u32, end_utf16: u32) -> Mention {
        Mention {
            id: MentionId::parse(id).unwrap(),
            entity_id: EntityId::parse(entity).unwrap(),
            start_utf16,
            end_utf16,
        }
    }

    fn with_ops(revision: i64, edits: Vec<SaveOp>) -> SaveRequest {
        SaveRequest {
            universe_token: "test".into(),
            page_id: PAGE.into(),
            session_id: "session".into(),
            request_id: 1,
            expected_revision: revision,
            title: None,
            edits,
            expected_body: String::new(),
            expected_mentions: Vec::new(),
        }
    }

    fn request(revision: i64, text: &str) -> SaveRequest {
        with_ops(revision, vec![replace(0, 0, text)])
    }

    fn as_edit(op: &SaveOp) -> MentionEdit {
        match op {
            SaveOp::Replace {
                start_utf16,
                end_utf16,
                inserted_text,
            } => MentionEdit::ReplaceText {
                start_utf16: *start_utf16,
                end_utf16: *end_utf16,
                inserted_text: inserted_text.clone(),
            },
            SaveOp::Link {
                mention_id,
                entity_id,
                start_utf16,
                end_utf16,
            } => MentionEdit::Link {
                mention: mention(mention_id, entity_id, *start_utf16, *end_utf16),
            },
            SaveOp::Unlink { mention_id } => MentionEdit::Unlink {
                mention_id: MentionId::parse(mention_id).unwrap(),
            },
        }
    }

    /// Fills the expectation a correct client would send, using the real
    /// reducer over what is stored now.
    fn honest(store: &Store, mut request: SaveRequest) -> SaveRequest {
        let mut body = store
            .page_document(&PageId::parse(&request.page_id).unwrap())
            .unwrap()
            .body;
        for op in &request.edits {
            if let Ok((next, _)) = apply_mention_edit(&body, &as_edit(op)) {
                body = next;
            }
        }
        request.expected_body = body.body;
        request.expected_mentions = wire_mentions(&body.mentions);
        request
    }

    fn save(store: &mut Store, request: SaveRequest) -> Result<SaveResponse, BridgeError> {
        let request = honest(store, request);
        save_to_store(store, request)
    }

    /// A synthetic universe with a linked scene and a second page.
    fn linked_universe(path: &Path) -> Store {
        let mut store = Store::create(path, UniverseId::parse(UNIVERSE).unwrap()).unwrap();
        for (id, title, body) in [(PAGE, "Scene", SCENE), (OTHER_PAGE, "Atlas", "Valdoria")] {
            store
                .create_page(Page {
                    id: PageId::parse(id).unwrap(),
                    title: title.into(),
                    body: body.into(),
                })
                .unwrap();
        }
        for (id, label) in [(PLACE, "Valdoria"), (HOUSE, "Valdoria")] {
            store
                .create_entity(Entity {
                    id: EntityId::parse(id).unwrap(),
                    label: label.into(),
                    type_id: None,
                    page_id: None,
                })
                .unwrap();
        }
        store
            .save_page_edits(
                &PageId::parse(PAGE).unwrap(),
                PageRevision::ZERO,
                &[PageEdit::Mention(MentionEdit::Link {
                    mention: mention(M1, PLACE, 19, 27),
                })],
            )
            .unwrap();
        store
    }

    type Logical = (
        Vec<(String, String, String, i64)>,
        Vec<(String, String, String, i64, i64)>,
        String,
    );

    /// Everything a mismatch must leave untouched: the logical aggregate of
    /// every page and the full snapshot, assertions and canon included.
    fn logical(path: &Path) -> Logical {
        let raw = rusqlite::Connection::open(path).unwrap();
        let pages = raw
            .prepare("SELECT id, title, body, revision FROM pages ORDER BY id")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mentions = raw
            .prepare(
                "SELECT id, page_id, entity_id, start_utf16, end_utf16 FROM mentions ORDER BY id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        drop(raw);
        let snapshot = format!(
            "{:?}",
            Store::open(path).unwrap().export_snapshot().unwrap()
        );
        (pages, mentions, snapshot)
    }

    fn stored(path: &Path) -> (String, String, Vec<WireMention>, i64) {
        let document = Store::open(path)
            .unwrap()
            .page_document(&PageId::parse(PAGE).unwrap())
            .unwrap();
        (
            document.title,
            document.body.body,
            wire_mentions(&document.body.mentions),
            document.revision.as_i64(),
        )
    }

    #[test]
    fn close_permit_is_required_and_single_use() {
        let state = DesktopState {
            store: std::sync::Mutex::new(None),
            close_request: std::sync::Mutex::new(Some("actual".into())),
        };
        assert!(matches!(
            consume_close_request(&state, "wrong"),
            Err(BridgeError::InvalidInput)
        ));
        assert!(consume_close_request(&state, "actual").is_ok());
        assert!(matches!(
            consume_close_request(&state, "actual"),
            Err(BridgeError::InvalidInput)
        ));
    }

    #[test]
    fn lost_open_response_cannot_write_old_draft_to_new_universe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.sqlite");
        let id = PageId::parse(PAGE).unwrap();
        let mut store = Store::create(&path, UniverseId::parse(UNIVERSE).unwrap()).unwrap();
        store
            .create_page(Page {
                id: id.clone(),
                title: "same id".into(),
                body: "new universe".into(),
            })
            .unwrap();
        let mut active = Some(ActiveUniverse {
            token: "new".into(),
            store,
        });
        assert!(matches!(
            active_store(&active, "test"),
            Err(BridgeError::UniverseChanged)
        ));
        assert!(matches!(
            save_active(&mut active, request(0, "old draft")),
            Err(BridgeError::UniverseChanged)
        ));
        drop(active);
        assert_eq!(
            Store::open(&path)
                .unwrap()
                .page_document(&id)
                .unwrap()
                .body
                .body,
            "new universe"
        );
    }

    #[test]
    fn failed_new_index_does_not_touch_the_existing_store() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("old.sqlite");
        let bad_path = dir.path().join("bad.sqlite");
        let universe = UniverseId::parse(UNIVERSE).unwrap();
        let mut old = Store::create(&old_path, universe.clone()).unwrap();
        let id = PageId::parse(PAGE).unwrap();
        old.create_page(Page {
            id: id.clone(),
            title: "old".into(),
            body: "kept".into(),
        })
        .unwrap();
        let bad = Store::create(&bad_path, universe).unwrap();
        drop(bad);
        let raw = rusqlite::Connection::open(&bad_path).unwrap();
        raw.execute(
            "INSERT INTO pages (id,title,body) VALUES (?1,'bad','body')",
            ["not-a-valid-id-xxxxxxxxxxxxxxxxxxxxx"],
        )
        .unwrap();
        drop(raw);
        assert!(prepare_universe(&bad_path).is_err());
        assert_eq!(old.page_document(&id).unwrap().body.body, "kept");
    }

    #[test]
    fn failure_envelope_carries_request_identity_and_stable_kind() {
        let failure = SaveFailure {
            page_id: "page".into(),
            session_id: "session".into(),
            request_id: 7,
            kind: BridgeError::Busy,
        };
        assert_eq!(
            serde_json::to_value(failure).unwrap(),
            serde_json::json!({"pageId":"page","sessionId":"session","requestId":7,"kind":"busy"})
        );
        let mismatch = SaveFailure {
            page_id: "page".into(),
            session_id: "session".into(),
            request_id: 8,
            kind: BridgeError::MentionMismatch(vec![M1.into()]),
        };
        assert_eq!(
            serde_json::to_value(mismatch).unwrap(),
            serde_json::json!({"pageId":"page","sessionId":"session","requestId":8,"kind":"mention_mismatch","message":[M1]})
        );
    }

    #[test]
    fn the_wire_request_is_the_shape_the_client_sends() {
        let request: SaveRequest = serde_json::from_value(serde_json::json!({
            "universeToken": "t", "pageId": PAGE, "sessionId": "s", "requestId": 1,
            "expectedRevision": 0, "title": null,
            "edits": [
                {"kind": "replace", "startUtf16": 0, "endUtf16": 0, "insertedText": "Old "},
                {"kind": "link", "mentionId": M1, "entityId": PLACE, "startUtf16": 23, "endUtf16": 31},
                {"kind": "unlink", "mentionId": M1}
            ],
            "expectedBody": "Old x",
            "expectedMentions": [{"id": M1, "entityId": PLACE, "startUtf16": 23, "endUtf16": 31}]
        }))
        .unwrap();
        assert_eq!(request.edits.len(), 3);
        assert!(matches!(
            &request.edits[1],
            SaveOp::Link {
                start_utf16: 23,
                ..
            }
        ));
        assert_eq!(request.expected_mentions, vec![wire(M1, PLACE, 23, 31)]);
        for bad in [
            serde_json::json!({"kind": "replace", "start_utf16": 0, "endUtf16": 0, "insertedText": ""}),
            serde_json::json!({"kind": "rename", "mentionId": M1}),
            serde_json::json!({"kind": "unlink", "mentionId": M1, "entityId": PLACE}),
        ] {
            assert!(serde_json::from_value::<SaveOp>(bad).is_err());
        }
    }

    #[test]
    fn bridge_saves_reopens_rejects_stale_and_busy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.sqlite");
        let id = PageId::parse(PAGE).unwrap();
        let mut store = Store::create(&path, UniverseId::parse(UNIVERSE).unwrap()).unwrap();
        store
            .create_page(Page {
                id: id.clone(),
                title: "title".into(),
                body: "texto 📚".into(),
            })
            .unwrap();
        let mut combined = request(0, "\u{e1} ");
        combined.title = Some("T\u{ed}tulo \u{1f409}".into());
        let saved = save(&mut store, combined).unwrap();
        assert_eq!(saved.title, "T\u{ed}tulo \u{1f409}");
        assert_eq!(saved.body, "á texto 📚");
        assert_eq!(saved.revision, 1);
        assert!(saved.mentions.is_empty());
        assert_eq!(
            save(&mut store, with_ops(1, Vec::new())).unwrap().revision,
            1
        );
        let mut invalid = with_ops(1, vec![replace(10_000, 10_000, "x")]);
        invalid.title = Some("rollback".into());
        assert!(matches!(
            save(&mut store, invalid),
            Err(BridgeError::InvalidInput)
        ));
        assert!(matches!(
            save(&mut store, request(0, "x")),
            Err(BridgeError::Stale)
        ));
        assert_eq!(
            store.page_document(&id).unwrap().title,
            "T\u{ed}tulo \u{1f409}"
        );
        let busy = honest(&store, request(1, "x"));
        let lock = rusqlite::Connection::open(&path).unwrap();
        lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
        assert!(matches!(
            save_to_store(&mut store, busy),
            Err(BridgeError::Busy)
        ));
        lock.execute_batch("ROLLBACK").unwrap();
        drop(store);
        assert_eq!(
            Store::open(&path)
                .unwrap()
                .page_document(&id)
                .unwrap()
                .body
                .body,
            "á texto 📚"
        );
    }

    #[test]
    fn creation_checks_token_duplicate_and_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("create.sqlite");
        let store = Store::create(&path, UniverseId::parse(UNIVERSE).unwrap()).unwrap();
        let mut active = Some(ActiveUniverse {
            token: "current".into(),
            store,
        });
        assert!(matches!(
            create_active(&mut active, "old", PAGE),
            Err(BridgeError::UniverseChanged)
        ));
        assert!(matches!(
            create_active(&mut active, "current", "bad"),
            Err(BridgeError::InvalidInput)
        ));
        let created = create_active(&mut active, "current", PAGE).unwrap();
        assert_eq!(created.revision, Some(0));
        assert_eq!(created.title, "Untitled document");
        assert!(created.mentions.is_empty());
        assert_eq!(
            active.as_ref().unwrap().store.page_index().unwrap()[0].title,
            "Untitled document"
        );
        assert!(matches!(
            create_active(&mut active, "current", PAGE),
            Err(BridgeError::DuplicateId)
        ));
        let lock = rusqlite::Connection::open(&path).unwrap();
        lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
        assert!(matches!(
            create_active(&mut active, "current", OTHER_PAGE),
            Err(BridgeError::Busy)
        ));
        lock.execute_batch("ROLLBACK").unwrap();
        assert_eq!(
            active.as_ref().unwrap().store.page_index().unwrap().len(),
            1
        );
    }

    #[test]
    fn bridge_refuses_unsupported_text_even_on_linked_pages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.sqlite");
        let mut store = Store::create(&path, UniverseId::parse(UNIVERSE).unwrap()).unwrap();
        store
            .create_page(Page {
                id: PageId::parse(PAGE).unwrap(),
                title: "title".into(),
                body: "one\r\ntwo Valdoria".into(),
            })
            .unwrap();
        store
            .create_entity(Entity {
                id: EntityId::parse(PLACE).unwrap(),
                label: "Valdoria".into(),
                type_id: None,
                page_id: None,
            })
            .unwrap();
        store
            .save_page_edits(
                &PageId::parse(PAGE).unwrap(),
                PageRevision::ZERO,
                &[PageEdit::Mention(MentionEdit::Link {
                    mention: mention(M1, PLACE, 9, 17),
                })],
            )
            .unwrap();
        assert!(matches!(
            save(&mut store, request(1, "x")),
            Err(BridgeError::ReadOnly)
        ));
        drop(store);

        let linked = dir.path().join("linked.sqlite");
        let mut store = linked_universe(&linked);
        let before = logical(&linked);
        for text in ["\r", "a\0"] {
            assert!(matches!(
                save(&mut store, request(1, text)),
                Err(BridgeError::InvalidInput)
            ));
            let mut title = with_ops(1, Vec::new());
            title.title = Some(format!("t{text}"));
            assert!(matches!(
                save(&mut store, title),
                Err(BridgeError::InvalidInput)
            ));
        }
        assert!(matches!(
            save(&mut store, request(i64::MAX, "x")),
            Err(BridgeError::InvalidInput)
        ));
        drop(store);
        assert_eq!(logical(&linked), before);
    }

    #[test]
    fn read_page_returns_the_complete_canonical_mention_set() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("read.sqlite");
        let mut store = linked_universe(&path);
        save(&mut store, with_ops(1, vec![link(M2, HOUSE, 0, 5)])).unwrap();
        let content = page_content(&store, PAGE).unwrap();
        assert_eq!(content.revision, Some(2));
        assert_eq!(
            content.mentions,
            vec![wire(M2, HOUSE, 0, 5), wire(M1, PLACE, 19, 27)]
        );
        assert_eq!(
            serde_json::to_value(&content).unwrap()["mentions"][0],
            serde_json::json!({"id": M2, "entityId": HOUSE, "startUtf16": 0, "endUtf16": 5})
        );
        assert!(matches!(
            page_content(&store, "bad"),
            Err(BridgeError::PageMissing)
        ));
    }

    #[test]
    fn linked_page_saves_link_unlink_and_text_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linked.sqlite");
        let mut store = linked_universe(&path);

        let mut combined = with_ops(
            1,
            vec![
                replace(0, 0, "Old "),
                link(M2, HOUSE, 4, 9),
                replace(31, 31, "n"),
            ],
        );
        combined.title = Some("Scene \u{1F409}".into());
        let saved = save(&mut store, combined).unwrap();
        assert_eq!(saved.body, "Old Elara crossed into Valdorian.");
        assert_eq!(saved.title, "Scene \u{1F409}");
        assert_eq!(saved.revision, 2);
        assert_eq!(
            saved.mentions,
            vec![wire(M2, HOUSE, 4, 9), wire(M1, PLACE, 23, 31)]
        );

        let saved = save(
            &mut store,
            with_ops(
                2,
                vec![SaveOp::Unlink {
                    mention_id: M2.into(),
                }],
            ),
        )
        .unwrap();
        assert_eq!(saved.mentions, vec![wire(M1, PLACE, 23, 31)]);
        drop(store);
        assert_eq!(
            stored(&path),
            (
                "Scene \u{1F409}".into(),
                "Old Elara crossed into Valdorian.".into(),
                vec![wire(M1, PLACE, 23, 31)],
                3
            )
        );
    }

    #[test]
    fn edits_around_a_link_follow_the_reducer_and_survive_reopen() {
        let cases: [(&str, Vec<SaveOp>, &str, Vec<WireMention>); 6] = [
            (
                "before",
                vec![replace(0, 0, "Old ")],
                "Old Elara crossed into Valdoria.",
                vec![wire(M1, PLACE, 23, 31)],
            ),
            (
                "start",
                vec![replace(19, 19, "the ")],
                "Elara crossed into the Valdoria.",
                vec![wire(M1, PLACE, 23, 31)],
            ),
            (
                "end",
                vec![replace(27, 27, "n")],
                "Elara crossed into Valdorian.",
                vec![wire(M1, PLACE, 19, 27)],
            ),
            (
                "inside",
                vec![replace(22, 22, "x")],
                "Elara crossed into Valxdoria.",
                vec![],
            ),
            (
                "cover",
                vec![replace(18, 28, "")],
                "Elara crossed into",
                vec![],
            ),
            (
                "multi_range",
                vec![
                    replace(0, 0, "Dear "),
                    replace(12, 12, "X"),
                    replace(33, 33, "!"),
                ],
                "Dear Elara cXrossed into Valdoria!.",
                vec![wire(M1, PLACE, 25, 33)],
            ),
        ];
        for (name, ops, body, mentions) in cases {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("{name}.sqlite"));
            let mut store = linked_universe(&path);
            let saved = save(&mut store, with_ops(1, ops))
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert_eq!(saved.body, body, "{name}");
            assert_eq!(saved.mentions, mentions, "{name}");
            assert_eq!(saved.revision, 2, "{name}");
            drop(store);
            let (_, reopened_body, reopened_mentions, revision) = stored(&path);
            assert_eq!(
                (reopened_body.as_str(), reopened_mentions, revision),
                (body, mentions, 2),
                "{name} reopen"
            );
        }
    }

    #[test]
    fn undo_relinks_a_dropped_id_and_keeps_it_after_a_committed_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("undo.sqlite");
        let mut store = linked_universe(&path);
        let restored = save(
            &mut store,
            with_ops(
                1,
                vec![
                    replace(22, 23, ""),
                    replace(22, 22, "d"),
                    link(M1, PLACE, 19, 27),
                ],
            ),
        )
        .unwrap();
        assert_eq!(restored.revision, 1, "the aggregate is unchanged");
        assert_eq!(restored.mentions, vec![wire(M1, PLACE, 19, 27)]);

        let dropped = save(&mut store, with_ops(1, vec![replace(22, 23, "")])).unwrap();
        assert!(dropped.mentions.is_empty());
        let relinked = save(
            &mut store,
            with_ops(2, vec![replace(22, 22, "d"), link(M1, PLACE, 19, 27)]),
        )
        .unwrap();
        assert_eq!(relinked.mentions, vec![wire(M1, PLACE, 19, 27)]);
        drop(store);
        assert_eq!(stored(&path).2, vec![wire(M1, PLACE, 19, 27)]);
    }

    #[test]
    fn a_prediction_mismatch_writes_nothing_and_names_the_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mismatch.sqlite");
        let mut store = linked_universe(&path);
        let before = logical(&path);

        let mut kept = honest(&store, with_ops(1, vec![replace(22, 22, "x")]));
        kept.expected_mentions = vec![wire(M1, PLACE, 19, 28)];
        kept.title = Some("not written".into());
        match save_to_store(&mut store, kept) {
            Err(BridgeError::MentionMismatch(ids)) => assert_eq!(ids, vec![M1.to_string()]),
            other => panic!("expected mismatch, got {other:?}"),
        }

        let variants: [Vec<WireMention>; 4] = [
            vec![],
            vec![wire(M1, HOUSE, 23, 31)],
            vec![wire(M1, PLACE, 23, 31), wire(M2, PLACE, 0, 5)],
            vec![wire(M1, PLACE, 23, 31), wire(M1, PLACE, 23, 31)],
        ];
        for expected in variants {
            let mut wrong = honest(&store, with_ops(1, vec![replace(0, 0, "Old ")]));
            wrong.expected_mentions = expected;
            assert!(matches!(
                save_to_store(&mut store, wrong),
                Err(BridgeError::MentionMismatch(_))
            ));
        }

        let mut body = honest(&store, with_ops(1, vec![replace(0, 0, "Old ")]));
        body.expected_body = "Old Elara crossed into Valdoria!".into();
        assert!(matches!(
            save_to_store(&mut store, body),
            Err(BridgeError::BodyMismatch)
        ));

        assert_eq!(logical(&path), before);

        let mut ordered = honest(&store, with_ops(1, vec![link(M2, HOUSE, 0, 5)]));
        ordered.expected_mentions.reverse();
        assert_eq!(
            save_to_store(&mut store, ordered).unwrap().revision,
            2,
            "order is not identity"
        );
    }

    #[test]
    fn a_mismatch_leaves_the_logical_aggregate_byte_for_byte_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exact.sqlite");
        let mut store = linked_universe(&path);
        let before = logical(&path);
        let bytes = std::fs::read(&path).unwrap();
        let mut wrong = honest(&store, with_ops(1, vec![replace(27, 27, "\u{301}")]));
        wrong.expected_mentions = vec![wire(M1, PLACE, 19, 27)];
        wrong.title = Some("changed".into());
        assert!(matches!(
            save_to_store(&mut store, wrong),
            Err(BridgeError::MentionMismatch(_))
        ));
        drop(store);
        assert_eq!(logical(&path), before);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn identity_rules_refuse_conflicts_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.sqlite");
        let mut store = linked_universe(&path);
        store
            .save_page_edits(
                &PageId::parse(OTHER_PAGE).unwrap(),
                PageRevision::ZERO,
                &[PageEdit::Mention(MentionEdit::Link {
                    mention: mention(M2, HOUSE, 0, 8),
                })],
            )
            .unwrap();
        let before = logical(&path);
        let refusals: [(&str, Vec<SaveOp>); 5] = [
            ("id owned by another page", vec![link(M2, HOUSE, 0, 5)]),
            (
                "stored id reused for another entity",
                vec![replace(22, 23, ""), link(M1, HOUSE, 0, 5)],
            ),
            (
                "batch id reused for another entity",
                vec![
                    link("cccccccc-cccc-4ccc-8ccc-cccccccccccc", PLACE, 0, 5),
                    SaveOp::Unlink {
                        mention_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(),
                    },
                    link("cccccccc-cccc-4ccc-8ccc-cccccccccccc", HOUSE, 0, 5),
                ],
            ),
            (
                "unknown entity",
                vec![link(
                    "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
                    "99999999-9999-4999-8999-999999999999",
                    0,
                    5,
                )],
            ),
            (
                "split grapheme",
                vec![
                    replace(5, 5, "e\u{301}"),
                    link("cccccccc-cccc-4ccc-8ccc-cccccccccccc", PLACE, 5, 6),
                ],
            ),
        ];
        for (name, ops) in refusals {
            let mut attempt = honest(&store, with_ops(1, ops));
            attempt.title = Some("not written".into());
            assert!(
                matches!(
                    save_to_store(&mut store, attempt),
                    Err(BridgeError::InvalidInput)
                ),
                "{name}"
            );
        }
        for bad in [
            link("not-an-id", PLACE, 0, 5),
            link(M2, "not-an-id", 0, 5),
            SaveOp::Unlink {
                mention_id: "x".into(),
            },
        ] {
            assert!(matches!(
                save_to_store(&mut store, with_ops(1, vec![bad])),
                Err(BridgeError::InvalidInput)
            ));
        }
        drop(store);
        assert_eq!(logical(&path), before);
    }

    #[test]
    fn stale_and_busy_link_saves_write_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stale.sqlite");
        let mut store = linked_universe(&path);
        let mut other = Store::open(&path).unwrap();
        other
            .save_page_edits(
                &PageId::parse(PAGE).unwrap(),
                PageRevision::new(1).unwrap(),
                &[PageEdit::SetTitle {
                    title: "elsewhere".into(),
                }],
            )
            .unwrap();
        drop(other);
        let before = logical(&path);
        assert!(matches!(
            save(&mut store, with_ops(1, vec![link(M2, HOUSE, 0, 5)])),
            Err(BridgeError::Stale)
        ));
        let busy = honest(&store, with_ops(2, vec![link(M2, HOUSE, 0, 5)]));
        let lock = rusqlite::Connection::open(&path).unwrap();
        lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
        assert!(matches!(
            save_to_store(&mut store, busy),
            Err(BridgeError::Busy)
        ));
        lock.execute_batch("ROLLBACK").unwrap();
        drop(lock);
        drop(store);
        assert_eq!(logical(&path), before);
    }

    #[test]
    fn entity_index_lists_duplicate_labels_separately_with_optional_relations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("entities.sqlite");
        let mut store = linked_universe(&path);
        store
            .create_entity_type(EntityType {
                id: TypeId::parse("44444444-4444-4444-8444-444444444444").unwrap(),
                label: "Place".into(),
            })
            .unwrap();
        store
            .create_entity(Entity {
                id: EntityId::parse("33333333-3333-4333-8333-333333333332").unwrap(),
                label: "Valdoria".into(),
                type_id: Some(TypeId::parse("44444444-4444-4444-8444-444444444444").unwrap()),
                page_id: Some(PageId::parse(OTHER_PAGE).unwrap()),
            })
            .unwrap();
        let json = serde_json::to_value(entity_entries(&store).unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                {"id": "33333333-3333-4333-8333-333333333332", "label": "Valdoria", "typeLabel": "Place", "primaryPageTitle": "Atlas"},
                {"id": PLACE, "label": "Valdoria", "typeLabel": null, "primaryPageTitle": null},
                {"id": HOUSE, "label": "Valdoria", "typeLabel": null, "primaryPageTitle": null}
            ])
        );
    }

    #[test]
    fn reveal_is_token_scoped_typed_read_only_and_sanitizes_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reveal.sqlite");
        let (store, _) = narrative_universe::import_new_json(
            &path,
            include_bytes!("../../../tests/fixtures/reveal-facts.json"),
        )
        .unwrap();
        let active = Some(super::ActiveUniverse {
            token: "reveal-token".into(),
            store,
        });
        let id = "00000000-0000-4000-8000-000000000011";
        assert!(matches!(
            super::reveal_active(&active, "old-token", id),
            Err(BridgeError::UniverseChanged)
        ));
        assert!(matches!(
            super::reveal_active(&None, "reveal-token", id),
            Err(BridgeError::UniverseNotOpen)
        ));
        for missing in ["invalid", "00000000-0000-4000-8000-000000000099"] {
            assert!(matches!(
                super::reveal_active(&active, "reveal-token", missing),
                Err(BridgeError::EntityMissing)
            ));
        }
        let before = std::fs::read(&path).unwrap();
        let json = serde_json::to_value(super::reveal_active(&active, "reveal-token", id).unwrap())
            .unwrap();
        assert_eq!(json["entity"]["label"], "Mara Vey");
        assert_eq!(
            json["outgoing"][0]["value"],
            serde_json::json!({"kind":"reference", "target":{"id":"00000000-0000-4000-8000-000000000013","label":"Ashfall Order"}})
        );
        assert_eq!(json["outgoing"][0]["canon"], "PROVISIONAL");
        assert_eq!(
            json["outgoing"][0]["provenance"],
            serde_json::json!({"externalRef":"chronicle-a","locator":"folio 3","fragment":null,"sourceRevision":null})
        );
        assert!(json["outgoing"][0]["scopeNote"].is_null());
        assert_eq!(json["outgoing"][0]["sourceStatusRaw"], "draft chapter 3");
        assert_eq!(json["outgoing"][5]["value"]["kind"], "unknown");
        assert!(json["outgoing"][5]["propertyLabel"].is_null());
        let corin = serde_json::to_value(
            super::reveal_active(
                &active,
                "reveal-token",
                "00000000-0000-4000-8000-000000000012",
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            corin["outgoing"][1]["value"],
            serde_json::json!({"kind":"boolean", "value":false})
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("BEGIN EXCLUSIVE").unwrap();
        assert!(matches!(
            super::reveal_active(&active, "reveal-token", id),
            Err(BridgeError::Busy)
        ));
        conn.execute_batch("ROLLBACK; PRAGMA ignore_check_constraints=ON; UPDATE assertions SET canon='private invalid content' WHERE id='00000000-0000-4000-8000-000000000041'").unwrap();
        let error = super::reveal_active(&active, "reveal-token", id).unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({"kind":"read_failed"})
        );
    }

    #[test]
    fn context_is_token_scoped_grouped_archive_preserving_bounded_and_sanitizes_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("context.sqlite");
        let (store, _) = narrative_universe::import_new_json(
            &path,
            include_bytes!("../../../tests/fixtures/reveal-facts.json"),
        )
        .unwrap();
        let active = Some(super::ActiveUniverse {
            token: "context-token".into(),
            store,
        });
        let mara_id = "00000000-0000-4000-8000-000000000011";
        assert!(matches!(
            super::context_active(&active, "old-token", mara_id),
            Err(BridgeError::UniverseChanged)
        ));
        assert!(matches!(
            super::context_active(&None, "context-token", mara_id),
            Err(BridgeError::UniverseNotOpen)
        ));
        for missing in ["invalid", "00000000-0000-4000-8000-000000000099"] {
            assert!(matches!(
                super::context_active(&active, "context-token", missing),
                Err(BridgeError::EntityMissing)
            ));
        }

        let before = std::fs::read(&path).unwrap();
        let mara_dto = super::context_active(&active, "context-token", mara_id).unwrap();
        let mara_bytes = serde_json::to_vec(&mara_dto).unwrap();
        assert!(mara_bytes.len() <= narrative_universe::MAX_CONTEXT_BYTES);
        let mara = serde_json::to_value(&mara_dto).unwrap();
        assert_eq!(mara["entity"]["label"], "Mara Vey");
        assert_eq!(mara["typeLabel"], "Person");
        let outgoing = mara["outgoingGroups"].as_array().unwrap();
        assert_eq!(outgoing.len(), 7);
        assert_eq!(
            outgoing[0]["definition"],
            serde_json::json!({
                "id": "00000000-0000-4000-8000-000000000031",
                "ownerKind": "type",
                "ownerId": "00000000-0000-4000-8000-000000000021",
                "propertyLabel": "member of",
                "valueClass": "reference",
                "cardinality": "multiple",
                "archived": false
            })
        );
        // Active zero-claim applicable definitions remain present.
        assert_eq!(
            outgoing[2]["definition"]["id"],
            "00000000-0000-4000-8000-000000000033"
        );
        assert_eq!(outgoing[2]["claims"], serde_json::json!([]));
        assert_eq!(outgoing[2]["hasArchivedClaims"], false);
        assert_eq!(
            outgoing[6]["definition"]["id"],
            "00000000-0000-4000-8000-000000000038"
        );
        assert_eq!(
            outgoing[6]["claims"][0]["value"],
            serde_json::json!({"kind":"reference","target":{"id":"00000000-0000-4000-8000-000000000013","label":"Ashfall Order"}})
        );
        assert_eq!(outgoing[6]["claims"][0]["archived"], false);

        let archived_out = mara["archivedOutgoingGroups"].as_array().unwrap();
        assert_eq!(archived_out.len(), 1);
        assert_eq!(
            archived_out[0]["definition"],
            serde_json::json!({
                "id": "00000000-0000-4000-8000-000000000036",
                "ownerKind": "entity",
                "ownerId": mara_id,
                "propertyLabel": "archived sigil",
                "valueClass": "text",
                "cardinality": "singular",
                "archived": true
            })
        );
        assert_eq!(archived_out[0]["claims"][0]["archived"], true);

        let corin_dto = super::context_active(
            &active,
            "context-token",
            "00000000-0000-4000-8000-000000000012",
        )
        .unwrap();
        let corin = serde_json::to_value(&corin_dto).unwrap();
        assert_eq!(
            corin["outgoingGroups"][2]["claims"][0]["value"],
            serde_json::json!({"kind":"boolean","value":false})
        );
        assert_eq!(
            corin["outgoingGroups"][4]["claims"][0]["value"],
            serde_json::json!({"kind":"unknown"})
        );
        assert_eq!(corin["outgoingGroups"][3]["hasArchivedClaims"], false);
        assert_eq!(corin["archivedOutgoingGroups"], serde_json::json!([]));
        assert_eq!(std::fs::read(&path).unwrap(), before);

        // Synthetic wire-budget, independent archive bits, zero-claim archived def, and ContextTooLarge checks.
        let large_path = dir.path().join("large.sqlite");
        let mut large_store =
            Store::create(&large_path, UniverseId::parse(UNIVERSE).unwrap()).unwrap();
        large_store
            .create_entity(Entity {
                id: EntityId::parse(PLACE).unwrap(),
                label: "Quoted".repeat(10),
                type_id: None,
                page_id: None,
            })
            .unwrap();
        let def_id =
            narrative_universe::DefinitionId::parse("44444444-4444-4444-8444-444444444441")
                .unwrap();
        large_store
            .create_definition(narrative_universe::Definition {
                id: def_id.clone(),
                owner: narrative_universe::Owner::Entity(EntityId::parse(PLACE).unwrap()),
                label: Some(narrative_universe::DefinitionLabel::parse("notes").unwrap()),
                value_class: narrative_universe::ValueClass::Text,
                cardinality: narrative_universe::Cardinality::Multiple,
                archived: false,
            })
            .unwrap();
        let empty_archived_def =
            narrative_universe::DefinitionId::parse("44444444-4444-4444-8444-444444444442")
                .unwrap();
        large_store
            .create_definition(narrative_universe::Definition {
                id: empty_archived_def,
                owner: narrative_universe::Owner::Entity(EntityId::parse(PLACE).unwrap()),
                label: Some(narrative_universe::DefinitionLabel::parse("old rule").unwrap()),
                value_class: narrative_universe::ValueClass::Boolean,
                cardinality: narrative_universe::Cardinality::Singular,
                archived: true,
            })
            .unwrap();
        let quotes_ok = "\"".repeat(900_000);
        large_store
            .create_assertion(narrative_universe::Assertion {
                id: narrative_universe::AssertionId::parse("55555555-5555-4555-8555-555555555551")
                    .unwrap(),
                subject_id: EntityId::parse(PLACE).unwrap(),
                definition_id: def_id.clone(),
                value: narrative_universe::Value::Text(quotes_ok),
                canon: narrative_universe::Canon::Acordado,
                source_status_raw: None,
                provenance: narrative_universe::Provenance::absent(),
                scope_note: None,
                archived: false,
            })
            .unwrap();
        large_store
            .create_assertion(narrative_universe::Assertion {
                id: narrative_universe::AssertionId::parse("55555555-5555-4555-8555-555555555550")
                    .unwrap(),
                subject_id: EntityId::parse(PLACE).unwrap(),
                definition_id: def_id.clone(),
                value: narrative_universe::Value::Text("archived note".into()),
                canon: narrative_universe::Canon::Provisional,
                source_status_raw: None,
                provenance: narrative_universe::Provenance::absent(),
                scope_note: None,
                archived: true,
            })
            .unwrap();
        let large_active = Some(super::ActiveUniverse {
            token: "large".into(),
            store: large_store,
        });
        let ok_dto = super::context_active(&large_active, "large", PLACE).unwrap();
        assert!(
            serde_json::to_vec(&ok_dto).unwrap().len() <= narrative_universe::MAX_CONTEXT_BYTES
        );
        let ok_json = serde_json::to_value(&ok_dto).unwrap();
        assert_eq!(ok_json["outgoingGroups"][0]["hasArchivedClaims"], true);
        assert_eq!(
            ok_json["archivedOutgoingGroups"][0]["definition"]["archived"],
            false
        );
        assert_eq!(
            ok_json["archivedOutgoingGroups"][0]["claims"][0]["archived"],
            true
        );
        assert_eq!(
            ok_json["archivedOutgoingGroups"][1]["definition"]["archived"],
            true
        );
        assert_eq!(
            ok_json["archivedOutgoingGroups"][1]["claims"],
            serde_json::json!([])
        );
        drop(large_active);

        let mut large_store = Store::open(&large_path).unwrap();
        large_store
            .create_assertion(narrative_universe::Assertion {
                id: narrative_universe::AssertionId::parse("55555555-5555-4555-8555-555555555552")
                    .unwrap(),
                subject_id: EntityId::parse(PLACE).unwrap(),
                definition_id: def_id,
                value: narrative_universe::Value::Text("\"".repeat(300_000)),
                canon: narrative_universe::Canon::Acordado,
                source_status_raw: None,
                provenance: narrative_universe::Provenance::absent(),
                scope_note: None,
                archived: false,
            })
            .unwrap();
        let too_large_active = Some(super::ActiveUniverse {
            token: "large".into(),
            store: large_store,
        });
        let too_large = super::context_active(&too_large_active, "large", PLACE).unwrap_err();
        assert!(matches!(too_large, BridgeError::ContextTooLarge));
        assert_eq!(
            serde_json::to_value(too_large).unwrap(),
            serde_json::json!({"kind":"context_too_large"})
        );

        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("BEGIN EXCLUSIVE").unwrap();
        assert!(matches!(
            super::context_active(&active, "context-token", mara_id),
            Err(BridgeError::Busy)
        ));
        conn.execute_batch("ROLLBACK; PRAGMA ignore_check_constraints=ON; UPDATE assertions SET canon='private invalid content' WHERE id='00000000-0000-4000-8000-000000000041'").unwrap();
        let error = super::context_active(&active, "context-token", mara_id).unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({"kind":"read_failed"})
        );
    }

    #[test]
    fn context_wire_bound_holds_at_exact_2_mib_boundary_for_zero_claim_and_mixed_dto_variants() {
        let dir = tempfile::tempdir().unwrap();
        let universe = UniverseId::parse(UNIVERSE).unwrap();
        let e1 = EntityId::parse("33333333-3333-4333-8333-333333333301").unwrap();

        // 1. Ari's 128-group zero-claim case: untyped entity, 128 active entity-owned
        // reference/multiple definitions labeled "x".
        let defs_128: Vec<narrative_universe::Definition> = (1
            ..=narrative_universe::MAX_CONTEXT_GROUPS)
            .map(|i| narrative_universe::Definition {
                id: narrative_universe::DefinitionId::parse(&format!(
                    "44444444-4444-4444-8444-{i:012x}"
                ))
                .unwrap(),
                owner: narrative_universe::Owner::Entity(e1.clone()),
                value_class: narrative_universe::ValueClass::Reference,
                cardinality: narrative_universe::Cardinality::Multiple,
                label: Some(narrative_universe::DefinitionLabel::parse("x").unwrap()),
                archived: false,
            })
            .collect();

        // Root (174 + 2 + 4 = 180) + 128 * 255 + 127 commas = 32_947 -> 2_097_152 - 32_947 = 2_064_205.
        let exact_fit_len = 2_064_205usize;
        for (idx, (label_len, should_fit)) in [
            (exact_fit_len, true),
            (exact_fit_len + 1, false),
            (2_064_591, false),
        ]
        .into_iter()
        .enumerate()
        {
            let db_path = dir.path().join(format!("boundary-128-{idx}.sqlite"));
            let (store, _) = Store::import_new(
                &db_path,
                narrative_universe::Snapshot {
                    universe_id: universe.clone(),
                    pages: vec![],
                    types: vec![],
                    entities: vec![Entity {
                        id: e1.clone(),
                        label: "a".repeat(label_len),
                        type_id: None,
                        page_id: None,
                    }],
                    definitions: defs_128.clone(),
                    assertions: vec![],
                },
            )
            .unwrap();
            let active = Some(super::ActiveUniverse {
                token: "t".into(),
                store,
            });
            let res = super::context_active(&active, "t", e1.as_str());
            if should_fit {
                let dto = res.unwrap();
                let serialized_len = serde_json::to_vec(&dto).unwrap().len();
                assert_eq!(super::context_dto_wire_bytes(&dto), Some(serialized_len));
                assert_eq!(serialized_len, narrative_universe::MAX_CONTEXT_BYTES);
            } else {
                assert!(
                    matches!(res, Err(BridgeError::ContextTooLarge)),
                    "label_len={label_len} should exceed 2 MiB wire limit"
                );
            }
        }

        // 2. Mixed typed entity with outgoing/incoming/self-ref/archived, booleans,
        // unknown, provenance, and large text padded up to the exact budget boundary.
        let t_person = TypeId::parse("22222222-2222-4222-8222-222222222201").unwrap();
        let e2 = EntityId::parse("33333333-3333-4333-8333-333333333302").unwrap();
        let d_text =
            narrative_universe::DefinitionId::parse("44444444-4444-4444-8444-000000000001")
                .unwrap();
        let d_bool =
            narrative_universe::DefinitionId::parse("44444444-4444-4444-8444-000000000002")
                .unwrap();
        let d_ref = narrative_universe::DefinitionId::parse("44444444-4444-4444-8444-000000000003")
            .unwrap();
        let d_arch =
            narrative_universe::DefinitionId::parse("44444444-4444-4444-8444-000000000004")
                .unwrap();

        let build_mixed_store = |db_name: &str, text_payload: String| {
            let db_path = dir.path().join(db_name);
            Store::import_new(
                &db_path,
                narrative_universe::Snapshot {
                    universe_id: universe.clone(),
                    pages: vec![],
                    types: vec![EntityType {
                        id: t_person.clone(),
                        label: "Person".into(),
                    }],
                    entities: vec![
                        Entity {
                            id: e1.clone(),
                            label: "Alden".into(),
                            type_id: Some(t_person.clone()),
                            page_id: None,
                        },
                        Entity {
                            id: e2.clone(),
                            label: "Bram".into(),
                            type_id: Some(t_person.clone()),
                            page_id: None,
                        },
                    ],
                    definitions: vec![
                        narrative_universe::Definition {
                            id: d_text.clone(),
                            owner: narrative_universe::Owner::Type(t_person.clone()),
                            value_class: narrative_universe::ValueClass::Text,
                            cardinality: narrative_universe::Cardinality::Multiple,
                            label: Some(
                                narrative_universe::DefinitionLabel::parse("notes").unwrap(),
                            ),
                            archived: false,
                        },
                        narrative_universe::Definition {
                            id: d_bool.clone(),
                            owner: narrative_universe::Owner::Type(t_person.clone()),
                            value_class: narrative_universe::ValueClass::Boolean,
                            cardinality: narrative_universe::Cardinality::Multiple,
                            label: Some(
                                narrative_universe::DefinitionLabel::parse("oath bound").unwrap(),
                            ),
                            archived: false,
                        },
                        narrative_universe::Definition {
                            id: d_ref.clone(),
                            owner: narrative_universe::Owner::Entity(e1.clone()),
                            value_class: narrative_universe::ValueClass::Reference,
                            cardinality: narrative_universe::Cardinality::Multiple,
                            label: None,
                            archived: false,
                        },
                        narrative_universe::Definition {
                            id: d_arch.clone(),
                            owner: narrative_universe::Owner::Entity(e1.clone()),
                            value_class: narrative_universe::ValueClass::Text,
                            cardinality: narrative_universe::Cardinality::Singular,
                            label: Some(
                                narrative_universe::DefinitionLabel::parse("retired").unwrap(),
                            ),
                            archived: true,
                        },
                    ],
                    assertions: vec![
                        narrative_universe::Assertion {
                            id: narrative_universe::AssertionId::parse(
                                "55555555-5555-4555-8555-000000000001",
                            )
                            .unwrap(),
                            subject_id: e1.clone(),
                            definition_id: d_text.clone(),
                            value: narrative_universe::Value::Text(text_payload),
                            canon: narrative_universe::Canon::Acordado,
                            source_status_raw: Some("draft".into()),
                            provenance: narrative_universe::Provenance {
                                external_ref: Some("ext".into()),
                                locator: Some("loc".into()),
                                fragment: Some("frag".into()),
                                source_revision: Some("rev".into()),
                            },
                            scope_note: Some("note".into()),
                            archived: false,
                        },
                        narrative_universe::Assertion {
                            id: narrative_universe::AssertionId::parse(
                                "55555555-5555-4555-8555-000000000002",
                            )
                            .unwrap(),
                            subject_id: e1.clone(),
                            definition_id: d_text.clone(),
                            value: narrative_universe::Value::Unknown,
                            canon: narrative_universe::Canon::Abierto,
                            source_status_raw: None,
                            provenance: narrative_universe::Provenance::absent(),
                            scope_note: None,
                            archived: true,
                        },
                        narrative_universe::Assertion {
                            id: narrative_universe::AssertionId::parse(
                                "55555555-5555-4555-8555-000000000003",
                            )
                            .unwrap(),
                            subject_id: e1.clone(),
                            definition_id: d_bool.clone(),
                            value: narrative_universe::Value::Boolean(false),
                            canon: narrative_universe::Canon::Provisional,
                            source_status_raw: None,
                            provenance: narrative_universe::Provenance::absent(),
                            scope_note: None,
                            archived: false,
                        },
                        narrative_universe::Assertion {
                            id: narrative_universe::AssertionId::parse(
                                "55555555-5555-4555-8555-000000000004",
                            )
                            .unwrap(),
                            subject_id: e1.clone(),
                            definition_id: d_bool.clone(),
                            value: narrative_universe::Value::Boolean(true),
                            canon: narrative_universe::Canon::Acordado,
                            source_status_raw: None,
                            provenance: narrative_universe::Provenance::absent(),
                            scope_note: None,
                            archived: false,
                        },
                        narrative_universe::Assertion {
                            id: narrative_universe::AssertionId::parse(
                                "55555555-5555-4555-8555-000000000005",
                            )
                            .unwrap(),
                            subject_id: e1.clone(),
                            definition_id: d_ref.clone(),
                            value: narrative_universe::Value::Reference(e1.clone()),
                            canon: narrative_universe::Canon::MisterioIntencional,
                            source_status_raw: None,
                            provenance: narrative_universe::Provenance::absent(),
                            scope_note: None,
                            archived: false,
                        },
                        narrative_universe::Assertion {
                            id: narrative_universe::AssertionId::parse(
                                "55555555-5555-4555-8555-000000000006",
                            )
                            .unwrap(),
                            subject_id: e1.clone(),
                            definition_id: d_ref.clone(),
                            value: narrative_universe::Value::Reference(e2.clone()),
                            canon: narrative_universe::Canon::Acordado,
                            source_status_raw: None,
                            provenance: narrative_universe::Provenance::absent(),
                            scope_note: None,
                            archived: true,
                        },
                    ],
                },
            )
            .unwrap()
            .0
        };

        let base_store = build_mixed_store("mixed-base.sqlite", String::new());
        let base_active = Some(super::ActiveUniverse {
            token: "m".into(),
            store: base_store,
        });
        let base_dto = super::context_active(&base_active, "m", e1.as_str()).unwrap();
        let base_len = serde_json::to_vec(&base_dto).unwrap().len();
        assert_eq!(super::context_dto_wire_bytes(&base_dto), Some(base_len));
        // `d_text` and `d_ref` each emit both an active and an archived outgoing group with
        // `hasArchivedClaims: true` (4B instead of 5B budgeted for `false`), so store budget charges
        // +4 bytes more than serialized wire length.
        let headroom = narrative_universe::MAX_CONTEXT_BYTES - base_len;
        let max_core_fit = headroom - 4;

        let fit_store = build_mixed_store("mixed-fit.sqlite", "x".repeat(max_core_fit));
        let fit_active = Some(super::ActiveUniverse {
            token: "m".into(),
            store: fit_store,
        });
        let fit_dto = super::context_active(&fit_active, "m", e1.as_str()).unwrap();
        let fit_len = serde_json::to_vec(&fit_dto).unwrap().len();
        assert_eq!(super::context_dto_wire_bytes(&fit_dto), Some(fit_len));
        assert!(fit_len <= narrative_universe::MAX_CONTEXT_BYTES);
        assert_eq!(fit_len, narrative_universe::MAX_CONTEXT_BYTES - 4);

        let over_store = build_mixed_store("mixed-over.sqlite", "x".repeat(headroom + 1));
        let over_active = Some(super::ActiveUniverse {
            token: "m".into(),
            store: over_store,
        });
        assert!(matches!(
            super::context_active(&over_active, "m", e1.as_str()),
            Err(BridgeError::ContextTooLarge)
        ));
    }

    #[test]
    fn context_wire_size_helpers_match_serde_json_exactly() {
        fn assert_wire_parity<T: serde::Serialize>(
            label: &str,
            value: &T,
            computed: Option<usize>,
        ) {
            let actual = serde_json::to_vec(value).unwrap().len();
            assert_eq!(computed, Some(actual), "{label}");
        }

        // 1. String helper: empty, plain ASCII, slash, quote, backslash, every ASCII control byte
        // (0x00..=0x1f, including NUL and 2-byte vs 6-byte escapes), 0x7f, BMP and non-BMP Unicode.
        let mut all_controls = String::new();
        for byte in 0u8..=0x7f {
            let single = char::from(byte).to_string();
            assert_wire_parity(
                &format!("ascii byte 0x{byte:02x}"),
                &single,
                super::json_str_wire_bytes(&single),
            );
            if byte <= 0x1f {
                all_controls.push(char::from(byte));
            }
        }
        for (name, sample) in [
            ("empty", ""),
            ("plain", "Mara Vey"),
            ("slash", "a/b/c"),
            ("quotes_and_backslash", r#""quoted"\path\"end""#),
            ("nul_and_controls", all_controls.as_str()),
            ("bmp_and_non_bmp", "café — \u{2028}\u{2029} 𐍈 𝄞 🧪"),
            (
                "mixed_escapes_and_non_bmp",
                "\"\0\\\x08\t\n\x0c\r\x1f 𐍈 \"line\"",
            ),
        ] {
            assert_wire_parity(name, &sample, super::json_str_wire_bytes(sample));
            assert_wire_parity(
                &format!("opt some {name}"),
                &Some(sample),
                super::json_opt_str_wire_bytes(Some(sample)),
            );
        }
        assert_wire_parity(
            "opt none",
            &Option::<&str>::None,
            super::json_opt_str_wire_bytes(None),
        );

        // 2. Array helper: 0, 1, and multiple elements.
        for (name, items) in [
            ("empty array", vec![]),
            ("single array", vec!["one"]),
            ("multi array", vec!["", "\"two\"\\", "\0𝄞"]),
        ] {
            assert_wire_parity(
                name,
                &items,
                super::json_array_wire_bytes(&items, |item| super::json_str_wire_bytes(item)),
            );
        }

        // 3. RevealIdentity: empty, ordinary, and escaped/control/non-BMP labels.
        let id_main = EntityId::parse(PLACE).unwrap().as_str().to_owned();
        let id_other = EntityId::parse(HOUSE).unwrap().as_str().to_owned();
        let make_self = || super::RevealIdentity {
            id: id_main.clone(),
            label: "Self \"Ref\" \\\0𝄞".into(),
        };
        let make_other = || super::RevealIdentity {
            id: id_other.clone(),
            label: "Other 𐍈".into(),
        };
        for (name, label) in [
            ("identity empty", ""),
            ("identity plain", "Valdoria"),
            ("identity escaped unicode", "Mara \"Vey\" \\\0\x1f 𐍈"),
        ] {
            let identity = super::RevealIdentity {
                id: id_main.clone(),
                label: label.to_owned(),
            };
            assert_wire_parity(
                name,
                &identity,
                super::reveal_identity_wire_bytes(&identity),
            );
        }

        // 4. ContextDefinition: owner kinds, optional propertyLabel, value classes, cardinalities, archived.
        let def_cases = [
            (
                "type owner unnamed text singular active",
                "type",
                None,
                "text",
                "singular",
                false,
            ),
            (
                "entity owner named boolean multiple archived",
                "entity",
                Some("oath \"sworn\" \\\0\n𐍈"),
                "boolean",
                "multiple",
                true,
            ),
            (
                "type owner named reference multiple active",
                "type",
                Some("allies"),
                "reference",
                "multiple",
                false,
            ),
        ];
        let make_def = |idx: usize| {
            let (_, owner_kind, prop_label, value_class, cardinality, archived) =
                def_cases[idx % def_cases.len()];
            super::ContextDefinition {
                id: "44444444-4444-4444-8444-000000000001".into(),
                owner_kind,
                owner_id: id_main.clone(),
                property_label: prop_label.map(str::to_owned),
                value_class: value_class.into(),
                cardinality: cardinality.into(),
                archived,
            }
        };
        for (idx, (name, ..)) in def_cases.iter().enumerate() {
            let def = make_def(idx);
            assert_wire_parity(name, &def, super::context_definition_wire_bytes(&def));
        }

        // 5. ContextClaim: all RevealValue variants, all Canon literals, archived bits,
        // and each optional field flipped independently (plus combined escaping/non-BMP).
        let base_claim = || super::ContextClaim {
            id: "55555555-5555-4555-8555-000000000001".into(),
            value: super::RevealValue::Unknown,
            canon: "ACORDADO".into(),
            source_status_raw: None,
            provenance: super::RevealSource {
                external_ref: None,
                locator: None,
                fragment: None,
                source_revision: None,
            },
            scope_note: None,
            archived: false,
        };
        let make_claim = |idx: usize| -> (&'static str, super::ContextClaim) {
            match idx {
                0 => ("unknown all-none acordado", base_claim()),
                1 => (
                    "text empty provisional archived",
                    super::ContextClaim {
                        value: super::RevealValue::Text {
                            text: String::new(),
                        },
                        canon: "PROVISIONAL".into(),
                        archived: true,
                        ..base_claim()
                    },
                ),
                2 => (
                    "text escaped controls non-bmp abierto",
                    super::ContextClaim {
                        value: super::RevealValue::Text {
                            text: "\"line1\"\n\\path\\\0\x01\x1f 𐍈 𝄞".into(),
                        },
                        canon: "ABIERTO".into(),
                        ..base_claim()
                    },
                ),
                3 => (
                    "boolean false misterio",
                    super::ContextClaim {
                        value: super::RevealValue::Boolean { value: false },
                        canon: "MISTERIO_INTENCIONAL".into(),
                        ..base_claim()
                    },
                ),
                4 => (
                    "boolean true archived",
                    super::ContextClaim {
                        value: super::RevealValue::Boolean { value: true },
                        archived: true,
                        ..base_claim()
                    },
                ),
                5 => (
                    "reference self",
                    super::ContextClaim {
                        value: super::RevealValue::Reference {
                            target: make_self(),
                        },
                        ..base_claim()
                    },
                ),
                6 => (
                    "reference other",
                    super::ContextClaim {
                        value: super::RevealValue::Reference {
                            target: make_other(),
                        },
                        ..base_claim()
                    },
                ),
                7 => (
                    "only source_status_raw",
                    super::ContextClaim {
                        source_status_raw: Some("draft \"1\"\0".into()),
                        ..base_claim()
                    },
                ),
                8 => (
                    "only external_ref",
                    super::ContextClaim {
                        provenance: super::RevealSource {
                            external_ref: Some("chronicle\\a\t𝄞".into()),
                            ..base_claim().provenance
                        },
                        ..base_claim()
                    },
                ),
                9 => (
                    "only locator",
                    super::ContextClaim {
                        provenance: super::RevealSource {
                            locator: Some("folio\n3\x08".into()),
                            ..base_claim().provenance
                        },
                        ..base_claim()
                    },
                ),
                10 => (
                    "only fragment",
                    super::ContextClaim {
                        provenance: super::RevealSource {
                            fragment: Some("para \"2\"".into()),
                            ..base_claim().provenance
                        },
                        ..base_claim()
                    },
                ),
                11 => (
                    "only source_revision",
                    super::ContextClaim {
                        provenance: super::RevealSource {
                            source_revision: Some("rev-1\x1b𐍈".into()),
                            ..base_claim().provenance
                        },
                        ..base_claim()
                    },
                ),
                12 => (
                    "only scope_note",
                    super::ContextClaim {
                        scope_note: Some("note\0\r\n\\".into()),
                        ..base_claim()
                    },
                ),
                13 => (
                    "all optionals populated",
                    super::ContextClaim {
                        value: super::RevealValue::Text {
                            text: "body \"text\" 𝄞".into(),
                        },
                        canon: "PROVISIONAL".into(),
                        source_status_raw: Some("canon-draft".into()),
                        provenance: super::RevealSource {
                            external_ref: Some("ext\"1".into()),
                            locator: Some("loc\\2".into()),
                            fragment: Some("frag\x003".into()),
                            source_revision: Some("rev\x04".into()),
                        },
                        scope_note: Some("scope 𐍈".into()),
                        archived: true,
                        ..base_claim()
                    },
                ),
                14 => (
                    "control 0x7f unescaped",
                    super::ContextClaim {
                        value: super::RevealValue::Text {
                            text: "\x7f".into(),
                        },
                        ..base_claim()
                    },
                ),
                _ => (
                    "repeated reference claim",
                    super::ContextClaim {
                        id: "55555555-5555-4555-8555-000000000002".into(),
                        value: super::RevealValue::Reference {
                            target: make_self(),
                        },
                        archived: true,
                        ..base_claim()
                    },
                ),
            }
        };
        for idx in 0..16 {
            let (name, claim) = make_claim(idx);
            assert_wire_parity(name, &claim, super::context_claim_wire_bytes(&claim));
        }

        // 6. Outgoing & incoming groups: 0, 1, and multiple claims across hasArchivedClaims bits.
        let make_claims = |indices: &[usize]| -> Vec<super::ContextClaim> {
            indices.iter().map(|&idx| make_claim(idx).1).collect()
        };
        for (idx, (claim_indices, has_arch)) in [
            (&[][..], false),
            (&[][..], true),
            (&[2][..], false),
            (&[1, 13][..], true),
        ]
        .into_iter()
        .enumerate()
        {
            let out_group = super::ContextOutgoingGroup {
                definition: make_def(idx),
                claims: make_claims(claim_indices),
                has_archived_claims: has_arch,
            };
            assert_wire_parity(
                &format!("outgoing group {idx}"),
                &out_group,
                super::context_outgoing_group_wire_bytes(&out_group),
            );
            let in_group = super::ContextIncomingGroup {
                subject: make_self(),
                definition: make_def(idx + 1),
                claims: make_claims(claim_indices),
            };
            assert_wire_parity(
                &format!("incoming group {idx}"),
                &in_group,
                super::context_incoming_group_wire_bytes(&in_group),
            );
        }

        // 7. Complete EntityContext: optional type_label, empty vs single vs multiple groups,
        // active & archived outgoing/incoming, and self-reference / repeated identities.
        let make_out_empty = || super::ContextOutgoingGroup {
            definition: make_def(0),
            claims: vec![],
            has_archived_claims: false,
        };
        let make_out_multi = || super::ContextOutgoingGroup {
            definition: make_def(2),
            claims: make_claims(&[5, 15]),
            has_archived_claims: true,
        };
        let make_in_single = || super::ContextIncomingGroup {
            subject: make_self(),
            definition: make_def(2),
            claims: make_claims(&[5]),
        };
        let make_in_multi = || super::ContextIncomingGroup {
            subject: make_other(),
            definition: make_def(1),
            claims: make_claims(&[6, 13]),
        };
        for (name, ctx) in [
            (
                "empty untyped entity context",
                super::EntityContext {
                    entity: make_self(),
                    type_label: None,
                    outgoing_groups: vec![],
                    incoming_groups: vec![],
                    archived_outgoing_groups: vec![],
                    archived_incoming_groups: vec![],
                },
            ),
            (
                "single-group typed entity context",
                super::EntityContext {
                    entity: make_self(),
                    type_label: Some("Person \"𐍈\"\\".into()),
                    outgoing_groups: vec![make_out_empty()],
                    incoming_groups: vec![make_in_single()],
                    archived_outgoing_groups: vec![make_out_multi()],
                    archived_incoming_groups: vec![make_in_multi()],
                },
            ),
            (
                "multi-group self-referential entity context",
                super::EntityContext {
                    entity: make_self(),
                    type_label: Some("House\0\n".into()),
                    outgoing_groups: vec![make_out_empty(), make_out_multi()],
                    incoming_groups: vec![make_in_single(), make_in_multi()],
                    archived_outgoing_groups: vec![make_out_multi()],
                    archived_incoming_groups: vec![make_in_multi()],
                },
            ),
        ] {
            assert_wire_parity(name, &ctx, super::context_dto_wire_bytes(&ctx));
        }
    }

    /// Core (`Store::entity_context`) budgets every outgoing group wrapper using
    /// `CONTEXT_OUTGOING_GROUP_WRAPPER_BYTES` (53 bytes, counting `"hasArchivedClaims":false`
    /// as 5 bytes). Because `"hasArchivedClaims":true` serializes in 4 bytes, core intentionally
    /// overestimates by exactly 1 byte for each outgoing group (in `outgoingGroups` or
    /// `archivedOutgoingGroups`) where `hasArchivedClaims == true`.
    ///
    /// As a result:
    /// - Core never underestimates the camelCase DTO wire length, so every accepted `EntityContext`
    ///   projection is guaranteed to serialize within `MAX_CONTEXT_BYTES`.
    /// - Bridge `context_dto_wire_bytes` remains exact (`== serde_json::to_vec(&dto).len()`).
    #[test]
    fn core_conservative_has_archived_claims_allowance_never_underestimates_wire_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let universe = UniverseId::parse(UNIVERSE).unwrap();
        let e1 = EntityId::parse(PLACE).unwrap();
        let d_id = narrative_universe::DefinitionId::parse("44444444-4444-4444-8444-000000000010")
            .unwrap();

        let make_assertion =
            |suffix: u8, text: String, archived: bool| narrative_universe::Assertion {
                id: narrative_universe::AssertionId::parse(&format!(
                    "55555555-5555-4555-8555-0000000000{suffix:02x}"
                ))
                .unwrap(),
                subject_id: e1.clone(),
                definition_id: d_id.clone(),
                value: narrative_universe::Value::Text(text),
                canon: narrative_universe::Canon::Acordado,
                source_status_raw: None,
                provenance: narrative_universe::Provenance::absent(),
                scope_note: None,
                archived,
            };

        let run_case = |label: &str,
                        def_archived: bool,
                        extra_archived_claim: bool,
                        payload_active: bool,
                        expected_allowance: usize| {
            let build = |name: &str, text: String| {
                let path = dir.path().join(name);
                let mut assertions = vec![make_assertion(1, text, !payload_active)];
                if extra_archived_claim {
                    assertions.push(make_assertion(2, "arch".into(), true));
                }
                Store::import_new(
                    &path,
                    narrative_universe::Snapshot {
                        universe_id: universe.clone(),
                        pages: vec![],
                        types: vec![],
                        entities: vec![Entity {
                            id: e1.clone(),
                            label: "Bounded".into(),
                            type_id: None,
                            page_id: None,
                        }],
                        definitions: vec![narrative_universe::Definition {
                            id: d_id.clone(),
                            owner: narrative_universe::Owner::Entity(e1.clone()),
                            value_class: narrative_universe::ValueClass::Text,
                            cardinality: narrative_universe::Cardinality::Multiple,
                            label: Some(
                                narrative_universe::DefinitionLabel::parse("notes").unwrap(),
                            ),
                            archived: def_archived,
                        }],
                        assertions,
                    },
                )
                .unwrap()
                .0
            };

            let base_active = Some(super::ActiveUniverse {
                token: "k".into(),
                store: build(&format!("{label}-base.sqlite"), String::new()),
            });
            let base_dto = super::context_active(&base_active, "k", PLACE).unwrap();
            let true_groups = base_dto
                .outgoing_groups
                .iter()
                .chain(base_dto.archived_outgoing_groups.iter())
                .filter(|g| g.has_archived_claims)
                .count();
            assert_eq!(true_groups, expected_allowance, "{label} true group count");
            let base_len = serde_json::to_vec(&base_dto).unwrap().len();
            assert_eq!(super::context_dto_wire_bytes(&base_dto), Some(base_len));

            let headroom = narrative_universe::MAX_CONTEXT_BYTES - base_len;
            let max_core_fit = headroom - expected_allowance;

            let fit_active = Some(super::ActiveUniverse {
                token: "k".into(),
                store: build(&format!("{label}-fit.sqlite"), "x".repeat(max_core_fit)),
            });
            let fit_dto = super::context_active(&fit_active, "k", PLACE).unwrap();
            let fit_len = serde_json::to_vec(&fit_dto).unwrap().len();
            assert_eq!(super::context_dto_wire_bytes(&fit_dto), Some(fit_len));
            assert!(
                fit_len <= narrative_universe::MAX_CONTEXT_BYTES,
                "{label} accepted wire len {fit_len} exceeded MAX_CONTEXT_BYTES"
            );
            assert_eq!(
                fit_len,
                narrative_universe::MAX_CONTEXT_BYTES - expected_allowance,
                "{label} exact core acceptance boundary"
            );

            // One extra byte hits core's ContextTooLarge ceiling (even when actual wire bytes
            // would be <= MAX_CONTEXT_BYTES if expected_allowance > 0).
            let over_active = Some(super::ActiveUniverse {
                token: "k".into(),
                store: build(
                    &format!("{label}-over.sqlite"),
                    "x".repeat(max_core_fit + 1),
                ),
            });
            assert!(
                matches!(
                    super::context_active(&over_active, "k", PLACE),
                    Err(BridgeError::ContextTooLarge)
                ),
                "{label} expected ContextTooLarge at core boundary + 1"
            );
        };

        // k = 0: active definition + active claim -> hasArchivedClaims is false (exact 2 MiB fit).
        run_case("k0", false, false, true, 0);
        // k = 1: archived definition + archived claim -> 1 archived outgoing group with hasArchivedClaims=true.
        run_case("k1", true, false, false, 1);
        // k = 2: active definition + active claim + archived claim -> both active & archived outgoing groups have hasArchivedClaims=true.
        run_case("k2", false, true, true, 2);
    }

    #[derive(serde::Deserialize)]
    struct Fixture {
        cases: Vec<FixtureCase>,
    }

    #[derive(serde::Deserialize)]
    struct FixtureCase {
        id: String,
        base: String,
        mentions: Vec<FixtureMention>,
        expected_body: String,
        expected_mentions: Vec<FixtureMention>,
        #[serde(default)]
        expected_edits: Vec<(u32, u32, String)>,
        expected_ops: Option<Vec<FixtureOp>>,
        expected_revision: i64,
    }

    #[derive(serde::Deserialize)]
    struct FixtureMention {
        id: String,
        entity_id: String,
        start_utf16: u32,
        end_utf16: u32,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum FixtureOp {
        Replace(u32, u32, String),
        Link(FixtureMention),
        Unlink(String),
    }

    fn fixture_wire(held: &FixtureMention) -> WireMention {
        wire(&held.id, &held.entity_id, held.start_utf16, held.end_utf16)
    }

    /// The fixture expectations are what the TypeScript session sends, so
    /// this is the client prediction checked by the real bridge.
    #[test]
    fn every_shared_case_passes_the_bridge_agreement_check_and_reopens() {
        let fixture: Fixture =
            serde_json::from_str(include_str!("../../../tests/fixtures/bridge_cases.json"))
                .unwrap();
        for case in fixture.cases {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("case.sqlite");
            let id = PageId::parse(PAGE).unwrap();
            let mut store = Store::create(&path, UniverseId::parse(UNIVERSE).unwrap()).unwrap();
            store
                .create_page(Page {
                    id: id.clone(),
                    title: "Bridge".into(),
                    body: case.base.clone(),
                })
                .unwrap();
            let ops: Vec<SaveOp> = match &case.expected_ops {
                Some(ops) => ops
                    .iter()
                    .map(|op| match op {
                        FixtureOp::Replace(start, end, text) => replace(*start, *end, text),
                        FixtureOp::Link(held) => {
                            link(&held.id, &held.entity_id, held.start_utf16, held.end_utf16)
                        }
                        FixtureOp::Unlink(mention_id) => SaveOp::Unlink {
                            mention_id: mention_id.clone(),
                        },
                    })
                    .collect(),
                None => case
                    .expected_edits
                    .iter()
                    .map(|(start, end, text)| replace(*start, *end, text))
                    .collect(),
            };
            let mut entities: Vec<&str> = case
                .mentions
                .iter()
                .map(|held| held.entity_id.as_str())
                .collect();
            for op in case.expected_ops.iter().flatten() {
                if let FixtureOp::Link(held) = op {
                    entities.push(&held.entity_id);
                }
            }
            entities.sort();
            entities.dedup();
            for entity in entities {
                store
                    .create_entity(Entity {
                        id: EntityId::parse(entity).unwrap(),
                        label: "Synthetic".into(),
                        type_id: None,
                        page_id: None,
                    })
                    .unwrap();
            }
            let links: Vec<PageEdit> = case
                .mentions
                .iter()
                .map(|held| {
                    PageEdit::Mention(MentionEdit::Link {
                        mention: mention(
                            &held.id,
                            &held.entity_id,
                            held.start_utf16,
                            held.end_utf16,
                        ),
                    })
                })
                .collect();
            let revision = if links.is_empty() {
                0
            } else {
                store
                    .save_page_edits(&id, PageRevision::ZERO, &links)
                    .unwrap()
                    .revision
                    .as_i64()
            };
            let mut request = with_ops(revision, ops);
            request.expected_body = case.expected_body.clone();
            request.expected_mentions = case.expected_mentions.iter().map(fixture_wire).collect();
            let saved = save_to_store(&mut store, request)
                .unwrap_or_else(|error| panic!("{}: {error:?}", case.id));
            let expected: Vec<WireMention> =
                case.expected_mentions.iter().map(fixture_wire).collect();
            assert_eq!(saved.body, case.expected_body, "{}", case.id);
            assert_eq!(saved.mentions, expected, "{}", case.id);
            assert_eq!(saved.revision, case.expected_revision, "{}", case.id);
            drop(store);
            let reopened = page_content(&Store::open(&path).unwrap(), PAGE).unwrap();
            assert_eq!(
                (reopened.body, reopened.mentions, reopened.revision),
                (case.expected_body, expected, Some(case.expected_revision)),
                "{} reopen",
                case.id
            );
        }
    }

    #[test]
    fn maps_expected_open_and_page_errors_to_stable_kinds() {
        let missing = Error::Validation {
            reason: ValidationReason::PathMissing,
            field: "path",
            id: None,
        };
        assert!(matches!(
            map_open_error(missing),
            BridgeError::DatabaseMissing
        ));
        let invalid = Error::Validation {
            reason: ValidationReason::NotADatabase,
            field: "path",
            id: None,
        };
        assert!(matches!(
            map_open_error(invalid),
            BridgeError::InvalidDatabase
        ));
        let missing_page = Error::Validation {
            reason: ValidationReason::NotFound,
            field: "id",
            id: None,
        };
        assert!(matches!(
            map_read_error(missing_page, true),
            BridgeError::PageMissing
        ));
        let storage = Error::Storage {
            message: "private sqlite detail".to_string(),
        };
        assert!(matches!(
            map_read_error(storage, true),
            BridgeError::ReadFailed
        ));
    }

    #[test]
    fn codex_bridge_reads_creates_persists_and_enforces_validation_and_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("codex-bridge.sqlite");
        let mut store = Store::create(&path, UniverseId::parse(UNIVERSE).unwrap()).unwrap();
        let page_id = PageId::parse(PAGE).unwrap();
        store
            .create_page(Page {
                id: page_id.clone(),
                title: "Harbor Ledger".into(),
                body: "Untouched prose.".into(),
            })
            .unwrap();
        let mut active = Some(ActiveUniverse {
            token: "tok-1".into(),
            store,
        });

        let empty = codex_entries(active_store(&active, "tok-1").unwrap()).unwrap();
        assert!(empty.types.is_empty());
        assert!(empty.entities.is_empty());

        // Stale universe token rejected on all Codex bridge paths.
        assert!(matches!(
            active_store(&active, "wrong"),
            Err(BridgeError::UniverseChanged)
        ));
        assert!(matches!(
            create_type_active(&mut active, "wrong", HOUSE, "House"),
            Err(BridgeError::UniverseChanged)
        ));
        assert!(matches!(
            create_entity_active(&mut active, "wrong", PLACE, "Valdoria", None),
            Err(BridgeError::UniverseChanged)
        ));

        // Invalid IDs and invalid nonblank labels rejected without mutating DB.
        assert!(matches!(
            create_type_active(&mut active, "tok-1", "bad-id", "House"),
            Err(BridgeError::InvalidInput)
        ));
        for bad_label in ["", "   ", "\u{00a0}", " House", "House ", "House\nTwo"] {
            assert!(
                matches!(
                    create_type_active(&mut active, "tok-1", HOUSE, bad_label),
                    Err(BridgeError::InvalidInput)
                ),
                "bad type label {bad_label:?}"
            );
            assert!(
                matches!(
                    create_entity_active(&mut active, "tok-1", PLACE, bad_label, None),
                    Err(BridgeError::InvalidInput)
                ),
                "bad entity label {bad_label:?}"
            );
        }
        let too_long = "x".repeat(narrative_universe::MAX_CODEX_LABEL_SCALARS + 1);
        assert!(matches!(
            create_entity_active(&mut active, "tok-1", PLACE, &too_long, None),
            Err(BridgeError::InvalidInput)
        ));

        // Missing or malformed type_id rejected cleanly.
        assert!(matches!(
            create_entity_active(&mut active, "tok-1", PLACE, "Valdoria", Some("not-uuid")),
            Err(BridgeError::InvalidInput)
        ));
        assert!(matches!(
            create_entity_active(&mut active, "tok-1", PLACE, "Valdoria", Some(HOUSE)),
            Err(BridgeError::TypeMissing)
        ));

        // Valid custom type creation (including duplicate type label under distinct ID).
        let t1 = create_type_active(&mut active, "tok-1", HOUSE, "House").unwrap();
        assert_eq!(
            t1,
            TypeIndexEntry {
                id: HOUSE.into(),
                label: "House".into()
            }
        );
        let house_2 = "33333333-3333-4333-8333-333333333339";
        let t2 = create_type_active(&mut active, "tok-1", house_2, "House").unwrap();
        assert_eq!(t2.id, house_2);
        assert!(matches!(
            create_type_active(&mut active, "tok-1", HOUSE, "House"),
            Err(BridgeError::DuplicateId)
        ));

        // Valid untyped + typed + duplicate-label entities.
        let e1 =
            create_entity_active(&mut active, "tok-1", PLACE, "Valdoria", Some(HOUSE)).unwrap();
        assert_eq!(
            e1,
            CodexEntityEntry {
                id: PLACE.into(),
                label: "Valdoria".into(),
                type_id: Some(HOUSE.into()),
                type_label: Some("House".into()),
                primary_page_title: None,
            }
        );
        let dup_id = "33333333-3333-4333-8333-333333333340";
        let e2 = create_entity_active(&mut active, "tok-1", dup_id, "Valdoria", None).unwrap();
        assert_eq!(e2.type_id, None);
        assert_eq!(e2.type_label, None);
        assert!(matches!(
            create_entity_active(&mut active, "tok-1", PLACE, "Valdoria", None),
            Err(BridgeError::DuplicateId)
        ));

        // Busy lock returns BridgeError::Busy without retrying.
        let lock = rusqlite::Connection::open(&path).unwrap();
        lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
        assert!(matches!(
            create_type_active(
                &mut active,
                "tok-1",
                "33333333-3333-4333-8333-333333333341",
                "Guild"
            ),
            Err(BridgeError::Busy)
        ));
        assert!(matches!(
            create_entity_active(
                &mut active,
                "tok-1",
                "33333333-3333-4333-8333-333333333342",
                "Guild Hall",
                None
            ),
            Err(BridgeError::Busy)
        ));
        lock.execute_batch("ROLLBACK").unwrap();

        let index = codex_entries(active_store(&active, "tok-1").unwrap()).unwrap();
        assert_eq!(index.types.len(), 2);
        assert_eq!(index.entities.len(), 2);
        // Write's entity_index immediately sees Codex-created entities too.
        assert_eq!(
            entity_entries(active_store(&active, "tok-1").unwrap())
                .unwrap()
                .len(),
            2
        );

        // Unrelated page remains untouched; reopen restores exact types & entities.
        let doc = active_store(&active, "tok-1")
            .unwrap()
            .page_document(&page_id)
            .unwrap();
        assert_eq!(doc.body.body, "Untouched prose.");
        assert_eq!(doc.revision, PageRevision::ZERO);
        drop(active);

        let reopened = Store::open(&path).unwrap();
        assert_eq!(codex_entries(&reopened).unwrap(), index);
    }
}
