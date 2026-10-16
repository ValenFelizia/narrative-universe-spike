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

#[test]
fn t14_export_and_import_new_are_separate_processes() {
    let scratch = scratch("t14-source.sqlite");
    let json = scratch.path.with_file_name("t14.json");
    let imported = scratch.path.with_file_name("t14-imported.sqlite");
    let seeded = success(&run(&[
        std::ffi::OsStr::new("seed"),
        scratch.path.as_os_str(),
    ]));
    assert_eq!(seeded, EXPECTED);
    let exported = success(&run(&[
        std::ffi::OsStr::new("export"),
        scratch.path.as_os_str(),
        json.as_os_str(),
    ]));
    assert!(exported.starts_with("bytes="), "{exported}");
    assert!(json.is_file());
    let imported_out = success(&run(&[
        std::ffi::OsStr::new("import-new"),
        imported.as_os_str(),
        json.as_os_str(),
    ]));
    assert!(imported_out.contains("inserted=7"), "{imported_out}");
    assert!(imported_out.contains("unchanged=0"), "{imported_out}");
    let store = Store::open(&imported).expect("imported");
    let page = store
        .page(
            &narrative_universe::PageId::parse("22222222-2222-4222-8222-222222222222")
                .expect("page"),
        )
        .expect("page");
    assert_eq!(page.title, "Página libre");
    assert_eq!(page.body, "Texto con acento áéíóú ñ y emoji 📚");
    let narrator = store
        .entity(&EntityId::parse("55555555-5555-4555-8555-555555555555").expect("narrator"))
        .expect("narrator");
    assert_eq!(narrator.label, "Narrador sintético");
    let house = EntityId::parse("66666666-6666-4666-8666-666666666666").expect("house");
    let assertion = store
        .assertion(
            &narrative_universe::AssertionId::parse("88888888-8888-4888-8888-888888888888")
                .expect("assertion"),
        )
        .expect("assertion");
    assert_eq!(assertion.canon, Canon::Provisional);
    assert_eq!(assertion.value, Value::Reference(house));
    assert_eq!(assertion.scope_note.as_deref(), Some("al inicio"));
    assert_eq!(assertion.source_status_raw, None);
    assert_eq!(
        assertion.provenance,
        narrative_universe::Provenance::absent()
    );
    assert!(!assertion.archived);
    assert_eq!(
        store
            .definition(
                &narrative_universe::DefinitionId::parse("77777777-7777-4777-8777-777777777777")
                    .expect("definition")
            )
            .expect("definition")
            .owner,
        Owner::Type(
            narrative_universe::TypeId::parse("33333333-3333-4333-8333-333333333333")
                .expect("type")
        )
    );
}

#[test]
fn late_import_conflict_from_another_process_keeps_the_original() {
    let scratch = scratch("conflict-source.sqlite");
    let json = scratch.path.with_file_name("conflict.json");
    success(&run(&[
        std::ffi::OsStr::new("seed"),
        scratch.path.as_os_str(),
    ]));
    let before = bytes(&scratch.path);
    std::fs::write(
        &json,
        r#"{"format_version":1,"universe_id":"11111111-1111-4111-8111-111111111111","pages":[{"id":"22222222-2222-4222-8222-222222222221","title":"Nueva","body":"no queda"},{"id":"22222222-2222-4222-8222-222222222222","title":"Cambiada","body":"cambia"}],"types":[],"entities":[],"definitions":[],"assertions":[]}"#,
    )
    .expect("json");
    let output = run(&[
        std::ffi::OsStr::new("import"),
        scratch.path.as_os_str(),
        json.as_os_str(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(
        stderr.contains("import conflict page 22222222-2222-4222-8222-222222222222"),
        "{stderr}"
    );
    assert_eq!(bytes(&scratch.path), before);
    let store = Store::open(&scratch.path).expect("reopen");
    assert_eq!(
        store
            .page(
                &narrative_universe::PageId::parse("22222222-2222-4222-8222-222222222222")
                    .expect("page")
            )
            .expect("page")
            .title,
        "Página libre"
    );
    match store
        .page(
            &narrative_universe::PageId::parse("22222222-2222-4222-8222-222222222221")
                .expect("new"),
        )
        .expect_err("new page")
    {
        narrative_universe::Error::Validation {
            reason: narrative_universe::ValidationReason::NotFound,
            ..
        } => {}
        other => panic!("expected not found, got {other}"),
    }
}

#[test]
fn competing_seed_processes_keep_the_winner() {
    let scratch = scratch("compete.sqlite");
    let path = scratch.path.as_os_str();
    let mut children = Vec::new();
    for _ in 0..8 {
        children.push(
            Command::new(env!("CARGO_BIN_EXE_spike"))
                .arg("seed")
                .arg(path)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("spawn seed"),
        );
    }
    let mut successes = 0;
    let mut failures = Vec::new();
    for child in children {
        let output = child.wait_with_output().expect("wait");
        if output.status.code() == Some(0) {
            successes += 1;
        } else {
            failures.push(String::from_utf8(output.stderr).expect("stderr"));
        }
    }
    assert_eq!(successes, 1, "{failures:?}");
    assert!(scratch.path.is_file(), "winner file missing; {failures:?}");
    for stderr in &failures {
        assert!(stderr.contains("PathExists"), "{stderr}");
    }
    let inspected = success(&run(&[std::ffi::OsStr::new("inspect"), path]));
    assert_eq!(inspected, EXPECTED);
}
