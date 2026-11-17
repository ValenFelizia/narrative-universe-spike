use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use rusqlite::{
    params, Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior,
};

use crate::error::{Error, Result, ValidationReason};
use crate::mentions::{apply_mention_edit, validate_mentions, AnnotatedBody, Mention, MentionEdit};
use crate::model::{
    value_fits, Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId,
    DefinitionLabel, Entity, EntityId, EntityType, MentionId, Owner, Page, PageId, PageRevision,
    Provenance, TypeId, UniverseId, Value, ValueClass,
};

pub(crate) const SCHEMA: &str = include_str!("schema.sql");
const HEADER_LEN: usize = 100;
const MAGIC: &[u8] = b"SQLite format 3\0";
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const SQLITE_CONSTRAINT_PRIMARYKEY: i32 = 1555;

/// Maximum rendered groups (active outgoing, active incoming, archived outgoing,
/// and archived incoming) returned by [`Store::entity_context`].
pub const MAX_CONTEXT_GROUPS: usize = 128;

/// Maximum unique assertions touched by one [`Store::entity_context`] read.
pub const MAX_CONTEXT_UNIQUE_ASSERTIONS: usize = 500;

/// Maximum projected JSON wire payload bytes permitted for one
/// [`Store::entity_context`] read, enforced before copying oversized fields.
pub const MAX_CONTEXT_BYTES: usize = 2 * 1024 * 1024;

pub(crate) const ASSERTION_COLUMNS: &str = "id, subject_id, definition_id, value_kind, text_value, bool_value, target_entity_id, canon, source_status_raw, provenance_external_ref, provenance_locator, provenance_fragment, provenance_source_revision, scope_note, archived";

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

/// A page's identity and title for navigation, without loading its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageSummary {
    pub id: PageId,
    pub title: String,
}

/// One entity for a link picker: identity, label, and the labels of its
/// optional type and primary page. No bodies, values, or assertions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntitySummary {
    pub id: EntityId,
    pub label: String,
    pub type_label: Option<String>,
    pub primary_page_title: Option<String>,
}

/// A point-in-time, read-only projection. Claims keep their original orientation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityReveal {
    pub entity: Entity,
    pub type_label: Option<String>,
    pub outgoing: Vec<RevealClaim>,
    pub incoming: Vec<RevealClaim>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevealClaim {
    pub assertion: Assertion,
    pub definition: Definition,
    pub subject: Entity,
    pub target: Option<Entity>,
}

/// A point-in-time, bounded, grouped entity context projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityContext {
    pub entity: Entity,
    pub type_label: Option<String>,
    pub outgoing_groups: Vec<ContextOutgoingGroup>,
    pub incoming_groups: Vec<ContextIncomingGroup>,
    pub archived_outgoing_groups: Vec<ContextOutgoingGroup>,
    pub archived_incoming_groups: Vec<ContextIncomingGroup>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextOutgoingGroup {
    pub definition: Definition,
    pub claims: Vec<ContextClaim>,
    pub has_archived_claims: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextIncomingGroup {
    pub subject: Entity,
    pub definition: Definition,
    pub claims: Vec<ContextClaim>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextClaim {
    pub assertion: Assertion,
    pub target: Option<Entity>,
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
            if header.user_version == crate::LEGACY_V1_SCHEMA_VERSION {
                return Err(legacy_outcome(path)?);
            }
            if header.user_version == crate::LEGACY_V2_SCHEMA_VERSION {
                return Err(preserved_v2_outcome(path)?);
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
            // Only an out-of-band deletion with foreign keys disabled can leave
            // occurrences behind without their page. Reusing that id would bind
            // them to this body, so the page is refused instead.
            require_no_orphan_mentions(tx, &id)?;
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

    /// Lists page identities and titles in stable title-then-id order.
    /// Bodies are deliberately not selected for navigation.
    pub fn page_index(&self) -> Result<Vec<PageSummary>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, title FROM pages ORDER BY title COLLATE BINARY, id COLLATE BINARY")
            .map_err(|err| map_sql("prepare page index", err))?;
        let mut rows = statement
            .query([])
            .map_err(|err| map_sql("read page index", err))?;
        let mut pages = Vec::new();
        while let Some(row) = rows.next().map_err(|err| map_sql("read page index", err))? {
            let raw_id: String = row.get(0).map_err(|err| map_sql("read page id", err))?;
            let title: String = row.get(1).map_err(|err| map_sql("read page title", err))?;
            let id = parse_stored_id("page", &raw_id, PageId::parse)?;
            pages.push(PageSummary { id, title });
        }
        Ok(pages)
    }

    /// Lists every entity in stable label-then-id order (SQLite BINARY, so
    /// equal labels stay separate and ordered by id). Read only.
    pub fn entity_index(&self) -> Result<Vec<EntitySummary>> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT e.id, e.label, t.label, p.title
                 FROM entities AS e
                 LEFT JOIN types AS t ON t.id = e.type_id
                 LEFT JOIN pages AS p ON p.id = e.page_id
                 ORDER BY e.label COLLATE BINARY, e.id COLLATE BINARY",
            )
            .map_err(|err| map_sql("prepare entity index", err))?;
        let mut rows = statement
            .query([])
            .map_err(|err| map_sql("read entity index", err))?;
        let mut entities = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|err| map_sql("read entity index", err))?
        {
            let raw_id: String = row.get(0).map_err(|err| map_sql("read entity id", err))?;
            let label: String = row
                .get(1)
                .map_err(|err| map_sql("read entity label", err))?;
            let type_label: Option<String> =
                row.get(2).map_err(|err| map_sql("read type label", err))?;
            let primary_page_title: Option<String> =
                row.get(3).map_err(|err| map_sql("read page title", err))?;
            entities.push(EntitySummary {
                id: parse_stored_id("entity", &raw_id, EntityId::parse)?,
                label,
                type_label,
                primary_page_title,
            });
        }
        Ok(entities)
    }

    /// Reads identity and all active claims from a single SQLite snapshot.
    /// Missing identities fail closed, including dangling stored references.
    pub fn entity_reveal(&self, id: &EntityId) -> Result<EntityReveal> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|err| map_sql("begin reveal", err))?;
        let entity = required_entity(&tx, id)?;
        let type_label = entity
            .type_id
            .as_ref()
            .map(|id| required_entity_type(&tx, id).map(|t| t.label))
            .transpose()?;
        let claims = list_assertions(&tx, &format!(
            "SELECT {ASSERTION_COLUMNS} FROM assertions WHERE subject_id = ?1 OR target_entity_id = ?1 ORDER BY definition_id, id"
        ), params![id.as_str()])?;
        let mut outgoing = Vec::new();
        let mut incoming = Vec::new();
        for assertion in claims {
            let definition = required_definition(&tx, &assertion.definition_id)?;
            if assertion.archived || definition.archived {
                continue;
            }
            let subject = required_entity(&tx, &assertion.subject_id)?;
            let target = match &assertion.value {
                Value::Reference(id) => Some(required_entity(&tx, id)?),
                _ => None,
            };
            require_definition_owner(&tx, &definition)?;
            validate_assertion(&tx, &assertion)?;
            let is_outgoing = assertion.subject_id == *id;
            let is_incoming = matches!(&assertion.value, Value::Reference(target) if target == id);
            let claim = RevealClaim {
                assertion,
                definition,
                subject,
                target,
            };
            if is_outgoing {
                outgoing.push(claim.clone());
            }
            if is_incoming {
                incoming.push(claim);
            }
        }
        tx.commit().map_err(|err| map_sql("commit reveal", err))?;
        Ok(EntityReveal {
            entity,
            type_label,
            outgoing,
            incoming,
        })
    }

    /// Reads identity, applicable active/archived definitions, grouped active
    /// outgoing/incoming claims, and archived history from a single SQLite
    /// snapshot, bounded before copying oversized payloads.
    pub fn entity_context(&self, id: &EntityId) -> Result<EntityContext> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|err| map_sql("begin context", err))?;
        let context = read_entity_context(&tx, id)?;
        tx.commit().map_err(|err| map_sql("commit context", err))?;
        Ok(context)
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
                            // An occurrence id belongs to one page, so a link
                            // may never claim an id that another page owns,
                            // not even for a moment inside one batch.
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
            if changed {
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
                    id, owner_type_id, owner_entity_id, value_class, cardinality, archived, label
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    definition.id.as_str(),
                    owner_type,
                    owner_entity,
                    definition.value_class.as_str(),
                    definition.cardinality.as_str(),
                    i64::from(definition.archived),
                    definition.label.as_ref().map(DefinitionLabel::as_str),
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
        let mut snapshot = read_snapshot(&transaction)?;
        // Keep the handle's identity authoritative for an export/import pair.
        // A concurrent writer that changes the metadata row must not make an
        // otherwise valid export fail its own import with WrongUniverse.
        snapshot.universe_id = self.universe_id.clone();
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
        Self::import_new_with_mode(path, snapshot, false)
    }

    /// Creates a private current-version candidate and closes it before returning.
    ///
    /// On Unix the file is created with mode `0600`, so its contents are never
    /// group- or world-readable while the migration is filling it. On Windows
    /// `create_new` inherits the destination directory's ACL; the alpha does
    /// not attempt to tighten it with platform-specific APIs.
    pub(crate) fn import_new_private(path: &Path, snapshot: Snapshot) -> Result<ImportOutcome> {
        let (store, outcome) = Self::import_new_with_mode(path, snapshot, true)?;
        drop(store);
        Ok(outcome)
    }

    fn import_new_with_mode(
        path: &Path,
        snapshot: Snapshot,
        private: bool,
    ) -> Result<(Self, ImportOutcome)> {
        ensure_unique_snapshot(&snapshot)?;
        let snapshot = normalize_snapshot(snapshot);
        // The geometry of an occurrence is local to the document, so it is
        // checked before the destination is reserved: a malformed document
        // must not leave a residual file behind.
        validate_snapshot_mentions(&snapshot)?;
        reserve_file(path, private)?;
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
        #[cfg(test)]
        run_test_hook("before_write_commit");
        transaction.commit().map_err(|err| map_sql("commit", err))?;
        Ok(value)
    }
}

#[cfg(test)]
type TestHook = std::sync::Arc<dyn Fn(&'static str) + Send + Sync>;

#[cfg(test)]
std::thread_local! {
    static TEST_HOOK: std::cell::RefCell<Option<TestHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn run_test_hook(point: &'static str) {
    let hook = TEST_HOOK.with(|cell| cell.borrow().clone());
    if let Some(hook) = hook {
        hook(point);
    }
}

/// Reads one canonical logical snapshot from a connection.
///
/// The caller owns the connection and its transaction. A read-only caller can
/// use the same typed readers, so a migration verifier cannot accidentally see
/// records that the ordinary snapshot path would reject.
pub(crate) fn read_snapshot(conn: &Connection) -> Result<Snapshot> {
    Ok(Snapshot {
        universe_id: read_universe(conn)?,
        pages: list_page_snapshots(conn)?,
        types: list_types(conn)?,
        entities: list_entities(conn)?,
        definitions: list_definitions(conn)?,
        assertions: list_assertions(
            conn,
            &format!("SELECT {ASSERTION_COLUMNS} FROM assertions ORDER BY id"),
            params![],
        )?,
    })
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
    reserve_file(path, false)
}

fn reserve_file(path: &Path, private: bool) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    match options.open(path) {
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
        // Never echo the stored value: it is untrusted database content.
        Error::validation(ValidationReason::ForeignDatabase, "universe_id", None)
    })
}

/// Lists the non-internal table names of a connection.
fn table_names(conn: &Connection) -> Result<Vec<String>> {
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
    Ok(tables)
}

fn has_table(tables: &[String], name: &str) -> bool {
    tables.iter().any(|table| table == name)
}

/// Tells whether a table carries a column with this name.
///
/// The table name is a compile-time constant at every call site; `PRAGMA
/// table_info` cannot take a bound parameter, so it is interpolated.
fn has_column(conn: &Connection, table: &'static str, column: &str) -> Result<bool> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|err| map_sql("schema", err))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| map_sql("schema", err))?;
    for row in rows {
        if row.map_err(|err| map_sql("schema", err))? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Tells a historical version 1 database apart from a foreign file.
///
/// A version 1 database keeps its format version inside the metadata table.
/// A file that only claims the version number is not one of ours.
fn is_legacy_v1(conn: &Connection) -> Result<bool> {
    let tables = table_names(conn)?;
    for name in [
        "assertions",
        "definitions",
        "entities",
        "metadata",
        "pages",
        "types",
    ] {
        if !has_table(&tables, name) {
            return Ok(false);
        }
    }
    has_column(conn, "metadata", "format_version")
}

/// Decides what a historical database needs, without touching it.
///
/// The file is only inspected through a read-only connection, so a version 1
/// database is never modified, upgraded or copied.
fn legacy_outcome(path: &Path) -> Result<Error> {
    let inspected = open_read_only(path)?;
    if is_legacy_v1(&inspected)? {
        return Ok(Error::MigrationRequired {
            found: crate::LEGACY_V1_SCHEMA_VERSION,
        });
    }
    Ok(Error::validation(
        ValidationReason::ForeignDatabase,
        "schema",
        None,
    ))
}

/// Refuses a preserved version 2 database without opening it for writing.
///
/// Version 2 has no in-place conversion in this build (DEC-002). The file is
/// inspected through a read-only connection only, so this is never an upgrade.
/// A file that merely claims the version number is reported as foreign, which
/// keeps a genuine version 2 universe distinguishable from an unrelated file.
fn preserved_v2_outcome(path: &Path) -> Result<Error> {
    let inspected = open_read_only(path)?;
    if crate::migration::is_preserved_v2_schema(&inspected)? {
        return Ok(Error::UnsupportedDatabase {
            found: crate::LEGACY_V2_SCHEMA_VERSION,
            supported: crate::SCHEMA_VERSION,
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

pub(crate) fn ensure_unique_snapshot(snapshot: &Snapshot) -> Result<()> {
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

/// Checks the geometry of every occurrence against its own body.
///
/// This is a document-local shape check: it needs no database, so an import
/// into a new path can run it before the destination exists.
fn validate_snapshot_mentions(snapshot: &Snapshot) -> Result<()> {
    for page_snapshot in &snapshot.pages {
        validate_mentions(&AnnotatedBody {
            body: page_snapshot.page.body.clone(),
            mentions: page_snapshot.mentions.clone(),
        })?;
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

/// Refuses to create a page while stored occurrences still point at its id.
///
/// A row can only be orphaned by a deletion performed outside SQLite with
/// foreign keys disabled. Creating the same id again would make those rows
/// valid again and bind them to different prose, so the page is refused. The
/// rows are never deleted, relinked or adopted, and the enclosing transaction
/// leaves the corrupt state untouched.
fn require_no_orphan_mentions(tx: &Transaction, id: &PageId) -> Result<()> {
    let stored: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM mentions WHERE page_id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )
        .map_err(|err| map_sql("mentions", err))?;
    if stored > 0 {
        return Err(Error::Storage {
            message: format!(
                "{stored} stored occurrences still point at missing page {}",
                id.as_str()
            ),
        });
    }
    Ok(())
}

fn insert_page(tx: &Transaction, page: &Page) -> Result<()> {
    require_no_orphan_mentions(tx, &page.id)?;
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
            id, owner_type_id, owner_entity_id, value_class, cardinality, archived, label
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            definition.id.as_str(),
            owner_type,
            owner_entity,
            definition.value_class.as_str(),
            definition.cardinality.as_str(),
            i64::from(definition.archived),
            definition.label.as_ref().map(DefinitionLabel::as_str),
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

/// Lists the stored pages, failing closed when a stored revision is not a
/// checked nonnegative integer.
///
/// The export reads the pages through here, so a row that `page_document`
/// already refuses cannot leave the database as part of a healthy universe.
fn list_pages(conn: &Connection) -> Result<Vec<Page>> {
    let mut statement = conn
        .prepare("SELECT id, title, body, revision FROM pages ORDER BY id")
        .map_err(|err| map_sql("prepare", err))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|err| map_sql("query", err))?;
    let mut pages = Vec::new();
    for row in rows {
        let (id, title, body, raw_revision) = row.map_err(|err| map_sql("row", err))?;
        let id = parse_stored_id("page", &id, PageId::parse)?;
        PageRevision::new(raw_revision).map_err(|_| Error::Storage {
            message: format!(
                "invalid stored revision {raw_revision} on page {}",
                id.as_str()
            ),
        })?;
        pages.push(Page { id, title, body });
    }
    Ok(pages)
}

/// Reads every page with its complete canonical mention set.
fn list_page_snapshots(conn: &Connection) -> Result<Vec<PageSnapshot>> {
    let mut snapshots = Vec::new();
    let mut counted: u64 = 0;
    for page in list_pages(conn)? {
        let mentions = read_page_annotated(conn, &page.id, &page.body)?.mentions;
        counted += mentions.len() as u64;
        snapshots.push(PageSnapshot { page, mentions });
    }
    // An occurrence that belongs to no stored page would be dropped by an
    // export, so the export refuses the universe instead.
    let stored = count(conn, "SELECT COUNT(*) FROM mentions")?;
    if stored != counted {
        return Err(Error::Storage {
            message: format!("{stored} stored occurrences but {counted} belong to a stored page"),
        });
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
            "SELECT id, owner_type_id, owner_entity_id, value_class, cardinality, archived, label
             FROM definitions ORDER BY id",
        )
        .map_err(|err| map_sql("prepare", err))?;
    let rows = statement
        .query_map([], read_definition_parts)
        .map_err(|err| map_sql("query", err))?;
    let mut definitions = Vec::new();
    for row in rows {
        definitions.push(definition_from_parts(
            row.map_err(|err| map_sql("row", err))?,
        )?);
    }
    Ok(definitions)
}

/// The stored columns of one definition, in selection order.
type DefinitionParts = (
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    i64,
    Option<String>,
);

fn read_definition_parts(row: &rusqlite::Row<'_>) -> rusqlite::Result<DefinitionParts> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

/// Builds a typed definition, enforcing the label rule on stored content.
///
/// The id is validated before anything else so a malformed row can never be
/// formatted into a diagnostic. The label is parsed through
/// [`DefinitionLabel::parse`], the same owner the JSON decoder and
/// `create_definition` use, and a rejected label is never echoed: it is
/// untrusted database content.
fn definition_from_parts(parts: DefinitionParts) -> Result<Definition> {
    let (raw_id, owner_type, owner_entity, value_class, cardinality, archived, label) = parts;
    let id = parse_stored_id("definition", &raw_id, DefinitionId::parse)?;
    let owner = match (owner_type, owner_entity) {
        (Some(raw), None) => Owner::Type(parse_stored_id("owner type", &raw, TypeId::parse)?),
        (None, Some(raw)) => Owner::Entity(parse_stored_id("owner entity", &raw, EntityId::parse)?),
        _ => {
            return Err(Error::Storage {
                message: format!("invalid definition owner {}", id.as_str()),
            });
        }
    };
    let label = match label {
        Some(raw) => Some(DefinitionLabel::parse(&raw).map_err(|_| Error::Storage {
            message: format!("invalid stored definition label on {}", id.as_str()),
        })?),
        None => None,
    };
    Ok(Definition {
        id,
        owner,
        value_class: ValueClass::parse(&value_class)?,
        cardinality: Cardinality::parse(&cardinality)?,
        label,
        archived: read_flag(archived, "archived")?,
    })
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

pub(crate) struct AssertionParts {
    pub(crate) id: String,
    pub(crate) subject_id: String,
    pub(crate) definition_id: String,
    pub(crate) value_kind: String,
    pub(crate) text_value: Option<String>,
    pub(crate) bool_value: Option<i64>,
    pub(crate) target_entity_id: Option<String>,
    pub(crate) canon: String,
    pub(crate) source_status_raw: Option<String>,
    pub(crate) provenance_external_ref: Option<String>,
    pub(crate) provenance_locator: Option<String>,
    pub(crate) provenance_fragment: Option<String>,
    pub(crate) provenance_source_revision: Option<String>,
    pub(crate) scope_note: Option<String>,
    pub(crate) archived: i64,
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

pub(crate) fn assertion_from_parts(parts: AssertionParts) -> Result<Assertion> {
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
        _ => {
            return Err(Error::Storage {
                message: format!("invalid value kind on {id}"),
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
        message: format!("invalid stored {field} id"),
    })
}

pub(crate) fn read_flag(value: i64, field: &str) -> Result<bool> {
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

/// Reads a stored page row and validates its revision.
///
/// The revision is checked here so every caller that reconciles or reads a
/// page fails closed on a value the aggregate read already refuses, instead
/// of committing around it.
fn lookup_page(conn: &Connection, id: &PageId) -> Result<Option<Page>> {
    let row = conn
        .query_row(
            "SELECT id, title, body, revision FROM pages WHERE id = ?1",
            [id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|err| map_sql("page", err))?;
    let Some((raw_id, title, body, raw_revision)) = row else {
        return Ok(None);
    };
    let id = parse_stored_id("page", &raw_id, PageId::parse)?;
    PageRevision::new(raw_revision).map_err(|_| Error::Storage {
        message: format!(
            "invalid stored revision {raw_revision} on page {}",
            id.as_str()
        ),
    })?;
    Ok(Some(Page { id, title, body }))
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
            message: "invalid stored mention id".to_string(),
        })?;
        let entity_id = EntityId::parse(&raw_entity).map_err(|_| Error::Storage {
            message: "invalid stored mention entity id".to_string(),
        })?;
        if lookup_entity(conn, &entity_id)?.is_none() {
            return Err(Error::Storage {
                message: format!("stored mention {} points at a missing entity", id.as_str()),
            });
        }
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
        message: "invalid stored mention page id".to_string(),
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
            message: "invalid stored mention page id".to_string(),
        })?;
        let id = MentionId::parse(&raw_id).map_err(|_| Error::Storage {
            message: "invalid stored mention id".to_string(),
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
        "SELECT id, owner_type_id, owner_entity_id, value_class, cardinality, archived, label
         FROM definitions WHERE id = ?1",
        [id.as_str()],
        read_definition_parts,
    )
    .optional()
    .map_err(|err| map_sql("definition", err))?
    .map(definition_from_parts)
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

// Compact camelCase JSON wire sizes matching `ui/src-tauri/src/lib.rs` DTOs.
// `RevealIdentity`: `{"id":"<36>","label":...}` = 54 + label.
const CONTEXT_ROOT_BASE_BYTES: usize = 174; // {"entity":{"id":"<36>","label":},"typeLabel":,"outgoingGroups":[],"incomingGroups":[],"archivedOutgoingGroups":[],"archivedIncomingGroups":[]}
const CONTEXT_DEFINITION_BASE_BYTES: usize = 165; // {"id":"<36>","ownerKind":,"ownerId":"<36>","propertyLabel":,"valueClass":,"cardinality":,"archived":}
const CONTEXT_OUTGOING_GROUP_WRAPPER_BYTES: usize = 53; // {"definition":,"claims":[],"hasArchivedClaims":false} (true saves 1B)
const CONTEXT_INCOMING_GROUP_WRAPPER_BYTES: usize = 92; // {"subject":{"id":"<36>","label":},"definition":,"claims":[]}
const CONTEXT_CLAIM_BASE_BYTES: usize = 180; // {"id":"<36>","value":,"canon":"","sourceStatusRaw":,"provenance":{"externalRef":,"locator":,"fragment":,"sourceRevision":},"scopeNote":,"archived":}
const CONTEXT_TEXT_VALUE_OVERHEAD: usize = 23; // {"kind":"text","text":}
const CONTEXT_BOOL_VALUE_BASE_BYTES: usize = 27; // {"kind":"boolean","value":} + 4 (true) or 5 (false)
const CONTEXT_UNKNOWN_VALUE_BYTES: usize = 18; // {"kind":"unknown"}
const CONTEXT_REF_VALUE_OVERHEAD: usize = 84; // {"kind":"reference","target":{"id":"<36>","label":}}
const JSON_NULL_BYTES: usize = 4;

struct ContextBudget<'a> {
    context_id: &'a EntityId,
    groups: usize,
    unique_assertions: BTreeSet<String>,
    bytes: usize,
}

impl<'a> ContextBudget<'a> {
    fn new(context_id: &'a EntityId) -> Self {
        Self {
            context_id,
            groups: 0,
            unique_assertions: BTreeSet::new(),
            bytes: 0,
        }
    }

    fn too_large(&self) -> Error {
        Error::validation(
            ValidationReason::ContextTooLarge,
            "context",
            Some(self.context_id.as_str()),
        )
    }

    fn remaining(&self) -> usize {
        MAX_CONTEXT_BYTES.saturating_sub(self.bytes)
    }

    fn charge(&mut self, amount: usize) -> Result<()> {
        let next = self
            .bytes
            .checked_add(amount)
            .ok_or_else(|| self.too_large())?;
        if next > MAX_CONTEXT_BYTES {
            return Err(self.too_large());
        }
        self.bytes = next;
        Ok(())
    }

    fn add_group(&mut self) -> Result<()> {
        let next = self.groups.checked_add(1).ok_or_else(|| self.too_large())?;
        if next > MAX_CONTEXT_GROUPS {
            return Err(self.too_large());
        }
        self.groups = next;
        Ok(())
    }

    fn add_unique_assertion(&mut self, id: &AssertionId) -> Result<()> {
        if self.unique_assertions.contains(id.as_str()) {
            return Ok(());
        }
        if self.unique_assertions.len() >= MAX_CONTEXT_UNIQUE_ASSERTIONS {
            return Err(self.too_large());
        }
        self.unique_assertions.insert(id.as_str().to_owned());
        Ok(())
    }
}

fn json_str_bytes(bytes: &[u8]) -> usize {
    let mut total = 2usize;
    for &b in bytes {
        total += match b {
            b'"' | b'\\' | b'\x08' | b'\x09' | b'\x0a' | b'\x0c' | b'\x0d' => 2,
            0x00..=0x1f => 6,
            _ => 1,
        };
    }
    total
}

fn json_bool_bytes(flag: bool) -> usize {
    if flag {
        4
    } else {
        5
    }
}

fn definition_json_bytes(definition: &Definition) -> usize {
    let owner_kind_bytes = match definition.owner {
        Owner::Type(_) => 6,   // "type"
        Owner::Entity(_) => 8, // "entity"
    };
    let label_bytes = match definition.label.as_ref() {
        Some(label) => json_str_bytes(label.as_str().as_bytes()),
        None => JSON_NULL_BYTES,
    };
    CONTEXT_DEFINITION_BASE_BYTES
        + owner_kind_bytes
        + label_bytes
        + 2
        + definition.value_class.as_str().len()
        + 2
        + definition.cardinality.as_str().len()
        + json_bool_bytes(definition.archived)
}

fn outgoing_group_json_bytes(definition: &Definition, has_prior_groups: bool) -> usize {
    CONTEXT_OUTGOING_GROUP_WRAPPER_BYTES
        + definition_json_bytes(definition)
        + usize::from(has_prior_groups)
}

fn incoming_group_json_bytes(
    subject: &Entity,
    definition: &Definition,
    has_prior_groups: bool,
) -> usize {
    CONTEXT_INCOMING_GROUP_WRAPPER_BYTES
        + json_str_bytes(subject.label.as_bytes())
        + definition_json_bytes(definition)
        + usize::from(has_prior_groups)
}

fn read_bounded_id<T>(
    row: &rusqlite::Row<'_>,
    idx: usize,
    field: &str,
    parse: impl FnOnce(&str) -> Result<T>,
) -> Result<T> {
    match row.get_ref(idx).map_err(|err| map_sql(field, err))? {
        rusqlite::types::ValueRef::Text(bytes) => {
            if bytes.len() != 36 {
                return Err(Error::Storage {
                    message: format!("invalid stored {field} id"),
                });
            }
            let raw = std::str::from_utf8(bytes).map_err(|_| Error::Storage {
                message: format!("invalid stored {field} id"),
            })?;
            parse_stored_id(field, raw, parse)
        }
        _ => Err(Error::Storage {
            message: format!("invalid stored {field} id"),
        }),
    }
}

fn read_bounded_opt_id<T>(
    row: &rusqlite::Row<'_>,
    idx: usize,
    field: &str,
    parse: impl FnOnce(&str) -> Result<T>,
) -> Result<Option<T>> {
    match row.get_ref(idx).map_err(|err| map_sql(field, err))? {
        rusqlite::types::ValueRef::Null => Ok(None),
        rusqlite::types::ValueRef::Text(bytes) => {
            if bytes.len() != 36 {
                return Err(Error::Storage {
                    message: format!("invalid stored {field} id"),
                });
            }
            let raw = std::str::from_utf8(bytes).map_err(|_| Error::Storage {
                message: format!("invalid stored {field} id"),
            })?;
            Ok(Some(parse_stored_id(field, raw, parse)?))
        }
        _ => Err(Error::Storage {
            message: format!("invalid stored {field} id"),
        }),
    }
}

fn read_bounded_short_str<'a>(
    row: &'a rusqlite::Row<'_>,
    idx: usize,
    field: &str,
    max_bytes: usize,
) -> Result<&'a str> {
    match row.get_ref(idx).map_err(|err| map_sql(field, err))? {
        rusqlite::types::ValueRef::Text(bytes) => {
            if bytes.len() > max_bytes {
                return Err(Error::Storage {
                    message: format!("invalid stored {field}"),
                });
            }
            std::str::from_utf8(bytes).map_err(|_| Error::Storage {
                message: format!("invalid stored {field}"),
            })
        }
        _ => Err(Error::Storage {
            message: format!("invalid stored {field}"),
        }),
    }
}

fn read_bounded_flag(row: &rusqlite::Row<'_>, idx: usize, field: &str) -> Result<bool> {
    match row.get_ref(idx).map_err(|err| map_sql(field, err))? {
        rusqlite::types::ValueRef::Integer(v) => read_flag(v, field),
        _ => Err(Error::Storage {
            message: format!("invalid {field}"),
        }),
    }
}

fn read_bounded_text(
    row: &rusqlite::Row<'_>,
    idx: usize,
    field: &str,
    budget: &mut ContextBudget<'_>,
) -> Result<String> {
    match row.get_ref(idx).map_err(|err| map_sql(field, err))? {
        rusqlite::types::ValueRef::Text(bytes) => {
            let max_json_bytes = budget.remaining();
            if bytes.len() > max_json_bytes {
                return Err(budget.too_large());
            }
            let encoded = json_str_bytes(bytes);
            if encoded > max_json_bytes {
                return Err(budget.too_large());
            }
            let text = std::str::from_utf8(bytes).map_err(|_| Error::Storage {
                message: format!("invalid utf8 in {field}"),
            })?;
            budget.charge(encoded)?;
            Ok(text.to_owned())
        }
        _ => Err(Error::Storage {
            message: format!("invalid column type for {field}"),
        }),
    }
}

fn read_bounded_opt_text(
    row: &rusqlite::Row<'_>,
    idx: usize,
    field: &str,
    budget: &mut ContextBudget<'_>,
) -> Result<Option<String>> {
    match row.get_ref(idx).map_err(|err| map_sql(field, err))? {
        rusqlite::types::ValueRef::Null => {
            budget.charge(JSON_NULL_BYTES)?;
            Ok(None)
        }
        rusqlite::types::ValueRef::Text(bytes) => {
            let max_json_bytes = budget.remaining();
            if bytes.len() > max_json_bytes {
                return Err(budget.too_large());
            }
            let encoded = json_str_bytes(bytes);
            if encoded > max_json_bytes {
                return Err(budget.too_large());
            }
            let text = std::str::from_utf8(bytes).map_err(|_| Error::Storage {
                message: format!("invalid utf8 in {field}"),
            })?;
            budget.charge(encoded)?;
            Ok(Some(text.to_owned()))
        }
        _ => Err(Error::Storage {
            message: format!("invalid column type for {field}"),
        }),
    }
}

fn read_bounded_entity_label(
    row: &rusqlite::Row<'_>,
    idx: usize,
    budget: &ContextBudget<'_>,
) -> Result<String> {
    match row.get_ref(idx).map_err(|err| map_sql("entity", err))? {
        rusqlite::types::ValueRef::Text(bytes) => {
            let max_json_bytes = budget.remaining();
            if bytes.len() > max_json_bytes || json_str_bytes(bytes) > max_json_bytes {
                return Err(budget.too_large());
            }
            let text = std::str::from_utf8(bytes).map_err(|_| Error::Storage {
                message: "invalid utf8 in entity label".to_string(),
            })?;
            Ok(text.to_owned())
        }
        _ => Err(Error::Storage {
            message: "invalid column type for entity label".to_string(),
        }),
    }
}

fn read_bounded_entity(
    conn: &Connection,
    id: &EntityId,
    budget: &ContextBudget<'_>,
) -> Result<Option<Entity>> {
    let mut stmt = conn
        .prepare_cached("SELECT id, label, type_id, page_id FROM entities WHERE id = ?1")
        .map_err(|err| map_sql("entity", err))?;
    let mut rows = stmt
        .query([id.as_str()])
        .map_err(|err| map_sql("entity", err))?;
    let Some(row) = rows.next().map_err(|err| map_sql("entity", err))? else {
        return Ok(None);
    };
    let parsed_id = read_bounded_id(row, 0, "entity", EntityId::parse)?;
    let type_id = read_bounded_opt_id(row, 2, "entity type", TypeId::parse)?;
    let page_id = read_bounded_opt_id(row, 3, "entity page", PageId::parse)?;
    let label = read_bounded_entity_label(row, 1, budget)?;
    Ok(Some(Entity {
        id: parsed_id,
        label,
        type_id,
        page_id,
    }))
}

fn lookup_cached_entity<'a>(
    conn: &Connection,
    id: &EntityId,
    cache: &'a mut BTreeMap<String, Entity>,
    budget: &ContextBudget<'_>,
    missing_reason: ValidationReason,
    missing_field: &'static str,
) -> Result<&'a Entity> {
    let key = id.as_str();
    if !cache.contains_key(key) {
        let entity = read_bounded_entity(conn, id, budget)?
            .ok_or_else(|| Error::validation(missing_reason, missing_field, Some(id.as_str())))?;
        cache.insert(key.to_owned(), entity);
    }
    Ok(cache.get(key).expect("entity was just inserted into cache"))
}

fn read_bounded_type_label(
    conn: &Connection,
    id: &TypeId,
    budget: &mut ContextBudget<'_>,
) -> Result<String> {
    let mut stmt = conn
        .prepare_cached("SELECT id, label FROM types WHERE id = ?1")
        .map_err(|err| map_sql("type", err))?;
    let mut rows = stmt
        .query([id.as_str()])
        .map_err(|err| map_sql("type", err))?;
    let Some(row) = rows.next().map_err(|err| map_sql("type", err))? else {
        return Err(Error::validation(
            ValidationReason::NotFound,
            "id",
            Some(id.as_str()),
        ));
    };
    let _parsed_id = read_bounded_id(row, 0, "type", TypeId::parse)?;
    read_bounded_text(row, 1, "type label", budget)
}

fn read_definition_from_row(row: &rusqlite::Row<'_>) -> Result<Definition> {
    let id = read_bounded_id(row, 0, "definition", DefinitionId::parse)?;
    let owner_type = read_bounded_opt_id(row, 1, "owner type", TypeId::parse)?;
    let owner_entity = read_bounded_opt_id(row, 2, "owner entity", EntityId::parse)?;
    let owner = match (owner_type, owner_entity) {
        (Some(tid), None) => Owner::Type(tid),
        (None, Some(eid)) => Owner::Entity(eid),
        _ => {
            return Err(Error::Storage {
                message: format!("invalid definition owner {}", id.as_str()),
            });
        }
    };
    let value_class = ValueClass::parse(read_bounded_short_str(row, 3, "value_class", 16)?)?;
    let cardinality = Cardinality::parse(read_bounded_short_str(row, 4, "cardinality", 16)?)?;
    let archived = read_bounded_flag(row, 5, "archived")?;
    let label = match row.get_ref(6).map_err(|err| map_sql("definition", err))? {
        rusqlite::types::ValueRef::Null => None,
        rusqlite::types::ValueRef::Text(bytes) => {
            // Maximum 120 Unicode scalar values can occupy at most 480 UTF-8 bytes.
            if bytes.len() > 480 {
                return Err(Error::Storage {
                    message: format!("invalid stored definition label on {}", id.as_str()),
                });
            }
            let raw = std::str::from_utf8(bytes).map_err(|_| Error::Storage {
                message: format!("invalid stored definition label on {}", id.as_str()),
            })?;
            Some(DefinitionLabel::parse(raw).map_err(|_| Error::Storage {
                message: format!("invalid stored definition label on {}", id.as_str()),
            })?)
        }
        _ => {
            return Err(Error::Storage {
                message: format!("invalid stored definition label on {}", id.as_str()),
            });
        }
    };
    Ok(Definition {
        id,
        owner,
        value_class,
        cardinality,
        label,
        archived,
    })
}

fn require_bounded_definition_owner(
    conn: &Connection,
    definition: &Definition,
    checked_owner_types: &mut BTreeSet<String>,
    checked_entities: &BTreeMap<String, Entity>,
    checked_owner_entities: &mut BTreeSet<String>,
) -> Result<()> {
    match &definition.owner {
        Owner::Type(type_id) => {
            let key = type_id.as_str();
            if checked_owner_types.contains(key) {
                return Ok(());
            }
            let mut stmt = conn
                .prepare_cached("SELECT id, typeof(label) FROM types WHERE id = ?1")
                .map_err(|err| map_sql("type", err))?;
            let mut rows = stmt.query([key]).map_err(|err| map_sql("type", err))?;
            let Some(row) = rows.next().map_err(|err| map_sql("type", err))? else {
                return Err(Error::validation(
                    ValidationReason::UnknownType,
                    "owner_type_id",
                    Some(key),
                ));
            };
            let _ = read_bounded_id(row, 0, "type", TypeId::parse)?;
            if read_bounded_short_str(row, 1, "type label", 16)? != "text" {
                return Err(Error::Storage {
                    message: "invalid column type for type label".to_string(),
                });
            }
            checked_owner_types.insert(key.to_owned());
        }
        Owner::Entity(entity_id) => {
            let key = entity_id.as_str();
            if checked_entities.contains_key(key) || checked_owner_entities.contains(key) {
                return Ok(());
            }
            let mut stmt = conn
                .prepare_cached(
                    "SELECT id, typeof(label), type_id, page_id FROM entities WHERE id = ?1",
                )
                .map_err(|err| map_sql("entity", err))?;
            let mut rows = stmt.query([key]).map_err(|err| map_sql("entity", err))?;
            let Some(row) = rows.next().map_err(|err| map_sql("entity", err))? else {
                return Err(Error::validation(
                    ValidationReason::UnknownEntity,
                    "owner_entity_id",
                    Some(key),
                ));
            };
            let _ = read_bounded_id(row, 0, "entity", EntityId::parse)?;
            if read_bounded_short_str(row, 1, "entity label", 16)? != "text" {
                return Err(Error::Storage {
                    message: "invalid column type for entity label".to_string(),
                });
            }
            let _ = read_bounded_opt_id(row, 2, "entity type", TypeId::parse)?;
            let _ = read_bounded_opt_id(row, 3, "entity page", PageId::parse)?;
            checked_owner_entities.insert(key.to_owned());
        }
    }
    Ok(())
}

fn lookup_cached_definition<'a>(
    tx: &Transaction,
    id: &DefinitionId,
    cache: &'a mut BTreeMap<String, Definition>,
    checked_owner_types: &mut BTreeSet<String>,
    checked_entities: &BTreeMap<String, Entity>,
    checked_owner_entities: &mut BTreeSet<String>,
) -> Result<&'a Definition> {
    let key = id.as_str();
    if !cache.contains_key(key) {
        let mut stmt = tx
            .prepare_cached(
                "SELECT id, owner_type_id, owner_entity_id, value_class, cardinality, archived, label
                 FROM definitions WHERE id = ?1",
            )
            .map_err(|err| map_sql("definition", err))?;
        let mut rows = stmt
            .query([id.as_str()])
            .map_err(|err| map_sql("definition", err))?;
        let Some(row) = rows.next().map_err(|err| map_sql("definition", err))? else {
            return Err(Error::validation(
                ValidationReason::UnknownDefinition,
                "definition_id",
                Some(id.as_str()),
            ));
        };
        let definition = read_definition_from_row(row)?;
        require_bounded_definition_owner(
            tx,
            &definition,
            checked_owner_types,
            checked_entities,
            checked_owner_entities,
        )?;
        cache.insert(key.to_owned(), definition);
    }
    Ok(cache
        .get(key)
        .expect("definition was just inserted into cache"))
}

enum AssertionValueHeader {
    Text,
    Boolean(bool),
    Reference(EntityId),
    Unknown,
}

struct AssertionHeader {
    id: AssertionId,
    subject_id: EntityId,
    definition_id: DefinitionId,
    value: AssertionValueHeader,
    canon: Canon,
    archived: bool,
}

fn read_assertion_header(row: &rusqlite::Row<'_>) -> Result<AssertionHeader> {
    let id = read_bounded_id(row, 0, "assertion", AssertionId::parse)?;
    let subject_id = read_bounded_id(row, 1, "subject", EntityId::parse)?;
    let definition_id = read_bounded_id(row, 2, "definition", DefinitionId::parse)?;
    let value_kind = read_bounded_short_str(row, 3, "value_kind", 16)?;
    let has_text = match row.get_ref(4).map_err(|err| map_sql("text_value", err))? {
        rusqlite::types::ValueRef::Null => false,
        rusqlite::types::ValueRef::Text(_) => true,
        _ => {
            return Err(Error::Storage {
                message: format!("invalid text assertion {id}"),
            });
        }
    };
    let bool_val = match row.get_ref(5).map_err(|err| map_sql("bool_value", err))? {
        rusqlite::types::ValueRef::Null => None,
        rusqlite::types::ValueRef::Integer(v) => Some(v),
        _ => {
            return Err(Error::Storage {
                message: format!("invalid boolean assertion {id}"),
            });
        }
    };
    let target_id = read_bounded_opt_id(row, 6, "target", EntityId::parse)?;
    let value = match value_kind {
        "text" => {
            if !has_text || bool_val.is_some() || target_id.is_some() {
                return Err(Error::Storage {
                    message: format!("invalid text assertion {id}"),
                });
            }
            AssertionValueHeader::Text
        }
        "boolean" => match bool_val {
            Some(0) if !has_text && target_id.is_none() => AssertionValueHeader::Boolean(false),
            Some(1) if !has_text && target_id.is_none() => AssertionValueHeader::Boolean(true),
            _ => {
                return Err(Error::Storage {
                    message: format!("invalid boolean assertion {id}"),
                });
            }
        },
        "reference" => match target_id {
            Some(target) if !has_text && bool_val.is_none() => {
                AssertionValueHeader::Reference(target)
            }
            _ => {
                return Err(Error::Storage {
                    message: format!("invalid reference assertion {id}"),
                });
            }
        },
        "unknown" => {
            if has_text || bool_val.is_some() || target_id.is_some() {
                return Err(Error::Storage {
                    message: format!("invalid unknown assertion {id}"),
                });
            }
            AssertionValueHeader::Unknown
        }
        _ => {
            return Err(Error::Storage {
                message: format!("invalid value kind on {id}"),
            });
        }
    };
    let canon = Canon::parse(read_bounded_short_str(row, 7, "canon", 32)?)?;
    let archived = read_bounded_flag(row, 14, "archived")?;
    Ok(AssertionHeader {
        id,
        subject_id,
        definition_id,
        value,
        canon,
        archived,
    })
}

fn validate_header_against_subject_and_definition(
    header: &AssertionHeader,
    subject: &Entity,
    definition: &Definition,
) -> Result<()> {
    let fits = matches!(
        (definition.value_class, &header.value),
        (_, AssertionValueHeader::Unknown)
            | (ValueClass::Text, AssertionValueHeader::Text)
            | (ValueClass::Boolean, AssertionValueHeader::Boolean(_))
            | (ValueClass::Reference, AssertionValueHeader::Reference(_))
    );
    if !fits {
        return Err(Error::validation(
            ValidationReason::ValueMismatch,
            "value",
            Some(header.id.as_str()),
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
    Ok(())
}

fn read_bounded_claim_payload(
    row: &rusqlite::Row<'_>,
    header: AssertionHeader,
    target: Option<&Entity>,
    has_prior_claims: bool,
    budget: &mut ContextBudget<'_>,
) -> Result<ContextClaim> {
    let base = CONTEXT_CLAIM_BASE_BYTES
        + usize::from(has_prior_claims)
        + header.canon.as_str().len()
        + json_bool_bytes(header.archived)
        + match &header.value {
            AssertionValueHeader::Text => CONTEXT_TEXT_VALUE_OVERHEAD,
            AssertionValueHeader::Boolean(flag) => {
                CONTEXT_BOOL_VALUE_BASE_BYTES + json_bool_bytes(*flag)
            }
            AssertionValueHeader::Unknown => CONTEXT_UNKNOWN_VALUE_BYTES,
            AssertionValueHeader::Reference(_) => {
                let target_label_len = target
                    .as_ref()
                    .map_or(0, |t| json_str_bytes(t.label.as_bytes()));
                CONTEXT_REF_VALUE_OVERHEAD + target_label_len
            }
        };
    budget.charge(base)?;
    let value = match header.value {
        AssertionValueHeader::Text => Value::Text(read_bounded_text(row, 4, "text_value", budget)?),
        AssertionValueHeader::Boolean(flag) => Value::Boolean(flag),
        AssertionValueHeader::Reference(id) => Value::Reference(id),
        AssertionValueHeader::Unknown => Value::Unknown,
    };
    let source_status_raw = read_bounded_opt_text(row, 8, "source_status_raw", budget)?;
    let external_ref = read_bounded_opt_text(row, 9, "provenance_external_ref", budget)?;
    let locator = read_bounded_opt_text(row, 10, "provenance_locator", budget)?;
    let fragment = read_bounded_opt_text(row, 11, "provenance_fragment", budget)?;
    let source_revision = read_bounded_opt_text(row, 12, "provenance_source_revision", budget)?;
    let scope_note = read_bounded_opt_text(row, 13, "scope_note", budget)?;
    Ok(ContextClaim {
        assertion: Assertion {
            id: header.id,
            subject_id: header.subject_id,
            definition_id: header.definition_id,
            value,
            canon: header.canon,
            source_status_raw,
            provenance: Provenance {
                external_ref,
                locator,
                fragment,
                source_revision,
            },
            scope_note,
            archived: header.archived,
        },
        target: target.cloned(),
    })
}

fn read_entity_context(tx: &Transaction, id: &EntityId) -> Result<EntityContext> {
    let mut budget = ContextBudget::new(id);
    budget.charge(CONTEXT_ROOT_BASE_BYTES)?;

    let entity = read_bounded_entity(tx, id, &budget)?
        .ok_or_else(|| Error::validation(ValidationReason::NotFound, "id", Some(id.as_str())))?;
    budget.charge(json_str_bytes(entity.label.as_bytes()))?;

    let mut checked_owner_types: BTreeSet<String> = BTreeSet::new();
    let type_label = match &entity.type_id {
        Some(type_id) => {
            let label = read_bounded_type_label(tx, type_id, &mut budget)?;
            checked_owner_types.insert(type_id.as_str().to_owned());
            Some(label)
        }
        None => {
            budget.charge(JSON_NULL_BYTES)?;
            None
        }
    };

    let mut checked_entities: BTreeMap<String, Entity> = BTreeMap::new();
    checked_entities.insert(entity.id.as_str().to_owned(), entity.clone());
    let mut checked_owner_entities: BTreeSet<String> = BTreeSet::new();

    let mut checked_definitions: BTreeMap<String, Definition> = BTreeMap::new();
    let mut active_outgoing: BTreeMap<String, ContextOutgoingGroup> = BTreeMap::new();
    let mut archived_outgoing: BTreeMap<String, ContextOutgoingGroup> = BTreeMap::new();

    {
        let mut load_definition = |row: &rusqlite::Row<'_>| -> Result<()> {
            let definition = read_definition_from_row(row)?;
            require_bounded_definition_owner(
                tx,
                &definition,
                &mut checked_owner_types,
                &checked_entities,
                &mut checked_owner_entities,
            )?;
            budget.add_group()?;
            let has_prior_groups = if definition.archived {
                !archived_outgoing.is_empty()
            } else {
                !active_outgoing.is_empty()
            };
            budget.charge(outgoing_group_json_bytes(&definition, has_prior_groups))?;
            let key = definition.id.as_str().to_owned();
            checked_definitions.insert(key.clone(), definition.clone());
            let group = ContextOutgoingGroup {
                definition,
                claims: Vec::new(),
                has_archived_claims: false,
            };
            if group.definition.archived {
                archived_outgoing.insert(key, group);
            } else {
                active_outgoing.insert(key, group);
            }
            Ok(())
        };

        if let Some(type_id) = &entity.type_id {
            let mut stmt = tx
                .prepare(
                    "SELECT id, owner_type_id, owner_entity_id, value_class, cardinality, archived, label
                     FROM definitions WHERE owner_entity_id = ?1 OR owner_type_id = ?2 ORDER BY id",
                )
                .map_err(|err| map_sql("prepare definitions", err))?;
            let mut rows = stmt
                .query(params![entity.id.as_str(), type_id.as_str()])
                .map_err(|err| map_sql("query definitions", err))?;
            while let Some(row) = rows.next().map_err(|err| map_sql("read definition", err))? {
                load_definition(row)?;
            }
        } else {
            let mut stmt = tx
                .prepare(
                    "SELECT id, owner_type_id, owner_entity_id, value_class, cardinality, archived, label
                     FROM definitions WHERE owner_entity_id = ?1 ORDER BY id",
                )
                .map_err(|err| map_sql("prepare definitions", err))?;
            let mut rows = stmt
                .query(params![entity.id.as_str()])
                .map_err(|err| map_sql("query definitions", err))?;
            while let Some(row) = rows.next().map_err(|err| map_sql("read definition", err))? {
                load_definition(row)?;
            }
        }
    }

    #[cfg(test)]
    run_test_hook("context_after_definitions");

    // Outgoing assertions for this subject (grouped in BTreeMap and sorted in memory
    // after bounded reads to avoid SQLite TEMP B-TREE sorts over full text payloads).
    {
        let sql = format!("SELECT {ASSERTION_COLUMNS} FROM assertions WHERE subject_id = ?1");
        let mut stmt = tx
            .prepare(&sql)
            .map_err(|err| map_sql("prepare outgoing", err))?;
        let mut rows = stmt
            .query([entity.id.as_str()])
            .map_err(|err| map_sql("query outgoing", err))?;
        while let Some(row) = rows.next().map_err(|err| map_sql("read outgoing", err))? {
            let header = read_assertion_header(row)?;
            if header.subject_id != entity.id {
                return Err(Error::Storage {
                    message: format!("mismatched subject on assertion {}", header.id),
                });
            }
            let definition = lookup_cached_definition(
                tx,
                &header.definition_id,
                &mut checked_definitions,
                &mut checked_owner_types,
                &checked_entities,
                &mut checked_owner_entities,
            )?
            .clone();
            validate_header_against_subject_and_definition(&header, &entity, &definition)?;
            let target_key = match &header.value {
                AssertionValueHeader::Reference(target_id) => {
                    let _ = lookup_cached_entity(
                        tx,
                        target_id,
                        &mut checked_entities,
                        &budget,
                        ValidationReason::UnknownTarget,
                        "target_entity_id",
                    )?;
                    Some(target_id.as_str().to_owned())
                }
                _ => None,
            };
            let target = target_key
                .as_deref()
                .and_then(|key| checked_entities.get(key));
            budget.add_unique_assertion(&header.id)?;
            let def_key = definition.id.as_str().to_owned();
            let is_archived = header.archived || definition.archived;
            if !is_archived {
                let has_prior_claims = !active_outgoing
                    .get(&def_key)
                    .ok_or_else(|| Error::Storage {
                        message: "missing active outgoing group".to_string(),
                    })?
                    .claims
                    .is_empty();
                let claim =
                    read_bounded_claim_payload(row, header, target, has_prior_claims, &mut budget)?;
                active_outgoing
                    .get_mut(&def_key)
                    .expect("active outgoing group was checked")
                    .claims
                    .push(claim);
            } else {
                if !definition.archived && !archived_outgoing.contains_key(&def_key) {
                    budget.add_group()?;
                    budget.charge(outgoing_group_json_bytes(
                        &definition,
                        !archived_outgoing.is_empty(),
                    ))?;
                    archived_outgoing.insert(
                        def_key.clone(),
                        ContextOutgoingGroup {
                            definition: definition.clone(),
                            claims: Vec::new(),
                            has_archived_claims: true,
                        },
                    );
                }
                if let Some(active_group) = active_outgoing.get_mut(&def_key) {
                    active_group.has_archived_claims = true;
                }
                let has_prior_claims = !archived_outgoing
                    .get(&def_key)
                    .ok_or_else(|| Error::Storage {
                        message: "missing archived outgoing group".to_string(),
                    })?
                    .claims
                    .is_empty();
                let claim =
                    read_bounded_claim_payload(row, header, target, has_prior_claims, &mut budget)?;
                let archived_group = archived_outgoing
                    .get_mut(&def_key)
                    .expect("archived outgoing group was checked");
                if claim.assertion.archived {
                    archived_group.has_archived_claims = true;
                }
                archived_group.claims.push(claim);
            }
        }
    }

    // Incoming reference assertions pointing at this entity (grouped and sorted in
    // memory after bounded reads to avoid SQLite TEMP B-TREE payload sorts).
    let mut active_incoming: BTreeMap<(String, String), ContextIncomingGroup> = BTreeMap::new();
    let mut archived_incoming: BTreeMap<(String, String), ContextIncomingGroup> = BTreeMap::new();
    {
        let sql = format!("SELECT {ASSERTION_COLUMNS} FROM assertions WHERE target_entity_id = ?1");
        let mut stmt = tx
            .prepare(&sql)
            .map_err(|err| map_sql("prepare incoming", err))?;
        let mut rows = stmt
            .query([entity.id.as_str()])
            .map_err(|err| map_sql("query incoming", err))?;
        while let Some(row) = rows.next().map_err(|err| map_sql("read incoming", err))? {
            let header = read_assertion_header(row)?;
            match &header.value {
                AssertionValueHeader::Reference(target_id) if target_id == &entity.id => {}
                _ => {
                    return Err(Error::Storage {
                        message: format!("invalid incoming target on assertion {}", header.id),
                    });
                }
            }
            let subject_key = header.subject_id.as_str().to_owned();
            let _ = lookup_cached_entity(
                tx,
                &header.subject_id,
                &mut checked_entities,
                &budget,
                ValidationReason::UnknownSubject,
                "subject_id",
            )?;
            let definition = lookup_cached_definition(
                tx,
                &header.definition_id,
                &mut checked_definitions,
                &mut checked_owner_types,
                &checked_entities,
                &mut checked_owner_entities,
            )?
            .clone();
            let subject = checked_entities
                .get(&subject_key)
                .expect("subject was cached");
            validate_header_against_subject_and_definition(&header, subject, &definition)?;
            budget.add_unique_assertion(&header.id)?;
            let is_archived = header.archived || definition.archived;
            let group_key = (subject_key.clone(), definition.id.as_str().to_owned());
            let target_map = if is_archived {
                &mut archived_incoming
            } else {
                &mut active_incoming
            };
            if !target_map.contains_key(&group_key) {
                budget.add_group()?;
                let subject = checked_entities
                    .get(&subject_key)
                    .expect("subject was cached");
                budget.charge(incoming_group_json_bytes(
                    subject,
                    &definition,
                    !target_map.is_empty(),
                ))?;
                target_map.insert(
                    group_key.clone(),
                    ContextIncomingGroup {
                        subject: subject.clone(),
                        definition,
                        claims: Vec::new(),
                    },
                );
            }
            let has_prior_claims = !target_map
                .get(&group_key)
                .expect("incoming group was just inserted")
                .claims
                .is_empty();
            let claim = read_bounded_claim_payload(
                row,
                header,
                Some(&entity),
                has_prior_claims,
                &mut budget,
            )?;
            target_map
                .get_mut(&group_key)
                .expect("incoming group was just inserted")
                .claims
                .push(claim);
        }
    }

    for group in active_outgoing
        .values_mut()
        .chain(archived_outgoing.values_mut())
    {
        group
            .claims
            .sort_by(|a, b| a.assertion.id.as_str().cmp(b.assertion.id.as_str()));
    }
    for group in active_incoming
        .values_mut()
        .chain(archived_incoming.values_mut())
    {
        group
            .claims
            .sort_by(|a, b| a.assertion.id.as_str().cmp(b.assertion.id.as_str()));
    }

    Ok(EntityContext {
        entity,
        type_label,
        outgoing_groups: active_outgoing.into_values().collect(),
        incoming_groups: active_incoming.into_values().collect(),
        archived_outgoing_groups: archived_outgoing.into_values().collect(),
        archived_incoming_groups: archived_incoming.into_values().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::Store;
    use crate::{Error, UniverseId, ValidationReason};

    #[test]
    fn stored_id_errors_do_not_echo_the_value() {
        let error =
            super::parse_stored_id("target", "SECRET_MARKER", crate::model::EntityId::parse)
                .expect_err("invalid stored id");
        assert!(!error.to_string().contains("SECRET_MARKER"), "{error}");
    }

    #[test]
    fn an_invalid_stored_universe_id_is_rejected_without_being_echoed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("tampered-universe.sqlite");
        let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").expect("universe");
        let store = Store::create(&path, universe).expect("create");
        store
            .conn
            .execute(
                "UPDATE metadata SET universe_id = ?1",
                ["SECRET_LORE_123456789012345678901234"],
            )
            .expect("tamper");

        let error = super::read_universe(&store.conn).expect_err("invalid stored id");
        assert!(!error.to_string().contains("SECRET_LORE"), "{error}");
        assert!(
            matches!(
                error,
                Error::Validation {
                    reason: ValidationReason::ForeignDatabase,
                    field: "universe_id",
                    ..
                }
            ),
            "{error}"
        );
    }

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
    fn export_keeps_the_handle_universe_when_metadata_changes_elsewhere() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("export.sqlite");
        let original = UniverseId::parse("11111111-1111-4111-8111-111111111111").expect("universe");
        let other = UniverseId::parse("22222222-2222-4222-8222-222222222222").expect("universe");
        let mut store = Store::create(&path, original.clone()).expect("create");
        let conn = rusqlite::Connection::open(&path).expect("external connection");
        conn.execute(
            "UPDATE metadata SET universe_id = ?1",
            rusqlite::params![other.as_str()],
        )
        .expect("metadata update");
        drop(conn);

        let snapshot = store.export_snapshot().expect("export");
        assert_eq!(snapshot.universe_id, original);
        let outcome = store
            .import_snapshot(snapshot)
            .expect("import with the cached identity");
        assert_eq!(outcome.inserted, 0);
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

    #[cfg(unix)]
    #[test]
    fn private_reservation_is_owner_only_before_data_is_written() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("candidate.sqlite");
        super::reserve_file(&path, true).expect("private reserve");
        let metadata = std::fs::metadata(&path).expect("metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.len(), 0);
    }

    #[test]
    fn context_reader_overlaps_atomic_import_commit_with_zero_tearing_and_releases_lock() {
        use crate::{
            Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId, DefinitionLabel,
            Entity, EntityId, Owner, Provenance, Snapshot, Value, ValueClass,
        };
        use std::sync::{mpsc, Arc};

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("overlap.sqlite");
        let universe = UniverseId::parse("11111111-1111-4111-8111-111111111111").expect("universe");
        let e1 = EntityId::parse("33333333-3333-4333-8333-333333333301").expect("entity");
        let new_def =
            DefinitionId::parse("44444444-4444-4444-8444-444444444401").expect("definition");
        let new_ass =
            AssertionId::parse("55555555-5555-4555-8555-555555555501").expect("assertion");

        let mut writer_store = Store::create(&path, universe.clone()).expect("create");
        writer_store
            .create_entity(Entity {
                id: e1.clone(),
                label: "Mara".into(),
                type_id: None,
                page_id: None,
            })
            .expect("create entity");
        let reader_store = Store::open(&path).expect("open reader");

        let (reader_paused_tx, reader_paused_rx) = mpsc::channel();
        let (writer_ready_tx, writer_ready_rx) = mpsc::channel();
        let (reader_done_tx, reader_done_rx) = mpsc::channel();

        let e1_reader = e1.clone();
        let path_reader = path.clone();
        let reader_thread = std::thread::spawn(move || {
            let rx = std::sync::Mutex::new(writer_ready_rx);
            super::TEST_HOOK.with(|cell| {
                *cell.borrow_mut() = Some(Arc::new(move |point| {
                    if point == "context_after_definitions" {
                        reader_paused_tx.send(()).expect("signal reader paused");
                        rx.lock()
                            .expect("lock rx")
                            .recv()
                            .expect("wait writer ready");
                    }
                }));
            });
            let mid_read = reader_store.entity_context(&e1_reader).expect("mid read");
            super::TEST_HOOK.with(|cell| {
                *cell.borrow_mut() = None;
            });
            reader_done_tx.send(()).expect("signal reader done");
            (mid_read, reader_store, path_reader)
        });

        // Ensure reader is inside read_entity_context (past definitions, holding SHARED lock).
        reader_paused_rx.recv().expect("wait reader paused");

        let rx_writer = std::sync::Mutex::new(reader_done_rx);
        super::TEST_HOOK.with(|cell| {
            *cell.borrow_mut() = Some(Arc::new(move |point| {
                if point == "before_write_commit" {
                    writer_ready_tx.send(()).expect("signal writer ready");
                    rx_writer
                        .lock()
                        .expect("lock rx")
                        .recv()
                        .expect("wait reader done");
                }
            }));
        });
        let outcome = writer_store
            .import_snapshot(Snapshot {
                universe_id: universe,
                pages: vec![],
                types: vec![],
                entities: vec![],
                definitions: vec![Definition {
                    id: new_def.clone(),
                    owner: Owner::Entity(e1.clone()),
                    value_class: ValueClass::Text,
                    cardinality: Cardinality::Singular,
                    label: Some(DefinitionLabel::parse("callsign").expect("label")),
                    archived: false,
                }],
                assertions: vec![Assertion {
                    id: new_ass.clone(),
                    subject_id: e1.clone(),
                    definition_id: new_def.clone(),
                    value: Value::Text("Lantern".into()),
                    canon: Canon::Acordado,
                    source_status_raw: None,
                    provenance: Provenance::absent(),
                    scope_note: None,
                    archived: false,
                }],
            })
            .expect("atomic import");
        super::TEST_HOOK.with(|cell| {
            *cell.borrow_mut() = None;
        });
        assert_eq!(outcome.inserted, 2);

        let (mid_read, reader_store, _) = reader_thread.join().expect("join reader");
        assert!(mid_read.outgoing_groups.is_empty());

        let post_read = reader_store.entity_context(&e1).expect("post read");
        assert_eq!(post_read.outgoing_groups.len(), 1);
        assert_eq!(post_read.outgoing_groups[0].definition.id, new_def);
        assert_eq!(post_read.outgoing_groups[0].claims.len(), 1);
        assert_eq!(post_read.outgoing_groups[0].claims[0].assertion.id, new_ass);

        drop(writer_store);
        drop(reader_store);
        let checker = rusqlite::Connection::open(&path).expect("checker");
        checker
            .execute_batch("BEGIN EXCLUSIVE; ROLLBACK;")
            .expect("read lock released");
    }
}
