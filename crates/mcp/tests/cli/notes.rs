//! The notes commands against a real store: effects, receipts, errors, and dry runs.

use std::process::Child;
use std::time::Duration;

use crate::support::{Fixture, Outcome, assert_matches_output_schema};

#[test]
fn create_then_verify_with_get_and_list() {
    let fixture = Fixture::new();
    let receipt = fixture
        .run(&["notes", "create", "--name", "groceries", "--title", "Groceries", "--body", "Milk"])
        .success();
    assert_eq!(receipt["status"], "completed");
    assert_eq!(receipt["action"], "created");
    assert_eq!(receipt["storeCreated"], true);
    assert_eq!(receipt["store"], fixture.store().display().to_string());
    assert_eq!(receipt["note"]["revision"], 1);
    assert_eq!(receipt["caveats"].as_array().unwrap().len(), 0);
    assert_matches_output_schema(&["notes", "create"], &receipt);

    let note = fixture.run(&["notes", "get", "groceries"]).success();
    assert_eq!(note, receipt["note"]);
    let page = fixture.run(&["notes", "list"]).success();
    assert_eq!(page["items"][0], receipt["note"]);
    assert_eq!(page["hasMore"], false);
    assert!(page["nextCursor"].is_null());

    let second =
        fixture.run(&["notes", "create", "--name", "standup", "--title", "Standup"]).success();
    assert_eq!(second["storeCreated"], false);
    assert_eq!(second["note"]["body"], "");
}

#[test]
fn creating_a_taken_name_conflicts_and_changes_nothing() {
    let fixture = Fixture::seeded();
    let before = fixture.snapshot();
    let outcome = fixture.run(&["notes", "create", "--name", "groceries", "--title", "Other"]);
    let error = outcome.assert_failed(4, "CONFLICT");
    assert_eq!(error["details"]["revision"], 1);
    assert!(error["suggestions"].to_string().contains("notes get groceries"));
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn reading_a_missing_note_is_not_found() {
    let fixture = Fixture::seeded();
    let error = fixture.run(&["notes", "get", "nothing"]).assert_failed(3, "NOT_FOUND");
    assert_eq!(error["details"]["name"], "nothing");
    assert!(error["suggestions"].to_string().contains("notes list"));
}

#[test]
fn update_bumps_the_revision_and_repeating_it_changes_nothing() {
    let fixture = Fixture::seeded();
    let args =
        ["notes", "update", "groceries", "--title", "Sunday groceries", "--expect-revision", "1"];
    let receipt = fixture.run(&args).success();
    assert_eq!(receipt["action"], "updated");
    assert_eq!(receipt["changed"], serde_json::json!(["title"]));
    assert_eq!(receipt["previousRevision"], 1);
    assert_eq!(receipt["note"]["revision"], 2);
    assert_eq!(receipt["note"]["body"], "Milk, eggs");
    assert_matches_output_schema(&["notes", "update"], &receipt);

    let again =
        fixture.run(&["notes", "update", "groceries", "--title", "Sunday groceries"]).success();
    assert_eq!(again["action"], "unchanged");
    assert_eq!(again["note"]["revision"], 2);
    assert_eq!(again["note"]["updatedAt"], receipt["note"]["updatedAt"]);

    let stored = fixture.run(&["notes", "get", "groceries"]).success();
    assert_eq!(stored["title"], "Sunday groceries");
    assert_eq!(stored["revision"], 2);
}

#[test]
fn a_stale_revision_conflicts_and_reports_the_current_one() {
    let fixture = Fixture::seeded();
    fixture.run(&["notes", "update", "groceries", "--body", "Bread"]).success();
    let before = fixture.snapshot();
    for args in [
        &["notes", "update", "groceries", "--title", "Mine", "--expect-revision", "1"][..],
        &["notes", "delete", "groceries", "--expect-revision", "1"][..],
    ] {
        let error = fixture.run(args).assert_failed(4, "CONFLICT");
        assert_eq!(error["details"]["expectedRevision"], 1);
        assert_eq!(error["details"]["actualRevision"], 2);
    }
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn an_update_needs_a_field_to_change() {
    let fixture = Fixture::seeded();
    let error = fixture.run(&["notes", "update", "groceries"]).assert_failed(2, "INVALID_ARGUMENT");
    assert_eq!(error["details"]["arguments"], serde_json::json!(["title", "body"]));
}

#[test]
fn delete_returns_the_note_and_a_second_delete_is_not_found() {
    let fixture = Fixture::seeded();
    let receipt = fixture.run(&["notes", "delete", "standup"]).success();
    assert_eq!(receipt["action"], "deleted");
    assert_eq!(receipt["note"]["title"], "Standup notes");
    assert_matches_output_schema(&["notes", "delete"], &receipt);
    fixture.run(&["notes", "get", "standup"]).assert_failed(3, "NOT_FOUND");
    fixture.run(&["notes", "delete", "standup"]).assert_failed(3, "NOT_FOUND");
}

#[test]
fn dry_runs_return_the_real_receipt_and_change_nothing() {
    let fixture = Fixture::seeded();
    let before = fixture.snapshot();
    let cases: [(&[&str], &str); 3] = [
        (&["notes", "create", "--name", "trip", "--title", "Trip", "--dry-run"], "created"),
        (&["notes", "update", "groceries", "--body", "Bread", "--dry-run"], "updated"),
        (&["notes", "delete", "standup", "--expect-revision", "1", "--dry-run"], "deleted"),
    ];
    for (args, action) in cases {
        let receipt = fixture.run(args).success();
        assert_eq!(receipt["status"], "dry-run", "{args:?}");
        assert_eq!(receipt["action"], action);
        assert!(!receipt["caveats"].as_array().unwrap().is_empty());
        assert_matches_output_schema(&args[..2], &receipt);
    }
    assert_eq!(fixture.snapshot(), before, "a dry run changed the fixture");
    assert_eq!(fixture.run(&["notes", "get", "groceries"]).success()["body"], "Milk, eggs");
    fixture.run(&["notes", "get", "trip"]).assert_failed(3, "NOT_FOUND");
}

#[test]
fn a_dry_run_fails_exactly_as_the_real_run_would() {
    let fixture = Fixture::seeded();
    let before = fixture.snapshot();
    let conflict = ["notes", "create", "--name", "groceries", "--title", "X", "--dry-run"];
    fixture.run(&conflict).assert_failed(4, "CONFLICT");
    fixture.run(&["notes", "delete", "nothing", "--dry-run"]).assert_failed(3, "NOT_FOUND");
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn reads_and_dry_runs_never_create_a_store() {
    let mut fixture = Fixture::new();
    let nested = fixture.path("nested/dir/store.sqlite3").display().to_string();
    fixture.set_env("MCP_STORE", Some(&nested));
    let before = fixture.snapshot();

    let list = fixture.run(&["notes", "list"]);
    assert_eq!(list.status, 0);
    assert_eq!(list.json()["items"], serde_json::json!([]));
    assert!(list.stderr.starts_with("warning: no store at"), "{}", list.stderr);
    fixture.run(&["notes", "get", "groceries"]).assert_failed(3, "NOT_FOUND");
    fixture.run(&["context", "show"]).success();
    let receipt = fixture
        .run(&["notes", "create", "--name", "trip", "--title", "Trip", "--dry-run"])
        .success();
    assert_eq!(receipt["storeCreated"], true);
    let migrate = fixture.run(&["store", "migrate", "--dry-run"]).success();
    assert_eq!(migrate["storeCreated"], true);

    assert_eq!(fixture.snapshot(), before, "a read or dry run created files");
    assert!(!fixture.path("nested").exists());

    fixture.run(&["notes", "create", "--name", "trip", "--title", "Trip"]).success();
    assert!(fixture.path("nested/dir/store.sqlite3").is_file());
}

#[test]
fn update_and_delete_without_a_store_fail_as_their_dry_runs_do_and_create_nothing() {
    let mut fixture = Fixture::new();
    let nested = fixture.path("nested/dir/store.sqlite3").display().to_string();
    fixture.set_env("MCP_STORE", Some(&nested));
    let before = fixture.snapshot();
    for args in [
        &["notes", "update", "groceries", "--title", "Sunday"][..],
        &["notes", "delete", "groceries"][..],
    ] {
        let error = fixture.run(args).assert_failed(3, "NOT_FOUND");
        let dry_run = [args, &["--dry-run"]].concat();
        assert_eq!(fixture.run(&dry_run).assert_failed(3, "NOT_FOUND"), error, "{args:?}");
    }
    assert_eq!(fixture.snapshot(), before, "a failed change created the store");
    assert!(!fixture.path("nested").exists());
}

#[test]
fn of_two_commands_racing_to_create_the_store_exactly_one_reports_creating_it() {
    for _ in 0..5 {
        let fixture = Fixture::new();
        let mut racers: Vec<Child> = ["a", "b"]
            .into_iter()
            .map(|name| {
                fixture.start(&["notes", "create", "--name", name, "--title", name, "--body", "-"])
            })
            .collect();
        // Both wait to read --body from stdin before they look at the store; closing stdin
        // releases them together.
        std::thread::sleep(Duration::from_millis(100));
        for racer in &mut racers {
            drop(racer.stdin.take());
        }
        let created: Vec<bool> = racers
            .into_iter()
            .map(|racer| Outcome::of(racer).success()["storeCreated"].as_bool().unwrap())
            .collect();
        assert_eq!(created.iter().filter(|created| **created).count(), 1, "{created:?}");
    }
}

#[test]
fn listing_pages_through_every_note_exactly_once() {
    let fixture = Fixture::new();
    let names = ["alpha", "bravo", "charlie", "delta", "echo"];
    for name in names {
        fixture.run(&["notes", "create", "--name", name, "--title", name]).success();
    }
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let mut args = vec!["notes", "list", "--limit", "2"];
        if let Some(cursor) = &cursor {
            args.extend(["--cursor", cursor.as_str()]);
        }
        let page = fixture.run(&args).success();
        assert_matches_output_schema(&["notes", "list"], &page);
        pages += 1;
        for item in page["items"].as_array().unwrap() {
            seen.push(item["name"].as_str().unwrap().to_string());
        }
        if page["hasMore"] == false {
            assert!(page["nextCursor"].is_null());
            break;
        }
        cursor = Some(page["nextCursor"].as_str().unwrap().to_string());
    }
    assert_eq!(pages, 3);
    assert_eq!(seen, names);
}

#[test]
fn an_invalid_cursor_or_name_is_an_invalid_argument() {
    let fixture = Fixture::seeded();
    fixture
        .run(&["notes", "list", "--cursor", "Not A Cursor"])
        .assert_failed(2, "INVALID_ARGUMENT");
    let error = fixture
        .run(&["notes", "create", "--name", "Groceries List", "--title", "X"])
        .assert_failed(2, "INVALID_ARGUMENT");
    assert_eq!(error["details"]["value"], "Groceries List");
    let error = fixture
        .run(&["notes", "create", "--name", "ok", "--title", "two\nlines"])
        .assert_failed(2, "INVALID_ARGUMENT");
    assert_eq!(error["details"]["argument"], "title");
}

#[test]
fn bodies_come_from_files_and_standard_input() {
    let fixture = Fixture::new();
    std::fs::write(fixture.path("agenda.md"), "1. Plan\n2. Ship\n").unwrap();
    let from_file = fixture
        .run(&[
            "notes",
            "create",
            "--name",
            "meeting",
            "--title",
            "Meeting",
            "--body",
            "@agenda.md",
        ])
        .success();
    assert_eq!(from_file["note"]["body"], "1. Plan\n2. Ship\n");
    let from_stdin =
        fixture.run_with_stdin(&["notes", "update", "meeting", "--body", "-"], "Moved\n");
    assert_eq!(from_stdin.success()["note"]["body"], "Moved\n");
    let missing = fixture.run(&["notes", "update", "meeting", "--body", "@nothing.md"]);
    assert_eq!(missing.assert_failed(2, "INVALID_ARGUMENT")["details"]["argument"], "body");
}

#[test]
fn text_output_is_for_people() {
    let fixture = Fixture::seeded();
    let list = fixture.run(&["notes", "list", "--output", "text"]);
    assert_eq!(list.status, 0);
    assert!(list.stdout.contains("groceries  Groceries"), "{}", list.stdout);
    let receipt = fixture.run(&["notes", "delete", "standup", "--dry-run", "--output", "text"]);
    assert!(
        receipt.stdout.starts_with("Dry run: note standup would be deleted"),
        "{}",
        receipt.stdout
    );
}
