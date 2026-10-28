use std::path::Path;
use std::sync::Mutex;

use narrative_universe::{Error, PageId, PageSummary, Store, ValidationReason};
use serde::Serialize;
use tauri::State;

struct DesktopState {
    store: Mutex<Option<Store>>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
enum BridgeError {
    EmptyPath,
    DatabaseMissing,
    InvalidDatabase,
    UnsupportedDatabase,
    UniverseNotOpen,
    PageMissing,
    ReadFailed,
    StateUnavailable,
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
fn open_universe(database_path: String, state: State<'_, DesktopState>) -> Result<(), BridgeError> {
    if database_path.trim().is_empty() {
        return Err(BridgeError::EmptyPath);
    }
    {
        let mut active = state
            .store
            .lock()
            .map_err(|_| BridgeError::StateUnavailable)?;
        *active = None;
    }
    let store = Store::open(Path::new(&database_path)).map_err(map_open_error)?;
    let mut active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    *active = Some(store);
    Ok(())
}

#[tauri::command(async)]
fn page_index(state: State<'_, DesktopState>) -> Result<Vec<PageIndexEntry>, BridgeError> {
    let active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    let store = active.as_ref().ok_or(BridgeError::UniverseNotOpen)?;
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
fn read_page(page_id: String, state: State<'_, DesktopState>) -> Result<PageContent, BridgeError> {
    let id = PageId::parse(&page_id).map_err(|_| BridgeError::PageMissing)?;
    let active = state
        .store
        .lock()
        .map_err(|_| BridgeError::StateUnavailable)?;
    let store = active.as_ref().ok_or(BridgeError::UniverseNotOpen)?;
    let document = store
        .page_document(&id)
        .map_err(|error| map_read_error(error, true))?;
    Ok(PageContent {
        id: document.id.as_str().to_string(),
        title: document.title,
        body: document.body.body,
    })
}

pub fn run() {
    tauri::Builder::default()
        .manage(DesktopState {
            store: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![open_universe, page_index, read_page])
        .run(tauri::generate_context!())
        .expect("failed to run Narrative Universe desktop shell");
}

#[cfg(test)]
mod tests {
    use super::{map_open_error, map_read_error, BridgeError};
    use narrative_universe::{Error, ValidationReason};

    #[test]
    fn maps_expected_open_and_page_errors_to_stable_kinds() {
        let missing = Error::Validation {
            reason: ValidationReason::PathMissing,
            field: "path",
            id: None,
        };
        assert!(matches!(map_open_error(missing), BridgeError::DatabaseMissing));

        let invalid = Error::Validation {
            reason: ValidationReason::NotADatabase,
            field: "path",
            id: None,
        };
        assert!(matches!(map_open_error(invalid), BridgeError::InvalidDatabase));

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
