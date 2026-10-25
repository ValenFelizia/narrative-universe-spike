use std::path::Path;
use std::process::ExitCode;

use narrative_universe::{
    export_json, import_json, import_new_json, migrate_v1_to_v2, read_limited, verify_v2,
    write_exclusive, Assertion, AssertionId, Canon, Cardinality, Definition, DefinitionId, Entity,
    EntityId, EntityType, Error, MigrationError, Owner, Page, PageId, Provenance, Store, TypeId,
    UniverseId, Value, ValueClass, VerificationError,
};

const UNIVERSE: &str = "11111111-1111-4111-8111-111111111111";
const PAGE: &str = "22222222-2222-4222-8222-222222222222";
const TYPE_CHARACTER: &str = "33333333-3333-4333-8333-333333333333";
const TYPE_HOUSE: &str = "44444444-4444-4444-8444-444444444444";
const ENTITY_NARRATOR: &str = "55555555-5555-4555-8555-555555555555";
const ENTITY_HOUSE: &str = "66666666-6666-4666-8666-666666666666";
const DEFINITION: &str = "77777777-7777-4777-8777-777777777777";
const ASSERTION: &str = "88888888-8888-4888-8888-888888888888";
const PAGE_BODY: &str = "Texto con acento áéíóú ñ y emoji 📚";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage) => {
            usage();
            ExitCode::from(2)
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

enum CliError {
    Usage,
    Core(Error),
    Migration(MigrationError),
    Verification(VerificationError),
}

impl std::fmt::Debug for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl From<Error> for CliError {
    fn from(err: Error) -> Self {
        Self::Core(err)
    }
}

impl From<MigrationError> for CliError {
    fn from(err: MigrationError) -> Self {
        Self::Migration(err)
    }
}

impl From<VerificationError> for CliError {
    fn from(err: VerificationError) -> Self {
        Self::Verification(err)
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage => formatter.write_str("invalid command line"),
            Self::Core(err) => err.fmt(formatter),
            Self::Migration(err) => err.fmt(formatter),
            Self::Verification(err) => err.fmt(formatter),
        }
    }
}

impl std::error::Error for CliError {}

fn run() -> Result<(), CliError> {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let first = args.next();
    let second = args.next();
    let extra = args.next();
    match (command.as_deref(), first, second, extra) {
        (Some("seed"), Some(path), None, None) => seed(Path::new(&path))?,
        (Some("inspect"), Some(path), None, None) => inspect(Path::new(&path))?,
        (Some("export"), Some(database), Some(json), None) => {
            export(Path::new(&database), Path::new(&json))?
        }
        (Some("import"), Some(database), Some(json), None) => {
            import(Path::new(&database), Path::new(&json))?
        }
        (Some("import-new"), Some(database), Some(json), None) => {
            import_new(Path::new(&database), Path::new(&json))?
        }
        (Some("migrate"), Some(source), Some(destination), None) => {
            migrate(Path::new(&source), Path::new(&destination))?
        }
        (Some("verify"), Some(database), None, None) => verify(Path::new(&database))?,
        _ => return Err(CliError::Usage),
    }
    Ok(())
}

fn usage() {
    eprintln!("usage: spike seed <database>");
    eprintln!("usage: spike inspect <database>");
    eprintln!("usage: spike export <database> <json>");
    eprintln!("usage: spike import <database> <json>");
    eprintln!("usage: spike import-new <database> <json>");
    eprintln!("usage: spike migrate <source-v1> <destination-v2>");
    eprintln!("usage: spike verify <database>");
}

fn migrate(source: &Path, destination: &Path) -> Result<(), MigrationError> {
    let report = migrate_v1_to_v2(source, destination)?;
    print!("{}", migration_report(&report));
    Ok(())
}

fn migration_report(report: &narrative_universe::MigrationReport) -> String {
    let mut out = String::new();
    push(&mut out, "universe_id", report.universe_id.as_str());
    push_counts(&mut out, report.counts);
    push(
        &mut out,
        "destination",
        &report.destination.display().to_string(),
    );
    for residual in &report.residuals {
        push(&mut out, "residual", &residual.display().to_string());
    }
    out
}

fn verify(database: &Path) -> Result<(), VerificationError> {
    let report = verify_v2(database)?;
    let mut out = String::new();
    push(&mut out, "verified", "true");
    push(&mut out, "universe_id", report.universe_id.as_str());
    push_counts(&mut out, report.counts);
    push(&mut out, "mentions", &report.mentions.to_string());
    print!("{out}");
    Ok(())
}

fn push_counts(out: &mut String, counts: narrative_universe::Counts) {
    push(out, "pages", &counts.pages.to_string());
    push(out, "types", &counts.types.to_string());
    push(out, "entities", &counts.entities.to_string());
    push(out, "definitions", &counts.definitions.to_string());
    push(out, "assertions", &counts.assertions.to_string());
}

fn seed(path: &Path) -> Result<(), Error> {
    let mut store = Store::create(path, UniverseId::parse(UNIVERSE)?)?;
    store.create_page(Page {
        id: PageId::parse(PAGE)?,
        title: "Página libre".to_string(),
        body: PAGE_BODY.to_string(),
    })?;
    store.create_entity_type(EntityType {
        id: TypeId::parse(TYPE_CHARACTER)?,
        label: "Personaje".to_string(),
    })?;
    store.create_entity_type(EntityType {
        id: TypeId::parse(TYPE_HOUSE)?,
        label: "Casa".to_string(),
    })?;
    store.create_entity(Entity {
        id: EntityId::parse(ENTITY_NARRATOR)?,
        label: "Narrador sintético".to_string(),
        type_id: Some(TypeId::parse(TYPE_CHARACTER)?),
        page_id: None,
    })?;
    store.create_entity(Entity {
        id: EntityId::parse(ENTITY_HOUSE)?,
        label: "Casa sintética".to_string(),
        type_id: Some(TypeId::parse(TYPE_HOUSE)?),
        page_id: None,
    })?;
    store.create_definition(Definition {
        id: DefinitionId::parse(DEFINITION)?,
        owner: Owner::Type(TypeId::parse(TYPE_CHARACTER)?),
        value_class: ValueClass::Reference,
        cardinality: Cardinality::Singular,
        archived: false,
    })?;
    store.create_assertion(Assertion {
        id: AssertionId::parse(ASSERTION)?,
        subject_id: EntityId::parse(ENTITY_NARRATOR)?,
        definition_id: DefinitionId::parse(DEFINITION)?,
        value: Value::Reference(EntityId::parse(ENTITY_HOUSE)?),
        canon: Canon::Provisional,
        source_status_raw: None,
        provenance: Provenance::absent(),
        scope_note: Some("al inicio".to_string()),
        archived: false,
    })?;
    print!("{}", report(&store)?);
    Ok(())
}

fn export(database: &Path, json_path: &Path) -> Result<(), Error> {
    let mut store = Store::open(database)?;
    let bytes = export_json(&mut store)?;
    write_exclusive(json_path, &bytes)?;
    println!("bytes={}", bytes.len());
    Ok(())
}

fn import(database: &Path, json_path: &Path) -> Result<(), Error> {
    let bytes = read_capped(json_path)?;
    let mut store = Store::open(database)?;
    let outcome = import_json(&mut store, &bytes)?;
    println!("inserted={}", outcome.inserted);
    println!("unchanged={}", outcome.unchanged);
    Ok(())
}

fn import_new(database: &Path, json_path: &Path) -> Result<(), Error> {
    let bytes = read_capped(json_path)?;
    let (_store, outcome) = import_new_json(database, &bytes)?;
    println!("inserted={}", outcome.inserted);
    println!("unchanged={}", outcome.unchanged);
    Ok(())
}

fn read_capped(path: &Path) -> Result<Vec<u8>, Error> {
    let file = std::fs::File::open(path).map_err(|err| Error::Io {
        message: format!("read {}: {err}", path.display()),
    })?;
    read_limited(file)
}

fn inspect(path: &Path) -> Result<(), Error> {
    let store = Store::open(path)?;
    print!("{}", report(&store)?);
    Ok(())
}

fn report(store: &Store) -> Result<String, Error> {
    let counts = store.counts()?;
    let subject = store.entity(&EntityId::parse(ENTITY_NARRATOR)?)?;
    let target = store.entity(&EntityId::parse(ENTITY_HOUSE)?)?;
    let page = store.page(&PageId::parse(PAGE)?)?;
    let definition = store.definition(&DefinitionId::parse(DEFINITION)?)?;
    let from = store.assertions_from_subject(&subject.id)?;
    let to = store.assertions_to_target(&target.id)?;
    if from.len() != 1 || to.len() != 1 || from[0].id != to[0].id {
        return Err(Error::Storage {
            message: format!(
                "expected one shared assertion, got {} outgoing and {} incoming",
                from.len(),
                to.len()
            ),
        });
    }
    let from_id = from[0].id.as_str().to_string();
    let to_id = to[0].id.as_str().to_string();
    let assertion = from[0].clone();
    let target_id = match &assertion.value {
        Value::Reference(id) => id.as_str().to_string(),
        _ => {
            return Err(Error::Storage {
                message: "fixture assertion is not a reference".to_string(),
            });
        }
    };
    let owner_type = match &definition.owner {
        Owner::Type(id) => id.as_str().to_string(),
        Owner::Entity(_) => {
            return Err(Error::Storage {
                message: "fixture definition is not owned by a type".to_string(),
            });
        }
    };
    let mut out = String::new();
    push(&mut out, "universe_id", store.universe_id().as_str());
    push(&mut out, "pages", &counts.pages.to_string());
    push(&mut out, "types", &counts.types.to_string());
    push(&mut out, "entities", &counts.entities.to_string());
    push(&mut out, "definitions", &counts.definitions.to_string());
    push(&mut out, "assertions", &counts.assertions.to_string());
    push(&mut out, "page_body", &page.body);
    push(&mut out, "subject_label", &subject.label);
    push(&mut out, "target_label", &target.label);
    push(&mut out, "owner_type_id", &owner_type);
    push(&mut out, "cardinality", definition.cardinality.as_str());
    push(&mut out, "assertion_id", assertion.id.as_str());
    push(&mut out, "from_subject", &from_id);
    push(&mut out, "to_target", &to_id);
    push(&mut out, "canon", assertion.canon.as_str());
    push(&mut out, "value_kind", value_kind(&assertion.value));
    push(&mut out, "target_id", &target_id);
    push(&mut out, "scope_note", &show_opt(&assertion.scope_note));
    push(
        &mut out,
        "source_status_raw",
        &show_opt(&assertion.source_status_raw),
    );
    push(
        &mut out,
        "provenance_external_ref",
        &show_opt(&assertion.provenance.external_ref),
    );
    push(
        &mut out,
        "provenance_locator",
        &show_opt(&assertion.provenance.locator),
    );
    push(
        &mut out,
        "provenance_fragment",
        &show_opt(&assertion.provenance.fragment),
    );
    push(
        &mut out,
        "provenance_source_revision",
        &show_opt(&assertion.provenance.source_revision),
    );
    push(
        &mut out,
        "archived",
        if assertion.archived { "true" } else { "false" },
    );
    Ok(out)
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Text(_) => "text",
        Value::Boolean(_) => "boolean",
        Value::Reference(_) => "reference",
        Value::Unknown => "unknown",
    }
}

fn show_opt(value: &Option<String>) -> String {
    match value {
        None => "<absent>".to_string(),
        Some(text) => text.clone(),
    }
}

fn push(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push('=');
    out.push_str(value);
    out.push('\n');
}
