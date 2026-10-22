use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use rusqlite::{
    params, Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior,
};

use crate::error::{Error, Result, ValidationReason};
use crate::mentions::{apply_mention_edit, validate_mentions, AnnotatedBody, Mention, MentionEdit};
use crate::model::{
    value_fits, Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId, Entity,
    EntityId, EntityType, MentionId, Owner, Page, PageId, PageRevision, Provenance, TypeId,
    UniverseId, Value, ValueClass,
};

const SCHEMA: &str = include_str!("schema.sql");
const HEADER_LEN: usize = 100;
const MAGIC: &[u8] = b"SQLite format 3\0";
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const SQLITE_CONSTRAINT_PRIMARYKEY: i32 = 1555;
const LEGACY_SCHEMA_VERSION: i32 = 1;

const ASSERTION_COLUMNS: &str = "id, subject_id, definition_id, value_kind, text_value, bool_value, target_entity_id, canon, source_status_raw, provenance_external_ref, provenance_locator, provenance_fragment, provenance_source_revision, scope_note, archived";

// `IS` treats NULL as equal to NULL, so empty text columns do not split two booleans.
const INCOMPATIBLE_PAIRS: &str = "
SELECT a.id, b.id
FROM assertions AS a
JOIN assertions AS b
  ON a.subject_id = b.subject_id
 AND a.definition_id = b.definition_id
 AND a.id < b.id
JOIN definitions AS d ON d.id = a.definition_id
WHERE a.archived = 0
  AND b.archived = 0
  AND d.archived = 0
  AND d.cardinality = 'singular'
  AND a.canon = 'ACORDADO'
  AND b.canon = 'ACORDADO'
  AND a.value_kind != 'unknown'
  AND b.value_kind != 'unknown'
  AND NOT (
       a.value_kind = b.value_kind
       AND a.text_value IS b.text_value
       AND a.bool_value IS b.bool_value
       AND a.target_entity_id IS b.target_entity_id
  )
ORDER BY a.subject_id, a.definition_id, a.id, b.id
";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PossibleIncompatibility {
    pub first: Assertion,
    pub second: Assertion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub universe_id: UniverseId,
    pub pages: Vec<PageSnapshot>,
    pub types: Vec<EntityType>,
    pub entities: Vec<Entity>,
    pub definitions: Vec<Definition>,
    pub assertions: Vec<Assertion>,
}

/// One page with the complete canonical mention set that points into it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageSnapshot {
    pub page: Page,
    pub mentions: Vec<Mention>,
}

/// A page read as one consistent aggregate.
///
/// The body carries its mentions, so a caller never has to combine a body
/// with a mention list that belongs to another version of that body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageDocument {
    pub id: PageId,
    pub title: String,
    pub body: AnnotatedBody,
    pub revision: PageRevision,
}

/// One transient editing command for a page.
///
/// The operations are applied in order, each one in the coordinates of the
/// body that the previous operation produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageEdit {
    SetTitle { title: String },
    Mention(MentionEdit),
}

/// One occurrence of an entity found through a backlink query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionOccurrence {
    pub page_id: PageId,
    pub page_revision: PageRevision,
    pub mention: Mention,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportOutcome {
    pub inserted: u64,
    pub unchanged: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    pub pages: u64,
    pub types: u64,
    pub entities: u64,
    pub definitions: u64,
    pub assertions: u64,
}

pub struct Store {
    conn: Connection,
    universe_id: UniverseId,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Store")
            .field("universe_id", &self.universe_id)
            .finish_non_exhaustive()
    }
}

impl Store {
    pub fn create(path: &Path, universe_id: UniverseId) -> Result<Self> {
        reserve_new_file(path)?;
        let mut conn = match open_read_write(path) {
            Ok(conn) => conn,
            Err(err) => return Err(residual(path, err)),
        };
        match init_new(&mut conn, &universe_id) {
            Ok(()) => Ok(Self { conn, universe_id }),
            Err(err) => {
                drop(conn);
                Err(residual(path, err))
            }
        }
    }

    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_file() {
            let reason = if path.exists() {
                ValidationReason::NotADatabase
            } else {
                ValidationReason::PathMissing
            };
            return Err(Error::validation(reason, "path", None));
        }
        let header = read_header(path)?;
        if header.user_version != crate::SCHEMA_VERSION {
            if header.user_version == LEGACY_SCHEMA_VERSION {
                return Err(legacy_outcome(path)?);
            }
            return Err(Error::UnsupportedVersion {
                found: header.user_version,
            });
        }
        let inspected = open_read_only(path)?;
        require_delete_journal(&inspected)?;
        require_tables(&inspected)?;
        let universe_id = read_universe(&inspected)?;
        drop(inspected);
        let conn = open_read_write(path)?;
        prepare_connection(&conn)?;
        require_delete_journal(&conn)?;
        let confirmed = read_universe(&conn)?;
        if confirmed != universe_id {
            return Err(Error::validation(
                ValidationReason::ForeignDatabase,
                "universe_id",
                Some(confirmed.as_str()),
            ));
        }
        Ok(Self { conn, universe_id })
    }

    pub fn universe_id(&self) -> &UniverseId {
        &self.universe_id
    }

    pub fn counts(&self) -> Result<Counts> {
        Ok(Counts {
            pages: count(&self.conn, "SELECT COUNT(*) FROM pages")?,
            types: count(&self.conn, "SELECT COUNT(*) FROM types")?,
            entities: count(&self.conn, "SELECT COUNT(*) FROM entities")?,
            definitions: count(&self.conn, "SELECT COUNT(*) FROM definitions")?,
            assertions: count(&self.conn, "SELECT COUNT(*) FROM assertions")?,
        })
    }

    pub fn create_page(&mut self, page: Page) -> Result<Page> {
        let id = page.id.clone();
        self.write(|tx| {
            tx.execute(
                "INSERT INTO pages (id, title, body) VALUES (?1, ?2, ?3)",
                params![page.id.as_str(), page.title, page.body],
            )
            .map_err(|err| map_write("id", id.as_str(), err))?;
            required_page(tx, &id)
        })
    }

    pub fn page(&self, id: &PageId) -> Result<Page> {
        required_page(&self.conn, id)
    }

    /// Reads a page as one aggregate: title, body with its mentions, and the
    /// revision of that aggregate.
    ///
    /// The page and its mentions are read in one transaction, so the caller
    /// never sees a body and a mention list that belong to different
    /// versions of the page.
    pub fn page_document(&self, id: &PageId) -> Result<PageDocument> {
        let transaction = self
            .conn
            .unchecked_transaction()
            .map_err(|err| map_sql("begin", err))?;
        let document = require_page_aggregate(&transaction, id);
        match document {
            Ok(document) => {
                transaction.commit().map_err(|err| map_sql("commit", err))?;
                Ok(document)
            }
            Err(err) => Err(err),
        }
    }

    /// Applies ordered edits to a page and stores the whole aggregate.
    ///
    /// The page is only written when its revision still matches
    /// `expected_revision`. A stale request is refused, even when it would
    /// produce the aggregate that is already stored. The edits are applied
    /// in memory first, so a rejected batch leaves the caller's draft and
    /// the stored page untouched.
    pub fn save_page_edits(
        &mut self,
        id: &PageId,
        expected_revision: PageRevision,
        edits: &[PageEdit],
    ) -> Result<PageDocument> {
        self.write(|tx| {
            let stored = require_page_row(tx, id)?;
            if stored.revision != expected_revision {
                return Err(Error::StaleRevision {
                    page_id: id.as_str().to_string(),
                    expected: expected_revision.as_i64(),
                    found: stored.revision.as_i64(),
                });
            }
            let mut title = stored.title.clone();
            let stored_body = read_page_annotated(tx, id, &stored.body)?;
            let stored_mentions = stored_body.mentions.clone();
            let mut body = stored_body;
            for edit in edits {
                match edit {
                    PageEdit::SetTitle { title: next } => title = next.clone(),
                    PageEdit::Mention(operation) => {
                        if let MentionEdit::Link { mention } = operation {
                            if lookup_entity(tx, &mention.entity_id)?.is_none() {
                                return Err(Error::validation(
                                    ValidationReason::UnknownEntity,
                                    "entity_id",
                                    Some(mention.entity_id.as_str()),
                                ));
                            }
                        }
                        let (next, _removed) = apply_mention_edit(&body, operation)?;
                        body = next;
                    }
                }
            }
            for mention in &body.mentions {
                if let Some(owner) = lookup_mention_owner(tx, &mention.id)? {
                    if owner != *id {
                        return Err(Error::validation(
                            ValidationReason::MentionIdConflict,
                            "mention_id",
                            Some(mention.id.as_str()),
                        ));
                    }
                }
            }
            let changed = title != stored.title
                || body.body != stored.body
                || body.mentions != stored_mentions;
            let revision = if changed {
                expected_revision.checked_next()?
            } else {
                expected_revision
            };
            tx.execute(
                "UPDATE pages SET title = ?1, body = ?2, revision = ?3 WHERE id = ?4",
                params![title, body.body, revision.as_i64(), id.as_str()],
            )
            .map_err(|err| map_sql("save page", err))?;
            tx.execute("DELETE FROM mentions WHERE page_id = ?1", [id.as_str()])
                .map_err(|err| map_sql("clear mentions", err))?;
            for mention in &body.mentions {
                tx.execute(
                    "INSERT INTO mentions (id, page_id, entity_id, start_utf16, end_utf16)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        mention.id.as_str(),
                        id.as_str(),
                        mention.entity_id.as_str(),
                        mention.start_utf16,
                        mention.end_utf16,
                    ],
                )
                .map_err(|err| map_write("mention", mention.id.as_str(), err))?;
            }
            Ok(PageDocument {
                id: id.clone(),
                title,
                body,
                revision,
            })
        })
    }

    /// Finds every occurrence that points at an entity.
    ///
    /// The occurrences are sorted by page, then offset, then mention id. An
    /// entity that does not exist is not found, and an entity without links
    /// has no occurrences.
    pub fn mentions_for_entity(&self, entity: &EntityId) -> Result<Vec<MentionOccurrence>> {
        let transaction = self
            .conn
            .unchecked_transaction()
            .map_err(|err| map_sql("begin", err))?;
        let occurrences = mentions_for_entity_in(&transaction, entity);
        match occurrences {
            Ok(occurrences) => {
                transaction.commit().map_err(|err| map_sql("commit", err))?;
                Ok(occurrences)
            }
            Err(err) => Err(err),
        }
    }

    pub fn create_entity_type(&mut self, entity_type: EntityType) -> Result<EntityType> {
        let id = entity_type.id.clone();
        self.write(|tx| {
            tx.execute(
                "INSERT INTO types (id, label) VALUES (?1, ?2)",
                params![entity_type.id.as_str(), entity_type.label],
            )
            .map_err(|err| map_write("id", id.as_str(), err))?;
            required_entity_type(tx, &id)
        })
    }

    pub fn entity_type(&self, id: &TypeId) -> Result<EntityType> {
        required_entity_type(&self.conn, id)
    }

    pub fn create_entity(&mut self, entity: Entity) -> Result<Entity> {
        let id = entity.id.clone();
        self.write(|tx| {
            if let Some(type_id) = &entity.type_id {
                if lookup_entity_type(tx, type_id)?.is_none() {
                    return Err(Error::validation(
                        ValidationReason::UnknownType,
                        "type_id",
                        Some(type_id.as_str()),
                    ));
                }
            }
            if let Some(page_id) = &entity.page_id {
                if lookup_page(tx, page_id)?.is_none() {
                    return Err(Error::validation(
                        ValidationReason::UnknownPage,
                        "page_id",
                        Some(page_id.as_str()),
                    ));
                }
            }
            tx.execute(
                "INSERT INTO entities (id, label, type_id, page_id) VALUES (?1, ?2, ?3, ?4)",
                params![
                    entity.id.as_str(),
                    entity.label,
                    entity.type_id.as_ref().map(TypeId::as_str),
                    entity.page_id.as_ref().map(PageId::as_str),
                ],
            )
            .map_err(|err| map_write("id", id.as_str(), err))?;
            required_entity(tx, &id)
        })
    }

    pub fn entity(&self, id: &EntityId) -> Result<Entity> {
        required_entity(&self.conn, id)
    }

    pub fn create_definition(&mut self, definition: Definition) -> Result<Definition> {
        let id = definition.id.clone();
        self.write(|tx| {
            match &definition.owner {
                Owner::Type(type_id) => {
                    if lookup_entity_type(tx, type_id)?.is_none() {
                        return Err(Error::validation(
                            ValidationReason::UnknownType,
                            "owner_type_id",
                            Some(type_id.as_str()),
                        ));
                    }
                }
                Owner::Entity(entity_id) => {
                    if lookup_entity(tx, entity_id)?.is_none() {
                        return Err(Error::validation(
                            ValidationReason::UnknownEntity,
                            "owner_entity_id",
                            Some(entity_id.as_str()),
                        ));
                    }
                }
            }
            let (owner_type, owner_entity) = match &definition.owner {
                Owner::Type(type_id) => (Some(type_id.as_str()), None),
                Owner::Entity(entity_id) => (None, Some(entity_id.as_str())),
            };
            tx.execute(
                "INSERT INTO definitions (
                    id, owner_type_id, owner_entity_id, value_class, cardinality, archived
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    definition.id.as_str(),
                    owner_type,
                    owner_entity,
                    definition.value_class.as_str(),
                    definition.cardinality.as_str(),
                    i64::from(definition.archived),
                ],
            )
            .map_err(|err| map_write("id", id.as_str(), err))?;
            required_definition(tx, &id)
        })
    }

    pub fn definition(&self, id: &DefinitionId) -> Result<Definition> {
        required_definition(&self.conn, id)
    }

    pub fn create_assertion(&mut self, assertion: Assertion) -> Result<Assertion> {
        let id = assertion.id.clone();
        self.write(|tx| {
            validate_assertion(tx, &assertion)?;
            let (kind, text_value, bool_value, target) = value_columns(&assertion.value);
            tx.execute(
                "INSERT INTO assertions (
                    id, subject_id, definition_id, value_kind, text_value, bool_value,
                    target_entity_id, canon, source_status_raw, provenance_external_ref,
                    provenance_locator, provenance_fragment, provenance_source_revision,
                    scope_note, archived
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    assertion.id.as_str(),
                    assertion.subject_id.as_str(),
                    assertion.definition_id.as_str(),
                    kind,
                    text_value,
                    bool_value,
                    target,
                    assertion.canon.as_str(),
                    assertion.source_status_raw,
                    assertion.provenance.external_ref,
                    assertion.provenance.locator,
                    assertion.provenance.fragment,
                    assertion.provenance.source_revision,
                    assertion.scope_note,
                    i64::from(assertion.archived),
                ],
            )
            .map_err(|err| map_write("id", id.as_str(), err))?;
            required_assertion(tx, &id)
        })
    }

    pub fn assertion(&self, id: &AssertionId) -> Result<Assertion> {
        required_assertion(&self.conn, id)
    }

    pub fn assertions_from_subject(&self, subject_id: &EntityId) -> Result<Vec<Assertion>> {
        list_assertions(
            &self.conn,
            &format!(
                "SELECT {ASSERTION_COLUMNS} FROM assertions WHERE subject_id = ?1 ORDER BY id"
            ),
            params![subject_id.as_str()],
        )
    }

    pub fn assertions_to_target(&self, target_id: &EntityId) -> Result<Vec<Assertion>> {
        list_assertions(
            &self.conn,
            &format!(
                "SELECT {ASSERTION_COLUMNS} FROM assertions WHERE target_entity_id = ?1 ORDER BY id"
            ),
            params![target_id.as_str()],
        )
    }

    pub fn assertions_for(
        &self,
        subject_id: &EntityId,
        definition_id: &DefinitionId,
    ) -> Result<Vec<Assertion>> {
        list_assertions(
            &self.conn,
            &format!(
                "SELECT {ASSERTION_COLUMNS} FROM assertions
                WHERE subject_id = ?1 AND definition_id = ?2
                ORDER BY id"
            ),
            params![subject_id.as_str(), definition_id.as_str()],
        )
    }

    pub fn set_canon(&mut self, id: &AssertionId, canon: Canon) -> Result<Assertion> {
        self.write(|tx| {
            if lookup_assertion_id(tx, id)?.is_none() {
                return Err(Error::validation(
                    ValidationReason::NotFound,
                    "id",
                    Some(id.as_str()),
                ));
            }
            tx.execute(
                "UPDATE assertions SET canon = ?1 WHERE id = ?2",
                params![canon.as_str(), id.as_str()],
            )
            .map_err(|err| map_sql("set canon", err))?;
            required_assertion(tx, id)
        })
    }

    pub fn archive_definition(&mut self, id: &DefinitionId) -> Result<Definition> {
        self.set_definition_archived(id, true)
    }

    pub fn restore_definition(&mut self, id: &DefinitionId) -> Result<Definition> {
        self.set_definition_archived(id, false)
    }

    pub fn archive_assertion(&mut self, id: &AssertionId) -> Result<Assertion> {
        self.set_assertion_archived(id, true)
    }

    pub fn restore_assertion(&mut self, id: &AssertionId) -> Result<Assertion> {
        self.set_assertion_archived(id, false)
    }

    pub fn possible_incompatibilities(&self) -> Result<Vec<PossibleIncompatibility>> {
        let id_pairs = {
            let mut statement = self
                .conn
                .prepare(INCOMPATIBLE_PAIRS)
                .map_err(|err| map_sql("prepare", err))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|err| map_sql("query", err))?;
            let mut pairs = Vec::new();
            for row in rows {
                pairs.push(row.map_err(|err| map_sql("row", err))?);
            }
            pairs
        };
        let mut pairs = Vec::new();
        for (first_raw, second_raw) in id_pairs {
            let first_id = parse_stored_id("assertion", &first_raw, AssertionId::parse)?;
            let second_id = parse_stored_id("assertion", &second_raw, AssertionId::parse)?;
            pairs.push(PossibleIncompatibility {
                first: required_assertion(&self.conn, &first_id)?,
                second: required_assertion(&self.conn, &second_id)?,
            });
        }
        Ok(pairs)
    }

    fn set_definition_archived(&mut self, id: &DefinitionId, archived: bool) -> Result<Definition> {
        self.write(|tx| {
            if lookup_definition(tx, id)?.is_none() {
                return Err(Error::validation(
                    ValidationReason::NotFound,
                    "id",
                    Some(id.as_str()),
                ));
            }
            tx.execute(
                "UPDATE definitions SET archived = ?1 WHERE id = ?2",
                params![i64::from(archived), id.as_str()],
            )
            .map_err(|err| map_sql("definition archived", err))?;
            required_definition(tx, id)
        })
    }

    fn set_assertion_archived(&mut self, id: &AssertionId, archived: bool) -> Result<Assertion> {
        self.write(|tx| {
            if lookup_assertion_id(tx, id)?.is_none() {
                return Err(Error::validation(
                    ValidationReason::NotFound,
                    "id",
                    Some(id.as_str()),
                ));
            }
            tx.execute(
                "UPDATE assertions SET archived = ?1 WHERE id = ?2",
                params![i64::from(archived), id.as_str()],
            )
            .map_err(|err| map_sql("assertion archived", err))?;
            required_assertion(tx, id)
        })
    }

    pub fn export_snapshot(&mut self) -> Result<Snapshot> {
        let transaction = self
            .conn
            .transaction()
            .map_err(|err| map_sql("begin", err))?;
        let snapshot = Snapshot {
            universe_id: self.universe_id.clone(),
            pages: list_page_snapshots(&transaction)?,
            types: list_types(&transaction)?,
            entities: list_entities(&transaction)?,
            definitions: list_definitions(&transaction)?,
            assertions: list_assertions(
                &transaction,
                &format!("SELECT {ASSERTION_COLUMNS} FROM assertions ORDER BY id"),
                params![],
            )?,
        };
        transaction.commit().map_err(|err| map_sql("commit", err))?;
        Ok(snapshot)
    }

    pub fn import_snapshot(&mut self, snapshot: Snapshot) -> Result<ImportOutcome> {
        ensure_unique_snapshot(&snapshot)?;
        let universe_id = self.universe_id.clone();
        let snapshot = normalize_snapshot(snapshot);
        self.write(|tx| {
            if snapshot.universe_id != universe_id {
                return Err(Error::WrongUniverse {
                    expected: universe_id.to_string(),
                    found: snapshot.universe_id.to_string(),
                });
            }
            let mut outcome = ImportOutcome {
                inserted: 0,
                unchanged: 0,
            };
            import_records(tx, &snapshot, &mut outcome)?;
            Ok(outcome)
        })
    }

    pub fn import_new(path: &Path, snapshot: Snapshot) -> Result<(Self, ImportOutcome)> {
        ensure_unique_snapshot(&snapshot)?;
        let snapshot = normalize_snapshot(snapshot);
        reserve_new_file(path)?;
        let mut conn = match open_read_write(path) {
            Ok(conn) => conn,
            Err(err) => return Err(residual(path, err)),
        };
        let result = (|| {
            prepare_connection(&conn)?;
            set_delete_journal(&conn)?;
            let transaction = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|err| map_sql("begin", err))?;
            write_schema(&transaction, &snapshot.universe_id)?;
            let mut outcome = ImportOutcome {
                inserted: 0,
                unchanged: 0,
            };
            import_records(&transaction, &snapshot, &mut outcome)?;
            transaction.commit().map_err(|err| map_sql("commit", err))?;
            Ok((snapshot.universe_id.clone(), outcome))
        })();
        match result {
            Ok((universe_id, outcome)) => Ok((Self { conn, universe_id }, outcome)),
            Err(err) => {
                drop(conn);
                Err(residual(path, err))
            }
        }
    }

    fn write<T>(&mut self, body: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| map_sql("begin", err))?;
        let value = body(&transaction)?;
        transaction.commit().map_err(|err| map_sql("commit", err))?;
        Ok(value)
    }
}

fn init_new(conn: &mut Connection, universe_id: &UniverseId) -> Result<()> {
    // The named test thread asks initialization to fail after the file exists.
    #[cfg(test)]
    if std::thread::current().name() == Some("fail-after-reserve") {
        return Err(Error::Storage {
            message: "injected initialization failure".to_string(),
        });
    }
    prepare_connection(conn)?;
    set_delete_journal(conn)?;
    let transaction = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| map_sql("begin", err))?;
    write_schema(&transaction, universe_id)?;
    transaction.commit().map_err(|err| map_sql("commit", err))?;
    Ok(())
}

fn write_schema(transaction: &Transaction, universe_id: &UniverseId) -> Result<()> {
    transaction
        .execute_batch(SCHEMA)
        .map_err(|err| map_sql("schema", err))?;
    transaction
        .execute(
            "INSERT INTO metadata (singleton, universe_id) VALUES (1, ?1)",
            params![universe_id.as_str()],
        )
        .map_err(|err| map_sql("metadata", err))?;
    // ROLLBACK undoes PRAGMA user_version on this bundled SQLite, so the pragma is committed with the schema.
    let sql = format!("PRAGMA user_version = {}", crate::SCHEMA_VERSION);
    transaction
        .execute_batch(&sql)
        .map_err(|err| map_sql("user_version", err))?;
    Ok(())
}

fn reserve_new_file(path: &Path) -> Result<()> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => {
            drop(file);
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Err(Error::validation(
            ValidationReason::PathExists,
            "path",
            None,
        )),
        Err(err) => Err(Error::Io {
            message: format!("reserve {}: {err}", path.display()),
        }),
    }
}

fn open_read_only(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|err| map_sql("open", err))?;
    prepare_connection(&conn)?;
    Ok(conn)
}

fn prepare_connection(conn: &Connection) -> Result<()> {
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|err| map_sql("busy_timeout", err))?;
    conn.execute_batch("PRAGMA foreign_keys = ON")
        .map_err(|err| map_sql("foreign_keys", err))?;
    let enabled: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .map_err(|err| map_sql("foreign_keys", err))?;
    if enabled != 1 {
        return Err(Error::Storage {
            message: "foreign_keys pragma stayed off".to_string(),
        });
    }
    Ok(())
}

fn set_delete_journal(conn: &Connection) -> Result<()> {
    let mode: String = conn
        .query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
        .map_err(|err| map_sql("journal_mode", err))?;
    if mode != "delete" {
        return Err(Error::Storage {
            message: format!("journal_mode returned {mode}"),
        });
    }
    Ok(())
}

fn require_delete_journal(conn: &Connection) -> Result<()> {
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(|err| map_sql("journal_mode", err))?;
    if mode != "delete" {
        return Err(Error::validation(
            ValidationReason::ForeignDatabase,
            "journal_mode",
            None,
        ));
    }
    Ok(())
}

fn require_tables(conn: &Connection) -> Result<()> {
    for name in [
        "assertions",
        "definitions",
        "entities",
        "mentions",
        "metadata",
        "pages",
        "types",
    ] {
        let found: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [name],
                |row| row.get(0),
            )
            .map_err(|err| map_sql("schema", err))?;
        if found != 1 {
            return Err(Error::validation(
                ValidationReason::ForeignDatabase,
                "schema",
                None,
            ));
        }
    }
    Ok(())
}

fn read_universe(conn: &Connection) -> Result<UniverseId> {
    let row = conn
        .query_row(
            "SELECT universe_id FROM metadata WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| map_sql("metadata", err))?;
    let Some(raw) = row else {
        return Err(Error::validation(
            ValidationReason::MissingMetadata,
            "metadata",
            None,
        ));
    };
    UniverseId::parse(&raw).map_err(|_| {
        Error::validation(ValidationReason::ForeignDatabase, "universe_id", Some(&raw))
    })
}

/// Tells a historical version 1 database apart from a foreign file.
///
/// A version 1 database keeps its format version inside the metadata table.
/// A file that only claims the version number is not one of ours.
fn is_legacy_v1(conn: &Connection) -> Result<bool> {
    let mut statement = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .map_err(|err| map_sql("schema", err))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| map_sql("schema", err))?;
    let mut tables: Vec<String> = Vec::new();
    for row in rows {
        tables.push(row.map_err(|err| map_sql("schema", err))?);
    }
    for name in [
        "assertions",
        "definitions",
        "entities",
        "metadata",
        "pages",
        "types",
    ] {
        if !tables.iter().any(|table| table == name) {
            return Ok(false);
        }
    }
    let mut statement = conn
        .prepare("PRAGMA table_info(metadata)")
        .map_err(|err| map_sql("schema", err))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| map_sql("schema", err))?;
    for row in rows {
        if row.map_err(|err| map_sql("schema", err))? == "format_version" {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Decides what a historical database needs, without touching it.
///
/// The file is only inspected through a read-only connection, so a version 1
/// database is never modified, upgraded or copied.
fn legacy_outcome(path: &Path) -> Result<Error> {
    let inspected = open_read_only(path)?;
    if is_legacy_v1(&inspected)? {
        return Ok(Error::MigrationRequired {
            found: LEGACY_SCHEMA_VERSION,
        });
    }
    Ok(Error::validation(
        ValidationReason::ForeignDatabase,
        "schema",
        None,
    ))
}

struct Header {
    user_version: i32,
}

fn read_header(path: &Path) -> Result<Header> {
    let mut file = std::fs::File::open(path).map_err(|err| Error::Io {
        message: format!("open {}: {err}", path.display()),
    })?;
    let mut buf = [0u8; HEADER_LEN];
    match file.read_exact(&mut buf) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(Error::validation(
                ValidationReason::NotADatabase,
                "path",
                None,
            ));
        }
        Err(err) => {
            return Err(Error::Io {
                message: format!("read header {}: {err}", path.display()),
            });
        }
    }
    if &buf[..16] != MAGIC {
        return Err(Error::validation(
            ValidationReason::NotADatabase,
            "path",
            None,
        ));
    }
    Ok(Header {
        user_version: i32::from_be_bytes([buf[60], buf[61], buf[62], buf[63]]),
    })
}

fn open_read_write(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|err| map_sql("open", err))
}

fn residual(path: &Path, err: Error) -> Error {
    let note = format!("residual file {}", path.display());
    match err {
        Error::Storage { message } => Error::Storage {
            message: format!("{message}; {note}"),
        },
        Error::Io { message } => Error::Io {
            message: format!("{message}; {note}"),
        },
        other => Error::Io {
            message: format!("{other}; {note}"),
        },
    }
}

fn ensure_unique_snapshot(snapshot: &Snapshot) -> Result<()> {
    unique_ids(
        snapshot.pages.iter().map(|page| page.page.id.as_str()),
        "page",
    )?;
    unique_mention_ids(snapshot)?;
    unique_ids(
        snapshot
            .types
            .iter()
            .map(|entity_type| entity_type.id.as_str()),
        "type",
    )?;
    unique_ids(
        snapshot.entities.iter().map(|entity| entity.id.as_str()),
        "entity",
    )?;
    unique_ids(
        snapshot
            .definitions
            .iter()
            .map(|definition| definition.id.as_str()),
        "definition",
    )?;
    unique_ids(
        snapshot
            .assertions
            .iter()
            .map(|assertion| assertion.id.as_str()),
        "assertion",
    )?;
    Ok(())
}

fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>, class: &'static str) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(Error::validation(
                ValidationReason::DuplicateId,
                class,
                Some(id),
            ));
        }
    }
    Ok(())
}

/// An occurrence id belongs to exactly one page, so the same id cannot
/// appear on two pages of the same document.
fn unique_mention_ids(snapshot: &Snapshot) -> Result<()> {
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for page in &snapshot.pages {
        for mention in &page.mentions {
            if !seen.insert(mention.id.as_str()) {
                return Err(Error::validation(
                    ValidationReason::MentionIdConflict,
                    "mention_id",
                    Some(mention.id.as_str()),
                ));
            }
        }
    }
    Ok(())
}

/// Puts the mentions of every page in the canonical order, so two documents
/// with the same occurrences compare equal.
fn normalize_snapshot(mut snapshot: Snapshot) -> Snapshot {
    for page in &mut snapshot.pages {
        page.mentions.sort_by(|left, right| {
            left.start_utf16
                .cmp(&right.start_utf16)
                .then(left.end_utf16.cmp(&right.end_utf16))
                .then(left.id.as_str().cmp(right.id.as_str()))
        });
    }
    snapshot
}

fn import_records(
    tx: &Transaction,
    snapshot: &Snapshot,
    outcome: &mut ImportOutcome,
) -> Result<()> {
    let mut inserted_pages: Vec<&PageSnapshot> = Vec::new();
    for page_snapshot in &snapshot.pages {
        let annotated = AnnotatedBody {
            body: page_snapshot.page.body.clone(),
            mentions: page_snapshot.mentions.clone(),
        };
        validate_mentions(&annotated)?;
        match lookup_page_snapshot(tx, &page_snapshot.page.id)? {
            Some(existing) => {
                if existing != *page_snapshot {
                    return Err(Error::ImportConflict {
                        class: "page",
                        id: page_snapshot.page.id.as_str().to_string(),
                    });
                }
                outcome.unchanged += 1;
            }
            None => {
                insert_page(tx, &page_snapshot.page)?;
                outcome.inserted += 1;
                inserted_pages.push(page_snapshot);
            }
        }
    }
    for entity_type in &snapshot.types {
        reconcile(
            outcome,
            "type",
            entity_type.id.as_str(),
            lookup_entity_type(tx, &entity_type.id)?,
            entity_type,
            || insert_type(tx, entity_type),
        )?;
    }
    for entity in &snapshot.entities {
        reconcile(
            outcome,
            "entity",
            entity.id.as_str(),
            lookup_entity(tx, &entity.id)?,
            entity,
            || {
                require_entity_links(tx, entity)?;
                insert_entity(tx, entity)
            },
        )?;
    }
    for definition in &snapshot.definitions {
        reconcile(
            outcome,
            "definition",
            definition.id.as_str(),
            lookup_definition(tx, &definition.id)?,
            definition,
            || {
                require_definition_owner(tx, definition)?;
                insert_definition(tx, definition)
            },
        )?;
    }
    for assertion in &snapshot.assertions {
        reconcile(
            outcome,
            "assertion",
            assertion.id.as_str(),
            lookup_assertion(tx, &assertion.id)?,
            assertion,
            || {
                validate_imported_assertion(tx, assertion)?;
                insert_assertion(tx, assertion)
            },
        )?;
    }
    // The mentions go in after the entities exist, so a document may point at
    // an entity that the same import creates.
    for page_snapshot in inserted_pages {
        for mention in &page_snapshot.mentions {
            if lookup_entity(tx, &mention.entity_id)?.is_none() {
                return Err(Error::validation(
                    ValidationReason::UnknownEntity,
                    "entity_id",
                    Some(mention.entity_id.as_str()),
                ));
            }
            if let Some(owner) = lookup_mention_owner(tx, &mention.id)? {
                if owner != page_snapshot.page.id {
                    return Err(Error::validation(
                        ValidationReason::MentionIdConflict,
                        "mention_id",
                        Some(mention.id.as_str()),
                    ));
                }
            }
            insert_mention(tx, &page_snapshot.page.id, mention)?;
        }
    }
    Ok(())
}

fn reconcile<T: PartialEq>(
    outcome: &mut ImportOutcome,
    class: &'static str,
    id: &str,
    existing: Option<T>,
    incoming: &T,
    insert: impl FnOnce() -> Result<()>,
) -> Result<()> {
    match existing {
        Some(existing) if &existing == incoming => {
            outcome.unchanged += 1;
            Ok(())
        }
        Some(_) => Err(Error::ImportConflict {
            class,
            id: id.to_string(),
        }),
        None => {
            insert()?;
            outcome.inserted += 1;
            Ok(())
        }
    }
}

fn require_entity_links(tx: &Transaction, entity: &Entity) -> Result<()> {
    if let Some(type_id) = &entity.type_id {
        if lookup_entity_type(tx, type_id)?.is_none() {
            return Err(Error::validation(
                ValidationReason::UnknownType,
                "type_id",
                Some(type_id.as_str()),
            ));
        }
    }
    if let Some(page_id) = &entity.page_id {
        if lookup_page(tx, page_id)?.is_none() {
            return Err(Error::validation(
                ValidationReason::UnknownPage,
                "page_id",
                Some(page_id.as_str()),
            ));
        }
    }
    Ok(())
}

fn require_definition_owner(tx: &Transaction, definition: &Definition) -> Result<()> {
    match &definition.owner {
        Owner::Type(type_id) => {
            if lookup_entity_type(tx, type_id)?.is_none() {
                return Err(Error::validation(
                    ValidationReason::UnknownType,
                    "owner_type_id",
                    Some(type_id.as_str()),
                ));
            }
        }
        Owner::Entity(entity_id) => {
            if lookup_entity(tx, entity_id)?.is_none() {
                return Err(Error::validation(
                    ValidationReason::UnknownEntity,
                    "owner_entity_id",
                    Some(entity_id.as_str()),
                ));
            }
        }
    }
    Ok(())
}

fn insert_page(tx: &Transaction, page: &Page) -> Result<()> {
    tx.execute(
        "INSERT INTO pages (id, title, body) VALUES (?1, ?2, ?3)",
        params![page.id.as_str(), page.title, page.body],
    )
    .map(|_| ())
    .map_err(|err| map_write("id", page.id.as_str(), err))
}

fn insert_mention(tx: &Transaction, page_id: &PageId, mention: &Mention) -> Result<()> {
    tx.execute(
        "INSERT INTO mentions (id, page_id, entity_id, start_utf16, end_utf16)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            mention.id.as_str(),
            page_id.as_str(),
            mention.entity_id.as_str(),
            mention.start_utf16,
            mention.end_utf16,
        ],
    )
    .map(|_| ())
    .map_err(|err| map_write("mention", mention.id.as_str(), err))
}

fn insert_type(tx: &Transaction, entity_type: &EntityType) -> Result<()> {
    tx.execute(
        "INSERT INTO types (id, label) VALUES (?1, ?2)",
        params![entity_type.id.as_str(), entity_type.label],
    )
    .map(|_| ())
    .map_err(|err| map_write("id", entity_type.id.as_str(), err))
}

fn insert_entity(tx: &Transaction, entity: &Entity) -> Result<()> {
    tx.execute(
        "INSERT INTO entities (id, label, type_id, page_id) VALUES (?1, ?2, ?3, ?4)",
        params![
            entity.id.as_str(),
            entity.label,
            entity.type_id.as_ref().map(TypeId::as_str),
            entity.page_id.as_ref().map(PageId::as_str),
        ],
    )
    .map(|_| ())
    .map_err(|err| map_write("id", entity.id.as_str(), err))
}

fn insert_definition(tx: &Transaction, definition: &Definition) -> Result<()> {
    let (owner_type, owner_entity) = match &definition.owner {
        Owner::Type(type_id) => (Some(type_id.as_str()), None),
        Owner::Entity(entity_id) => (None, Some(entity_id.as_str())),
    };
    tx.execute(
        "INSERT INTO definitions (
            id, owner_type_id, owner_entity_id, value_class, cardinality, archived
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            definition.id.as_str(),
            owner_type,
            owner_entity,
            definition.value_class.as_str(),
            definition.cardinality.as_str(),
            i64::from(definition.archived),
        ],
    )
    .map(|_| ())
    .map_err(|err| map_write("id", definition.id.as_str(), err))
}

fn insert_assertion(tx: &Transaction, assertion: &Assertion) -> Result<()> {
    let (kind, text_value, bool_value, target) = value_columns(&assertion.value);
    tx.execute(
        "INSERT INTO assertions (
            id, subject_id, definition_id, value_kind, text_value, bool_value,
            target_entity_id, canon, source_status_raw, provenance_external_ref,
            provenance_locator, provenance_fragment, provenance_source_revision,
            scope_note, archived
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            assertion.id.as_str(),
            assertion.subject_id.as_str(),
            assertion.definition_id.as_str(),
            kind,
            text_value,
            bool_value,
            target,
            assertion.canon.as_str(),
            assertion.source_status_raw,
            assertion.provenance.external_ref,
            assertion.provenance.locator,
            assertion.provenance.fragment,
            assertion.provenance.source_revision,
            assertion.scope_note,
            i64::from(assertion.archived),
        ],
    )
    .map(|_| ())
    .map_err(|err| map_write("id", assertion.id.as_str(), err))
}

fn lookup_assertion(conn: &Connection, id: &AssertionId) -> Result<Option<Assertion>> {
    let found = list_assertions(
        conn,
        &format!("SELECT {ASSERTION_COLUMNS} FROM assertions WHERE id = ?1"),
        params![id.as_str()],
    )?;
    Ok(found.into_iter().next())
}

fn list_pages(conn: &Connection) -> Result<Vec<Page>> {
    let mut statement = conn
        .prepare("SELECT id, title, body FROM pages ORDER BY id")
        .map_err(|err| map_sql("prepare", err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|err| map_sql("query", err))?;
    let mut pages = Vec::new();
    for row in rows {
        let (id, title, body) = row.map_err(|err| map_sql("row", err))?;
        pages.push(Page {
            id: parse_stored_id("page", &id, PageId::parse)?,
            title,
            body,
        });
    }
    Ok(pages)
}

/// Reads every page with its complete canonical mention set.
fn list_page_snapshots(conn: &Connection) -> Result<Vec<PageSnapshot>> {
    let mut snapshots = Vec::new();
    for page in list_pages(conn)? {
        let mentions = read_page_annotated(conn, &page.id, &page.body)?.mentions;
        snapshots.push(PageSnapshot { page, mentions });
    }
    Ok(snapshots)
}

fn lookup_page_snapshot(conn: &Connection, id: &PageId) -> Result<Option<PageSnapshot>> {
    let Some(page) = lookup_page(conn, id)? else {
        return Ok(None);
    };
    let mentions = read_page_annotated(conn, id, &page.body)?.mentions;
    Ok(Some(PageSnapshot { page, mentions }))
}

fn list_types(conn: &Connection) -> Result<Vec<EntityType>> {
    let mut statement = conn
        .prepare("SELECT id, label FROM types ORDER BY id")
        .map_err(|err| map_sql("prepare", err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| map_sql("query", err))?;
    let mut types = Vec::new();
    for row in rows {
        let (id, label) = row.map_err(|err| map_sql("row", err))?;
        types.push(EntityType {
            id: parse_stored_id("type", &id, TypeId::parse)?,
            label,
        });
    }
    Ok(types)
}

fn list_entities(conn: &Connection) -> Result<Vec<Entity>> {
    let mut statement = conn
        .prepare("SELECT id, label, type_id, page_id FROM entities ORDER BY id")
        .map_err(|err| map_sql("prepare", err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|err| map_sql("query", err))?;
    let mut entities = Vec::new();
    for row in rows {
        let (id, label, type_id, page_id) = row.map_err(|err| map_sql("row", err))?;
        entities.push(Entity {
            id: parse_stored_id("entity", &id, EntityId::parse)?,
            label,
            type_id: match type_id {
                Some(raw) => Some(parse_stored_id("entity type", &raw, TypeId::parse)?),
                None => None,
            },
            page_id: match page_id {
                Some(raw) => Some(parse_stored_id("entity page", &raw, PageId::parse)?),
                None => None,
            },
        });
    }
    Ok(entities)
}

fn list_definitions(conn: &Connection) -> Result<Vec<Definition>> {
    let mut statement = conn
        .prepare(
            "SELECT id, owner_type_id, owner_entity_id, value_class, cardinality, archived
             FROM definitions ORDER BY id",
        )
        .map_err(|err| map_sql("prepare", err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|err| map_sql("query", err))?;
    let mut definitions = Vec::new();
    for row in rows {
        let (id, owner_type, owner_entity, value_class, cardinality, archived) =
            row.map_err(|err| map_sql("row", err))?;
        let owner = match (owner_type, owner_entity) {
            (Some(raw), None) => Owner::Type(parse_stored_id("owner type", &raw, TypeId::parse)?),
            (None, Some(raw)) => {
                Owner::Entity(parse_stored_id("owner entity", &raw, EntityId::parse)?)
            }
            _ => {
                return Err(Error::Storage {
                    message: format!("invalid definition owner {id}"),
                });
            }
        };
        definitions.push(Definition {
            id: parse_stored_id("definition", &id, DefinitionId::parse)?,
            owner,
            value_class: ValueClass::parse(&value_class)?,
            cardinality: Cardinality::parse(&cardinality)?,
            archived: read_flag(archived, "archived")?,
        });
    }
    Ok(definitions)
}

fn count(conn: &Connection, sql: &str) -> Result<u64> {
    let n: i64 = conn
        .query_row(sql, [], |row| row.get(0))
        .map_err(|err| map_sql("count", err))?;
    u64::try_from(n).map_err(|_| Error::Storage {
        message: format!("negative count {n}"),
    })
}

fn validate_assertion(tx: &Transaction, assertion: &Assertion) -> Result<()> {
    validate_assertion_in(tx, assertion, false)
}

fn validate_imported_assertion(tx: &Transaction, assertion: &Assertion) -> Result<()> {
    validate_assertion_in(tx, assertion, true)
}

fn validate_assertion_in(
    tx: &Transaction,
    assertion: &Assertion,
    allow_archived_definition: bool,
) -> Result<()> {
    let subject = lookup_entity(tx, &assertion.subject_id)?.ok_or_else(|| {
        Error::validation(
            ValidationReason::UnknownSubject,
            "subject_id",
            Some(assertion.subject_id.as_str()),
        )
    })?;
    let definition = lookup_definition(tx, &assertion.definition_id)?.ok_or_else(|| {
        Error::validation(
            ValidationReason::UnknownDefinition,
            "definition_id",
            Some(assertion.definition_id.as_str()),
        )
    })?;
    if definition.archived && !allow_archived_definition {
        return Err(Error::validation(
            ValidationReason::ArchivedDefinition,
            "definition_id",
            Some(definition.id.as_str()),
        ));
    }
    if !value_fits(definition.value_class, &assertion.value) {
        return Err(Error::validation(
            ValidationReason::ValueMismatch,
            "value",
            Some(assertion.id.as_str()),
        ));
    }
    let applicable = match &definition.owner {
        Owner::Entity(owner) => owner == &subject.id,
        Owner::Type(owner) => subject.type_id.as_ref() == Some(owner),
    };
    if !applicable {
        return Err(Error::validation(
            ValidationReason::NotApplicable,
            "definition_id",
            Some(definition.id.as_str()),
        ));
    }
    if let Value::Reference(target) = &assertion.value {
        if lookup_entity(tx, target)?.is_none() {
            return Err(Error::validation(
                ValidationReason::UnknownTarget,
                "target_entity_id",
                Some(target.as_str()),
            ));
        }
    }
    Ok(())
}

fn value_columns(value: &Value) -> (&'static str, Option<&str>, Option<i64>, Option<&str>) {
    match value {
        Value::Text(text) => ("text", Some(text.as_str()), None, None),
        Value::Boolean(flag) => ("boolean", None, Some(i64::from(*flag)), None),
        Value::Reference(id) => ("reference", None, None, Some(id.as_str())),
        Value::Unknown => ("unknown", None, None, None),
    }
}

struct AssertionParts {
    id: String,
    subject_id: String,
    definition_id: String,
    value_kind: String,
    text_value: Option<String>,
    bool_value: Option<i64>,
    target_entity_id: Option<String>,
    canon: String,
    source_status_raw: Option<String>,
    provenance_external_ref: Option<String>,
    provenance_locator: Option<String>,
    provenance_fragment: Option<String>,
    provenance_source_revision: Option<String>,
    scope_note: Option<String>,
    archived: i64,
}

fn read_assertion_parts(row: &rusqlite::Row<'_>) -> rusqlite::Result<AssertionParts> {
    Ok(AssertionParts {
        id: row.get(0)?,
        subject_id: row.get(1)?,
        definition_id: row.get(2)?,
        value_kind: row.get(3)?,
        text_value: row.get(4)?,
        bool_value: row.get(5)?,
        target_entity_id: row.get(6)?,
        canon: row.get(7)?,
        source_status_raw: row.get(8)?,
        provenance_external_ref: row.get(9)?,
        provenance_locator: row.get(10)?,
        provenance_fragment: row.get(11)?,
        provenance_source_revision: row.get(12)?,
        scope_note: row.get(13)?,
        archived: row.get(14)?,
    })
}

fn assertion_from_parts(parts: AssertionParts) -> Result<Assertion> {
    let id = parse_stored_id("assertion", &parts.id, AssertionId::parse)?;
    let subject_id = parse_stored_id("subject", &parts.subject_id, EntityId::parse)?;
    let definition_id = parse_stored_id("definition", &parts.definition_id, DefinitionId::parse)?;
    let value = match parts.value_kind.as_str() {
        "text" => match parts.text_value {
            Some(text) if parts.bool_value.is_none() && parts.target_entity_id.is_none() => {
                Value::Text(text)
            }
            _ => {
                return Err(Error::Storage {
                    message: format!("invalid text assertion {id}"),
                });
            }
        },
        "boolean" => match parts.bool_value {
            Some(0) if parts.text_value.is_none() && parts.target_entity_id.is_none() => {
                Value::Boolean(false)
            }
            Some(1) if parts.text_value.is_none() && parts.target_entity_id.is_none() => {
                Value::Boolean(true)
            }
            _ => {
                return Err(Error::Storage {
                    message: format!("invalid boolean assertion {id}"),
                });
            }
        },
        "reference" => match parts.target_entity_id {
            Some(target) if parts.text_value.is_none() && parts.bool_value.is_none() => {
                Value::Reference(parse_stored_id("target", &target, EntityId::parse)?)
            }
            _ => {
                return Err(Error::Storage {
                    message: format!("invalid reference assertion {id}"),
                });
            }
        },
        "unknown" => {
            if parts.text_value.is_none()
                && parts.bool_value.is_none()
                && parts.target_entity_id.is_none()
            {
                Value::Unknown
            } else {
                return Err(Error::Storage {
                    message: format!("invalid unknown assertion {id}"),
                });
            }
        }
        other => {
            return Err(Error::Storage {
                message: format!("invalid value kind {other} on {id}"),
            });
        }
    };
    Ok(Assertion {
        id,
        subject_id,
        definition_id,
        value,
        canon: Canon::parse(&parts.canon)?,
        source_status_raw: parts.source_status_raw,
        provenance: Provenance {
            external_ref: parts.provenance_external_ref,
            locator: parts.provenance_locator,
            fragment: parts.provenance_fragment,
            source_revision: parts.provenance_source_revision,
        },
        scope_note: parts.scope_note,
        archived: read_flag(parts.archived, "archived")?,
    })
}

fn parse_stored_id<T>(field: &str, raw: &str, parse: impl FnOnce(&str) -> Result<T>) -> Result<T> {
    parse(raw).map_err(|_| Error::Storage {
        message: format!("invalid stored {field} id {raw}"),
    })
}

fn read_flag(value: i64, field: &str) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(Error::Storage {
            message: format!("invalid {field} {other}"),
        }),
    }
}

fn list_assertions(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Assertion>> {
    let mut statement = conn.prepare(sql).map_err(|err| map_sql("prepare", err))?;
    let rows = statement
        .query_map(params, read_assertion_parts)
        .map_err(|err| map_sql("query", err))?;
    let mut assertions = Vec::new();
    for row in rows {
        let parts = row.map_err(|err| map_sql("row", err))?;
        assertions.push(assertion_from_parts(parts)?);
    }
    Ok(assertions)
}

fn lookup_page(conn: &Connection, id: &PageId) -> Result<Option<Page>> {
    conn.query_row(
        "SELECT id, title, body FROM pages WHERE id = ?1",
        [id.as_str()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )
    .optional()
    .map_err(|err| map_sql("page", err))?
    .map(|(raw_id, title, body)| {
        Ok(Page {
            id: parse_stored_id("page", &raw_id, PageId::parse)?,
            title,
            body,
        })
    })
    .transpose()
}

fn required_page(conn: &Connection, id: &PageId) -> Result<Page> {
    lookup_page(conn, id)?
        .ok_or_else(|| Error::validation(ValidationReason::NotFound, "id", Some(id.as_str())))
}

/// One stored page row with its local revision.
struct PageRow {
    title: String,
    body: String,
    revision: PageRevision,
}

fn require_page_row(conn: &Connection, id: &PageId) -> Result<PageRow> {
    let row = conn
        .query_row(
            "SELECT title, body, revision FROM pages WHERE id = ?1",
            [id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|err| map_sql("page", err))?;
    let Some((title, body, raw_revision)) = row else {
        return Err(Error::validation(
            ValidationReason::NotFound,
            "id",
            Some(id.as_str()),
        ));
    };
    let revision = PageRevision::new(raw_revision).map_err(|_| Error::Storage {
        message: format!(
            "invalid stored revision {raw_revision} on page {}",
            id.as_str()
        ),
    })?;
    Ok(PageRow {
        title,
        body,
        revision,
    })
}

/// Reads the mentions of a page and checks them against the exact body.
///
/// A stored mention that is out of range, splits a grapheme cluster, or
/// overlaps another one makes the whole page unreadable. Nothing is dropped
/// and nothing is repaired.
fn read_page_annotated(conn: &Connection, page_id: &PageId, body: &str) -> Result<AnnotatedBody> {
    let mut statement = conn
        .prepare(
            "SELECT id, entity_id, start_utf16, end_utf16 FROM mentions
             WHERE page_id = ?1 ORDER BY start_utf16, end_utf16, id",
        )
        .map_err(|err| map_sql("prepare", err))?;
    let rows = statement
        .query_map([page_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|err| map_sql("query", err))?;
    let mut mentions: Vec<Mention> = Vec::new();
    for row in rows {
        let (raw_id, raw_entity, start, end) = row.map_err(|err| map_sql("row", err))?;
        let id = MentionId::parse(&raw_id).map_err(|_| Error::Storage {
            message: format!("invalid stored mention id {raw_id}"),
        })?;
        let entity_id = EntityId::parse(&raw_entity).map_err(|_| Error::Storage {
            message: format!("invalid stored mention entity id {raw_entity}"),
        })?;
        let start = u32::try_from(start).map_err(|_| Error::Storage {
            message: format!("invalid stored mention offset {start}"),
        })?;
        let end = u32::try_from(end).map_err(|_| Error::Storage {
            message: format!("invalid stored mention offset {end}"),
        })?;
        mentions.push(Mention {
            id,
            entity_id,
            start_utf16: start,
            end_utf16: end,
        });
    }
    let annotated = AnnotatedBody {
        body: body.to_string(),
        mentions,
    };
    validate_mentions(&annotated).map_err(|err| Error::Storage {
        message: format!(
            "stored mentions of page {} are invalid: {err}",
            page_id.as_str()
        ),
    })?;
    Ok(annotated)
}

fn require_page_aggregate(conn: &Connection, id: &PageId) -> Result<PageDocument> {
    let row = require_page_row(conn, id)?;
    let body = read_page_annotated(conn, id, &row.body)?;
    Ok(PageDocument {
        id: id.clone(),
        title: row.title,
        body,
        revision: row.revision,
    })
}

fn lookup_mention_owner(conn: &Connection, id: &MentionId) -> Result<Option<PageId>> {
    let found = conn
        .query_row(
            "SELECT page_id FROM mentions WHERE id = ?1",
            [id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| map_sql("mention", err))?;
    let Some(raw) = found else {
        return Ok(None);
    };
    Ok(Some(PageId::parse(&raw).map_err(|_| Error::Storage {
        message: format!("invalid stored mention page id {raw}"),
    })?))
}

fn mentions_for_entity_in(conn: &Connection, entity: &EntityId) -> Result<Vec<MentionOccurrence>> {
    if lookup_entity(conn, entity)?.is_none() {
        return Err(Error::validation(
            ValidationReason::NotFound,
            "id",
            Some(entity.as_str()),
        ));
    }
    let mut statement = conn
        .prepare(
            "SELECT page_id, id, start_utf16, end_utf16 FROM mentions
             WHERE entity_id = ?1 ORDER BY page_id, start_utf16, end_utf16, id",
        )
        .map_err(|err| map_sql("prepare", err))?;
    let rows = statement
        .query_map([entity.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|err| map_sql("query", err))?;
    let mut by_page: BTreeMap<String, (PageId, Vec<Mention>)> = BTreeMap::new();
    for row in rows {
        let (raw_page, raw_id, start, end) = row.map_err(|err| map_sql("row", err))?;
        let page_id = PageId::parse(&raw_page).map_err(|_| Error::Storage {
            message: format!("invalid stored mention page id {raw_page}"),
        })?;
        let id = MentionId::parse(&raw_id).map_err(|_| Error::Storage {
            message: format!("invalid stored mention id {raw_id}"),
        })?;
        let start = u32::try_from(start).map_err(|_| Error::Storage {
            message: format!("invalid stored mention offset {start}"),
        })?;
        let end = u32::try_from(end).map_err(|_| Error::Storage {
            message: format!("invalid stored mention offset {end}"),
        })?;
        let entry = by_page
            .entry(page_id.as_str().to_string())
            .or_insert_with(|| (page_id, Vec::new()));
        entry.1.push(Mention {
            id,
            entity_id: entity.clone(),
            start_utf16: start,
            end_utf16: end,
        });
    }
    let mut occurrences: Vec<MentionOccurrence> = Vec::new();
    for (_key, (page_id, mentions)) in by_page {
        let row = require_page_row(conn, &page_id)?;
        read_page_annotated(conn, &page_id, &row.body)?;
        for mention in mentions {
            occurrences.push(MentionOccurrence {
                page_id: page_id.clone(),
                page_revision: row.revision,
                mention,
            });
        }
    }
    Ok(occurrences)
}

fn lookup_entity_type(conn: &Connection, id: &TypeId) -> Result<Option<EntityType>> {
    conn.query_row(
        "SELECT id, label FROM types WHERE id = ?1",
        [id.as_str()],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .optional()
    .map_err(|err| map_sql("type", err))?
    .map(|(raw_id, label)| {
        Ok(EntityType {
            id: parse_stored_id("type", &raw_id, TypeId::parse)?,
            label,
        })
    })
    .transpose()
}

fn required_entity_type(conn: &Connection, id: &TypeId) -> Result<EntityType> {
    lookup_entity_type(conn, id)?
        .ok_or_else(|| Error::validation(ValidationReason::NotFound, "id", Some(id.as_str())))
}

fn lookup_entity(conn: &Connection, id: &EntityId) -> Result<Option<Entity>> {
    conn.query_row(
        "SELECT id, label, type_id, page_id FROM entities WHERE id = ?1",
        [id.as_str()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )
    .optional()
    .map_err(|err| map_sql("entity", err))?
    .map(|(raw_id, label, type_id, page_id)| {
        Ok(Entity {
            id: parse_stored_id("entity", &raw_id, EntityId::parse)?,
            label,
            type_id: match type_id {
                Some(raw) => Some(parse_stored_id("entity type", &raw, TypeId::parse)?),
                None => None,
            },
            page_id: match page_id {
                Some(raw) => Some(parse_stored_id("entity page", &raw, PageId::parse)?),
                None => None,
            },
        })
    })
    .transpose()
}

fn required_entity(conn: &Connection, id: &EntityId) -> Result<Entity> {
    lookup_entity(conn, id)?
        .ok_or_else(|| Error::validation(ValidationReason::NotFound, "id", Some(id.as_str())))
}

fn lookup_definition(conn: &Connection, id: &DefinitionId) -> Result<Option<Definition>> {
    conn.query_row(
        "SELECT id, owner_type_id, owner_entity_id, value_class, cardinality, archived
         FROM definitions WHERE id = ?1",
        [id.as_str()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        },
    )
    .optional()
    .map_err(|err| map_sql("definition", err))?
    .map(
        |(raw_id, owner_type, owner_entity, value_class, cardinality, archived)| {
            let owner = match (owner_type, owner_entity) {
                (Some(raw), None) => {
                    Owner::Type(parse_stored_id("owner type", &raw, TypeId::parse)?)
                }
                (None, Some(raw)) => {
                    Owner::Entity(parse_stored_id("owner entity", &raw, EntityId::parse)?)
                }
                _ => {
                    return Err(Error::Storage {
                        message: format!("invalid definition owner {raw_id}"),
                    });
                }
            };
            Ok(Definition {
                id: parse_stored_id("definition", &raw_id, DefinitionId::parse)?,
                owner,
                value_class: ValueClass::parse(&value_class)?,
                cardinality: Cardinality::parse(&cardinality)?,
                archived: read_flag(archived, "archived")?,
            })
        },
    )
    .transpose()
}

fn required_definition(conn: &Connection, id: &DefinitionId) -> Result<Definition> {
    lookup_definition(conn, id)?
        .ok_or_else(|| Error::validation(ValidationReason::NotFound, "id", Some(id.as_str())))
}

fn lookup_assertion_id(conn: &Connection, id: &AssertionId) -> Result<Option<()>> {
    let found: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assertions WHERE id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )
        .map_err(|err| map_sql("assertion", err))?;
    Ok(if found == 1 { Some(()) } else { None })
}

fn required_assertion(conn: &Connection, id: &AssertionId) -> Result<Assertion> {
    list_assertions(
        conn,
        &format!("SELECT {ASSERTION_COLUMNS} FROM assertions WHERE id = ?1"),
        params![id.as_str()],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| Error::validation(ValidationReason::NotFound, "id", Some(id.as_str())))
}

fn map_write(field: &'static str, id: &str, err: rusqlite::Error) -> Error {
    if is_busy(&err) {
        return Error::Busy;
    }
    if is_primary_key(&err) {
        return Error::validation(ValidationReason::DuplicateId, field, Some(id));
    }
    Error::Storage {
        message: format!("insert {field} {id}: {err}"),
    }
}

fn map_sql(context: &str, err: rusqlite::Error) -> Error {
    if is_busy(&err) {
        return Error::Busy;
    }
    Error::Storage {
        message: format!("{context}: {err}"),
    }
}

fn is_busy(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(ffi, _) => {
            ffi.code == rusqlite::ErrorCode::DatabaseBusy
                || ffi.code == rusqlite::ErrorCode::DatabaseLocked
        }
        _ => false,
    }
}

fn is_primary_key(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(ffi, _) => ffi.extended_code == SQLITE_CONSTRAINT_PRIMARYKEY,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::Store;
    use crate::{Error, UniverseId, ValidationReason};

    #[test]
    fn open_connection_enables_foreign_keys_and_delete_journal() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("pragma.sqlite");
        let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").expect("universe");
        let store = Store::create(&path, universe).expect("create");
        let enabled: i64 = store
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("foreign_keys");
        assert_eq!(enabled, 1);
        let mode: String = store
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal");
        assert_eq!(mode, "delete");
    }

    #[test]
    fn user_version_rolls_back_with_the_transaction() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("rollback.sqlite");
        let conn = rusqlite::Connection::open(&path).expect("open");
        conn.execute_batch("BEGIN IMMEDIATE").expect("begin");
        conn.execute_batch("PRAGMA user_version = 5")
            .expect("version");
        conn.execute_batch("ROLLBACK").expect("rollback");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("read version");
        assert_eq!(version, 0);
    }

    #[test]
    fn failed_initialization_keeps_the_reserved_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("reserved.sqlite");
        let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").expect("universe");
        let err = std::thread::Builder::new()
            .name("fail-after-reserve".to_string())
            .spawn(move || Store::create(&path, universe))
            .expect("spawn")
            .join()
            .expect("join")
            .expect_err("init");
        let message = err.to_string();
        assert!(
            message.contains("injected initialization failure"),
            "{message}"
        );
        assert!(message.contains("residual file"), "{message}");
        let reserved = dir.path().join("reserved.sqlite");
        assert!(reserved.is_file());
        let open_err = Store::open(&reserved).expect_err("residual open");
        assert!(
            matches!(
                open_err,
                Error::UnsupportedVersion { .. }
                    | Error::Validation {
                        reason: ValidationReason::NotADatabase
                            | ValidationReason::ForeignDatabase
                            | ValidationReason::MissingMetadata,
                        ..
                    }
            ),
            "{open_err}"
        );
        assert!(reserved.is_file());
    }
}
