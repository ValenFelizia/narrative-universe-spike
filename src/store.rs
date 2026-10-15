use std::io::Read;
use std::path::Path;
use std::time::Duration;

use rusqlite::{
    params, Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior,
};

use crate::error::{Error, Result, ValidationReason};
use crate::model::{
    value_fits, Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId, Entity,
    EntityId, EntityType, Owner, Page, PageId, Provenance, TypeId, UniverseId, Value, ValueClass,
};

const SCHEMA: &str = include_str!("schema.sql");
const HEADER_LEN: usize = 100;
const MAGIC: &[u8] = b"SQLite format 3\0";
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const SQLITE_CONSTRAINT_PRIMARYKEY: i32 = 1555;

const ASSERTION_COLUMNS: &str = "id, subject_id, definition_id, value_kind, text_value, bool_value, target_entity_id, canon, source_status_raw, provenance_external_ref, provenance_locator, provenance_fragment, provenance_source_revision, scope_note, archived";

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
        if path.exists() {
            return Err(Error::validation(
                ValidationReason::PathExists,
                "path",
                None,
            ));
        }
        let mut conn = match Connection::open(path) {
            Ok(conn) => conn,
            Err(err) => return Err(fail_create(path, map_sql("create", err))),
        };
        match init_new(&mut conn, &universe_id) {
            Ok(()) => Ok(Self { conn, universe_id }),
            Err(err) => {
                drop(conn);
                Err(fail_create(path, err))
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
            return Err(Error::UnsupportedVersion {
                found: header.user_version,
            });
        }
        let inspected = open_read_only(path)?;
        require_delete_journal(&inspected)?;
        require_tables(&inspected)?;
        let universe_id = read_universe(&inspected)?;
        drop(inspected);
        let conn = Connection::open(path).map_err(|err| map_sql("open", err))?;
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
    prepare_connection(conn)?;
    set_delete_journal(conn)?;
    let transaction = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| map_sql("begin", err))?;
    transaction
        .execute_batch(SCHEMA)
        .map_err(|err| map_sql("schema", err))?;
    transaction
        .execute(
            "INSERT INTO metadata (singleton, universe_id, format_version) VALUES (1, ?1, ?2)",
            params![universe_id.as_str(), crate::FORMAT_VERSION],
        )
        .map_err(|err| map_sql("metadata", err))?;
    // ROLLBACK undoes PRAGMA user_version on this bundled SQLite, so the pragma is committed with the schema.
    let sql = format!("PRAGMA user_version = {}", crate::SCHEMA_VERSION);
    transaction
        .execute_batch(&sql)
        .map_err(|err| map_sql("user_version", err))?;
    transaction.commit().map_err(|err| map_sql("commit", err))?;
    Ok(())
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
            "SELECT universe_id, format_version FROM metadata WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|err| map_sql("metadata", err))?;
    let Some((raw, version)) = row else {
        return Err(Error::validation(
            ValidationReason::MissingMetadata,
            "metadata",
            None,
        ));
    };
    if version != i64::from(crate::FORMAT_VERSION) {
        let Ok(found) = i32::try_from(version) else {
            return Err(Error::Storage {
                message: format!("format_version {version} does not fit a schema version"),
            });
        };
        return Err(Error::UnsupportedVersion { found });
    }
    UniverseId::parse(&raw).map_err(|_| {
        Error::validation(ValidationReason::ForeignDatabase, "universe_id", Some(&raw))
    })
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

fn fail_create(path: &Path, err: Error) -> Error {
    match std::fs::remove_file(path) {
        Ok(()) => err,
        Err(remove_err) if remove_err.kind() == std::io::ErrorKind::NotFound => err,
        Err(remove_err) => Error::Io {
            message: format!(
                "{err}; leftover file {} could not be removed: {remove_err}",
                path.display()
            ),
        },
    }
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
    use crate::UniverseId;

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
}
