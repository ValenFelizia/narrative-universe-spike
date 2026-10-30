use std::path::Path;
use std::sync::Mutex;

use narrative_universe::{
    Error, MentionEdit, Page, PageEdit, PageId, PageRevision, PageSummary, Store, ValidationReason,
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
    mention_count: usize,
}

const MAX_JS_REVISION: i64 = 9_007_199_254_740_991;

#[derive(Debug, Deserialize)]
struct TextEdit {
    start_utf16: u32,
    end_utf16: u32,
    inserted_text: String,
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
    edits: Vec<TextEdit>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveResponse {
    page_id: String,
    session_id: String,
    request_id: u64,
    title: String,
    body: String,
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
        Error::UnsupportedVersion { .. } | Error::MigrationRequired { .. } => {
            BridgeError::UnsupportedDatabase
        }
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
    let id = PageId::parse(&page_id).map_err(|_| BridgeError::PageMissing)?;
    let active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    let store = active_store(&active, &universe_token)?;
    let document = store
        .page_document(&id)
        .map_err(|error| map_read_error(error, true))?;
    Ok(PageContent {
        id: document.id.as_str().to_string(),
        title: document.title,
        body: document.body.body,
        revision: (document.revision.as_i64() <= MAX_JS_REVISION)
            .then_some(document.revision.as_i64()),
        mention_count: document.body.mentions.len(),
    })
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
    if !current.body.mentions.is_empty()
        || current.body.body.contains(['\r', '\0'])
        || current.title.contains(['\r', '\0'])
    {
        return Err(BridgeError::ReadOnly);
    }
    if request
        .title
        .as_ref()
        .is_some_and(|title| title.contains(['\r', '\0']))
    {
        return Err(BridgeError::InvalidInput);
    }
    if request
        .edits
        .iter()
        .any(|edit| edit.inserted_text.contains(['\r', '\0']))
    {
        return Err(BridgeError::InvalidInput);
    }
    let mut edits: Vec<PageEdit> = request
        .edits
        .into_iter()
        .map(|edit| {
            PageEdit::Mention(MentionEdit::ReplaceText {
                start_utf16: edit.start_utf16,
                end_utf16: edit.end_utf16,
                inserted_text: edit.inserted_text,
            })
        })
        .collect();
    if let Some(title) = request.title {
        edits.insert(0, PageEdit::SetTitle { title });
    }
    let saved = store
        .save_page_edits(&id, revision, &edits)
        .map_err(save_error)?;
    Ok(SaveResponse {
        page_id: saved.id.as_str().to_owned(),
        session_id: request.session_id,
        request_id: request.request_id,
        title: saved.title,
        body: saved.body.body,
        revision: saved.revision.as_i64(),
    })
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
        mention_count: 0,
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
        active_store, consume_close_request, create_active, map_open_error, map_read_error,
        prepare_universe, save_active, save_to_store, ActiveUniverse, BridgeError, DesktopState,
        SaveFailure, SaveRequest, TextEdit,
    };
    use narrative_universe::{
        Entity, EntityId, Error, Mention, MentionEdit, MentionId, Page, PageEdit, PageId,
        PageRevision, Store, UniverseId, ValidationReason,
    };

    fn request(revision: i64, text: &str) -> SaveRequest {
        SaveRequest {
            universe_token: "test".into(),
            page_id: "22222222-2222-4222-8222-222222222222".into(),
            session_id: "session".into(),
            request_id: 1,
            expected_revision: revision,
            title: None,
            edits: vec![TextEdit {
                start_utf16: 0,
                end_utf16: 0,
                inserted_text: text.into(),
            }],
        }
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
        let id = PageId::parse("22222222-2222-4222-8222-222222222222").unwrap();
        let mut store = Store::create(
            &path,
            UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap(),
        )
        .unwrap();
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
        let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap();
        let mut old = Store::create(&old_path, universe.clone()).unwrap();
        let id = PageId::parse("22222222-2222-4222-8222-222222222222").unwrap();
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
    }

    #[test]
    fn bridge_saves_reopens_rejects_stale_and_busy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.sqlite");
        let id = PageId::parse("22222222-2222-4222-8222-222222222222").unwrap();
        let mut store = Store::create(
            &path,
            UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap(),
        )
        .unwrap();
        store
            .create_page(Page {
                id: id.clone(),
                title: "title".into(),
                body: "texto 📚".into(),
            })
            .unwrap();
        let mut combined = request(0, "\u{e1} ");
        combined.title = Some("T\u{ed}tulo \u{1f409}".into());
        let saved = save_to_store(&mut store, combined).unwrap();
        assert_eq!(saved.title, "T\u{ed}tulo \u{1f409}");
        assert_eq!(saved.body, "á texto 📚");
        assert_eq!(saved.revision, 1);
        let mut noop = request(1, "");
        noop.edits.clear();
        assert_eq!(save_to_store(&mut store, noop).unwrap().revision, 1);
        let mut invalid = request(1, "x");
        invalid.title = Some("rollback".into());
        invalid.edits[0].start_utf16 = 10_000;
        invalid.edits[0].end_utf16 = 10_000;
        assert!(matches!(
            save_to_store(&mut store, invalid),
            Err(BridgeError::InvalidInput)
        ));
        assert!(matches!(
            save_to_store(&mut store, request(0, "x")),
            Err(BridgeError::Stale)
        ));
        assert_eq!(
            store.page_document(&id).unwrap().title,
            "T\u{ed}tulo \u{1f409}"
        );
        let lock = rusqlite::Connection::open(&path).unwrap();
        lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
        assert!(matches!(
            save_to_store(&mut store, request(1, "x")),
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
        let store = Store::create(
            &path,
            UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap(),
        )
        .unwrap();
        let mut active = Some(ActiveUniverse {
            token: "current".into(),
            store,
        });
        let id = "22222222-2222-4222-8222-222222222222";
        assert!(matches!(
            create_active(&mut active, "old", id),
            Err(BridgeError::UniverseChanged)
        ));
        assert!(matches!(
            create_active(&mut active, "current", "bad"),
            Err(BridgeError::InvalidInput)
        ));
        let created = create_active(&mut active, "current", id).unwrap();
        assert_eq!(created.revision, Some(0));
        assert_eq!(created.title, "Untitled document");
        assert_eq!(
            active.as_ref().unwrap().store.page_index().unwrap()[0].title,
            "Untitled document"
        );
        assert!(matches!(
            create_active(&mut active, "current", id),
            Err(BridgeError::DuplicateId)
        ));
        let lock = rusqlite::Connection::open(&path).unwrap();
        lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
        assert!(matches!(
            create_active(
                &mut active,
                "current",
                "33333333-3333-4333-8333-333333333333"
            ),
            Err(BridgeError::Busy)
        ));
        lock.execute_batch("ROLLBACK").unwrap();
        assert_eq!(
            active.as_ref().unwrap().store.page_index().unwrap().len(),
            1
        );
    }

    #[test]
    fn bridge_refuses_unsupported_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.sqlite");
        let id = PageId::parse("22222222-2222-4222-8222-222222222222").unwrap();
        let mut store = Store::create(
            &path,
            UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap(),
        )
        .unwrap();
        store
            .create_page(Page {
                id,
                title: "title".into(),
                body: "one\r\ntwo".into(),
            })
            .unwrap();
        assert!(matches!(
            save_to_store(&mut store, request(0, "x")),
            Err(BridgeError::ReadOnly)
        ));
    }

    #[test]
    fn bridge_refuses_linked_page_without_changing_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linked.sqlite");
        let id = PageId::parse("22222222-2222-4222-8222-222222222222").unwrap();
        let entity = EntityId::parse("33333333-3333-4333-8333-333333333333").unwrap();
        let mut store = Store::create(
            &path,
            UniverseId::parse("11111111-1111-4111-8111-111111111111").unwrap(),
        )
        .unwrap();
        store
            .create_page(Page {
                id: id.clone(),
                title: "title".into(),
                body: "linked text".into(),
            })
            .unwrap();
        store
            .create_entity(Entity {
                id: entity.clone(),
                label: "synthetic".into(),
                type_id: None,
                page_id: None,
            })
            .unwrap();
        store
            .save_page_edits(
                &id,
                PageRevision::ZERO,
                &[PageEdit::Mention(MentionEdit::Link {
                    mention: Mention {
                        id: MentionId::parse("44444444-4444-4444-8444-444444444444").unwrap(),
                        entity_id: entity,
                        start_utf16: 0,
                        end_utf16: 6,
                    },
                })],
            )
            .unwrap();
        assert!(matches!(
            save_to_store(&mut store, request(1, "x")),
            Err(BridgeError::ReadOnly)
        ));
        let mut title_request = request(1, "");
        title_request.title = Some("changed".into());
        title_request.edits.clear();
        assert!(matches!(
            save_to_store(&mut store, title_request),
            Err(BridgeError::ReadOnly)
        ));
        let document = store.page_document(&id).unwrap();
        assert_eq!(document.title, "title");
        assert_eq!(document.body.body, "linked text");
        assert_eq!(document.body.mentions.len(), 1);
        assert_eq!(document.revision.as_i64(), 1);
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
