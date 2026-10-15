#[path = "support/mod.rs"]
mod support;

use std::process::{Command, Output};

use narrative_universe::{Canon, EntityId, Owner, Store, Value};
use support::{bytes, scratch};

const EXPECTED: &str = "\
universe_id=11111111-1111-4111-8111-111111111111
pages=1
types=2
entities=2
definitions=1
assertions=1
page_body=Texto con acento áéíóú ñ y emoji 📚
subject_label=Narrador sintético
target_label=Casa sintética
owner_type_id=33333333-3333-4333-8333-333333333333
cardinality=singular
assertion_id=88888888-8888-4888-8888-888888888888
from_subject=88888888-8888-4888-8888-888888888888
to_target=88888888-8888-4888-8888-888888888888
canon=PROVISIONAL
value_kind=reference
target_id=66666666-6666-4666-8666-666666666666
scope_note=al inicio
source_status_raw=<absent>
provenance_external_ref=<absent>
provenance_locator=<absent>
provenance_fragment=<absent>
provenance_source_revision=<absent>
archived=false
";

fn run(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spike"))
        .args(args)
        .output()
        .expect("run spike")
}

fn success(output: &Output) -> String {
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf-8 stdout");
    let stderr = String::from_utf8(output.stderr.clone()).expect("utf-8 stderr");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr {stderr}\nstdout {stdout}"
    );
    assert_eq!(stderr, "");
    stdout
}

#[test]
fn t02_seed_process_then_inspect_process() {
    let scratch = scratch("t02.sqlite");
    let path = scratch.path.as_os_str();
    let seeded = success(&run(&[std::ffi::OsStr::new("seed"), path]));
    let after_seed = bytes(&scratch.path);
    let inspected = success(&run(&[std::ffi::OsStr::new("inspect"), path]));
    assert_eq!(bytes(&scratch.path), after_seed);
    assert_eq!(seeded, EXPECTED);
    assert_eq!(inspected, EXPECTED);

    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(store.counts().expect("counts").pages, 1);
    assert_eq!(store.counts().expect("counts").types, 2);
    assert_eq!(store.counts().expect("counts").entities, 2);
    assert_eq!(store.counts().expect("counts").definitions, 1);
    assert_eq!(store.counts().expect("counts").assertions, 1);
    let page = store
        .page(
            &narrative_universe::PageId::parse("22222222-2222-4222-8222-222222222222")
                .expect("page"),
        )
        .expect("page");
    assert_eq!(page.body, "Texto con acento áéíóú ñ y emoji 📚");
    let subject = EntityId::parse("55555555-5555-4555-8555-555555555555").expect("subject");
    let target = EntityId::parse("66666666-6666-4666-8666-666666666666").expect("target");
    let from = store.assertions_from_subject(&subject).expect("from");
    let to = store.assertions_to_target(&target).expect("to");
    assert_eq!(from.len(), 1);
    assert_eq!(to.len(), 1);
    assert_eq!(from[0].id, to[0].id);
    assert_eq!(from[0].id.as_str(), "88888888-8888-4888-8888-888888888888");
    assert_eq!(from[0].canon, Canon::Provisional);
    assert_eq!(from[0].value, Value::Reference(target.clone()));
    assert_eq!(from[0].scope_note.as_deref(), Some("al inicio"));
    assert_eq!(from[0].source_status_raw, None);
    assert_eq!(from[0].provenance, narrative_universe::Provenance::absent());
    assert!(!from[0].archived);
    let definition = store
        .definition(
            &narrative_universe::DefinitionId::parse("77777777-7777-4777-8777-777777777777")
                .expect("definition"),
        )
        .expect("definition");
    assert_eq!(
        definition.owner,
        Owner::Type(
            narrative_universe::TypeId::parse("33333333-3333-4333-8333-333333333333")
                .expect("type")
        )
    );
    let conn = rusqlite::Connection::open(&scratch.path).expect("raw");
    let assertion_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM assertions", [], |row| row.get(0))
        .expect("count");
    assert_eq!(assertion_rows, 1);

    let again = run(&[std::ffi::OsStr::new("seed"), path]);
    assert_eq!(again.status.code(), Some(1));
    assert_eq!(bytes(&scratch.path), after_seed);
    let stderr = String::from_utf8(again.stderr).expect("stderr");
    assert!(stderr.contains("PathExists"), "{stderr}");
}

#[test]
fn inspect_missing_file_does_not_create_it() {
    let scratch = scratch("inspect-missing.sqlite");
    let output = run(&[std::ffi::OsStr::new("inspect"), scratch.path.as_os_str()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!scratch.path.exists());
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("PathMissing"), "{stderr}");
}
