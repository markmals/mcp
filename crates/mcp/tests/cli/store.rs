//! Store states the commands must handle: older and newer schemas, foreign files, and locks.

use diesel::connection::SimpleConnection;
use diesel::{Connection, SqliteConnection};

use crate::support::{Fixture, assert_matches_output_schema};

/// Opens the fixture's store directly, as another program would.
fn open(fixture: &Fixture) -> SqliteConnection {
    SqliteConnection::establish(fixture.store().to_str().unwrap()).unwrap()
}

/// A store from before the first migration: the bookkeeping table, nothing applied.
fn older_store(fixture: &Fixture) {
    open(fixture)
        .batch_execute(
            "CREATE TABLE __diesel_schema_migrations (\
             version VARCHAR(50) PRIMARY KEY NOT NULL, \
             run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)",
        )
        .unwrap();
}

/// SQLite databases another program wrote: schema objects, but no mcp migration history.
const FOREIGN_SCHEMAS: [&str; 2] = [
    "CREATE TABLE contacts (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
     INSERT INTO contacts (name) VALUES ('Ada');",
    "CREATE VIEW answer AS SELECT 42 AS value;",
];

#[test]
fn an_older_store_is_refused_until_store_migrate_upgrades_it() {
    let fixture = Fixture::new();
    older_store(&fixture);

    let error = fixture.run(&["notes", "list"]).assert_failed(3, "PRECONDITION_FAILED");
    assert!(error["suggestions"].to_string().contains("store migrate"), "{error}");
    assert_eq!(error["details"]["pendingMigrations"].as_array().unwrap().len(), 1);
    let create = ["notes", "create", "--name", "a", "--title", "A"];
    fixture.run(&create).assert_failed(3, "PRECONDITION_FAILED");
    let context = fixture.run(&["context", "show"]).success();
    assert_eq!(context["store"]["state"], "needs-migration");

    let before = fixture.snapshot();
    let plan = fixture.run(&["store", "migrate", "--dry-run"]).success();
    assert_eq!(plan["status"], "dry-run");
    assert_eq!(plan["migrations"], context["store"]["pendingMigrations"]);
    assert_eq!(fixture.snapshot(), before);

    let applied = fixture.run(&["store", "migrate"]).success();
    assert_eq!(applied["status"], "completed");
    assert_eq!(applied["migrations"], plan["migrations"]);
    assert_eq!(applied["storeCreated"], false);
    assert_matches_output_schema(&["store", "migrate"], &applied);

    let again = fixture.run(&["store", "migrate"]).success();
    assert_eq!(again["migrations"], serde_json::json!([]));
    fixture.run(&create).success();
    assert_eq!(fixture.run(&["context", "show"]).success()["store"]["notes"], 1);
}

#[test]
fn store_migrate_creates_a_new_store() {
    let fixture = Fixture::new();
    let receipt = fixture.run(&["store", "migrate"]).success();
    assert_eq!(receipt["storeCreated"], true);
    assert_eq!(receipt["migrations"].as_array().unwrap().len(), 1);
    let context = fixture.run(&["context", "show"]).success();
    assert_eq!(context["store"]["state"], "ready");
    assert_eq!(context["store"]["notes"], 0);
}

#[test]
fn a_store_from_a_newer_mcp_is_refused_and_left_alone() {
    let fixture = Fixture::seeded();
    open(&fixture)
        .batch_execute("INSERT INTO __diesel_schema_migrations (version) VALUES ('99991231000000')")
        .unwrap();
    let before = fixture.snapshot();
    for args in
        [&["notes", "list"][..], &["store", "migrate"][..], &["notes", "delete", "standup"][..]]
    {
        let error = fixture.run(args).assert_failed(3, "PRECONDITION_FAILED");
        assert!(error["message"].as_str().unwrap().contains("newer"), "{error}");
    }
    let context = fixture.run(&["context", "show"]).success();
    assert_eq!(context["store"]["state"], "unusable");
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn a_file_that_is_not_a_store_is_refused_and_left_alone() {
    let fixture = Fixture::new();
    std::fs::write(fixture.store(), "not a database, just text\n".repeat(40)).unwrap();
    let before = fixture.snapshot();
    fixture.run(&["notes", "list"]).assert_failed(3, "PRECONDITION_FAILED");
    let create = ["notes", "create", "--name", "a", "--title", "A"];
    fixture.run(&create).assert_failed(3, "PRECONDITION_FAILED");
    let context = fixture.run(&["context", "show"]).success();
    assert_eq!(context["store"]["state"], "unusable");
    assert!(context["store"]["problem"].is_string());
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn a_database_another_program_wrote_is_refused_by_every_command_and_left_alone() {
    let commands: [&[&str]; 8] = [
        &["store", "migrate"],
        &["store", "migrate", "--dry-run"],
        &["notes", "list"],
        &["notes", "get", "groceries"],
        &["notes", "create", "--name", "a", "--title", "A"],
        &["notes", "create", "--name", "a", "--title", "A", "--dry-run"],
        &["notes", "update", "groceries", "--title", "B"],
        &["notes", "delete", "groceries"],
    ];
    let expected = format!("no {} migration history", mcp::NAME);
    for schema in FOREIGN_SCHEMAS {
        let fixture = Fixture::new();
        open(&fixture).batch_execute(schema).unwrap();
        let before = fixture.snapshot();
        for args in commands {
            let error = fixture.run(args).assert_failed(3, "PRECONDITION_FAILED");
            let message = error["message"].as_str().unwrap();
            assert!(message.contains(&expected), "{args:?}: {error}");
        }
        let context = fixture.run(&["context", "show"]).success();
        assert_eq!(context["store"]["state"], "unusable", "{schema}");
        assert_eq!(fixture.snapshot(), before, "{schema}");
    }
}

#[test]
fn a_locked_store_is_unavailable_and_retryable() {
    let fixture = Fixture::seeded();
    let mut holder = open(&fixture);
    holder.batch_execute("BEGIN EXCLUSIVE").unwrap();
    let outcome = fixture.run(&["notes", "update", "groceries", "--title", "Locked out"]);
    let error = outcome.assert_failed(5, "UNAVAILABLE");
    assert_eq!(error["retryable"], true);
    holder.batch_execute("ROLLBACK").unwrap();
    let retried = fixture.run(&["notes", "update", "groceries", "--title", "Locked out"]).success();
    assert_eq!(retried["action"], "updated");
}
