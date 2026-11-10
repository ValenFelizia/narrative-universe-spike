use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Mutex;

use narrative_universe::{
    apply_mention_edit, EntityId, Error, Mention, MentionEdit, MentionId, Page, PageEdit, PageId,
    PageRevision, PageSummary, Store, ValidationReason,
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
        active_store, consume_close_request, create_active, entity_entries, map_open_error,
        map_read_error, page_content, prepare_universe, save_active, save_to_store, wire_mentions,
        ActiveUniverse, BridgeError, DesktopState, SaveFailure, SaveOp, SaveRequest, SaveResponse,
        WireMention,
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
}
