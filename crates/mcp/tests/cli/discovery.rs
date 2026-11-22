//! Discovery and the output contract every command shares.

use crate::support::{Fixture, assert_matches_output_schema};

#[test]
fn bare_mcp_and_help_orient_without_failing() {
    let fixture = Fixture::new();
    for args in [&[][..], &["--help"][..]] {
        let outcome = fixture.run(args);
        assert_eq!(outcome.status, 0);
        for entry_point in ["cli search", "schema", "docs search", "context show", "--dry-run"] {
            assert!(outcome.stdout.contains(entry_point), "{entry_point}: {}", outcome.stdout);
        }
    }
}

#[test]
fn help_is_brief_rather_than_the_whole_catalogue() {
    let outcome = Fixture::new().run(&["--help"]);
    assert!(outcome.stdout.lines().count() < 40, "{}", outcome.stdout);
    assert!(!outcome.stdout.contains("expect-revision"));
}

#[test]
fn version_names_the_release() {
    let outcome = Fixture::new().run(&["--version"]);
    assert_eq!(outcome.status, 0);
    assert_eq!(outcome.stdout.trim(), format!("{} {}", mcp::NAME, env!("CARGO_PKG_VERSION")));
}

#[test]
fn cli_search_returns_a_bounded_ranked_page_with_exact_commands() {
    let result = Fixture::new().run(&["cli", "search", "delete", "a", "note"]).success();
    let items = result["items"].as_array().unwrap();
    assert!(!items.is_empty() && items.len() <= 5);
    assert_eq!(items[0]["command"], format!("{} notes delete", mcp::NAME));
    assert!(items[0]["effect"].as_str().unwrap().starts_with("modifies"));
    assert!(result.get("hasMore").is_some() && result.get("nextCursor").is_some());
    assert_matches_output_schema(&["cli", "search"], &result);
}

#[test]
fn cli_search_pages_through_every_match_with_its_cursor() {
    let fixture = Fixture::new();
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut args = vec!["cli", "search", "note", "store", "--limit", "2"];
        if let Some(cursor) = &cursor {
            args.extend(["--cursor", cursor.as_str()]);
        }
        let page = fixture.run(&args).success();
        for item in page["items"].as_array().unwrap() {
            seen.push(item["id"].as_str().unwrap().to_string());
        }
        if page["hasMore"] == false {
            assert!(page["nextCursor"].is_null());
            break;
        }
        cursor = Some(page["nextCursor"].as_str().unwrap().to_string());
    }
    let unique: std::collections::BTreeSet<&String> = seen.iter().collect();
    assert!(seen.len() > 2 && unique.len() == seen.len(), "{seen:?}");
}

#[test]
fn a_cursor_past_the_last_match_is_an_invalid_argument() {
    let fixture = Fixture::new();
    for search in [&["cli", "search", "note"][..], &["docs", "search", "pagination"][..]] {
        let first = fixture.run(&[search, &["--limit", "1"]].concat()).success();
        assert_eq!(first["nextCursor"], "1", "{search:?}");
        fixture.run(&[search, &["--cursor", "1"]].concat()).success();
        let error = fixture
            .run(&[search, &["--cursor", "1000"]].concat())
            .assert_failed(2, "INVALID_ARGUMENT");
        assert_eq!(error["details"]["argument"], "cursor", "{search:?}");
    }
}

#[test]
fn schema_lists_every_command_and_describes_one_by_any_spelling() {
    let fixture = Fixture::new();
    let list = fixture.run(&["schema"]).success();
    let commands: Vec<&str> =
        list["items"].as_array().unwrap().iter().map(|item| item["id"].as_str().unwrap()).collect();
    assert!(commands.contains(&"notes.create") && commands.contains(&"store.migrate"));

    let entry = fixture.run(&["schema", "notes", "create"]).success();
    assert_eq!(entry["id"], "notes.create");
    assert_eq!(entry["dryRun"], true);
    assert_eq!(entry["effect"]["kind"], "modifies");
    let name = entry["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|argument| argument["name"] == "name")
        .unwrap();
    assert_eq!(name["required"], true);
    assert_eq!(name["pattern"], mcp::registry::NAME_PATTERN);
    assert!(entry["output"].is_object());
    assert!(entry["errors"].as_array().unwrap().iter().any(|error| error["code"] == "CONFLICT"));
    assert_eq!(fixture.run(&["schema", "notes.create"]).success()["id"], "notes.create");
}

#[test]
fn an_unknown_command_in_schema_is_not_found_with_suggestions() {
    let outcome = Fixture::new().run(&["schema", "notes", "rename"]);
    let error = outcome.assert_failed(3, "NOT_FOUND");
    assert!(error["suggestions"].to_string().contains("schema notes"), "{error}");
}

#[test]
fn missing_input_fails_at_once_naming_the_missing_argument() {
    let outcome = Fixture::new().run(&["notes", "create", "--name", "groceries"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    assert_eq!(error["retryable"], false);
    assert!(error["details"]["missing"].to_string().contains("--title"), "{error}");
    assert!(error["details"]["arguments"].is_array());
    assert!(error["suggestions"].to_string().contains("schema notes create"));
}

#[test]
fn an_unaccepted_value_lists_what_is_accepted() {
    let outcome = Fixture::new().run(&["context", "show", "--output", "yaml"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    assert!(error["details"]["acceptedValues"].to_string().contains("text"), "{error}");
}

#[test]
fn an_out_of_range_limit_is_an_invalid_argument() {
    let outcome = Fixture::new().run(&["notes", "list", "--limit", "500"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    assert_eq!(error["details"]["value"], "500");
}

#[test]
fn a_resource_without_an_operation_lists_its_operations() {
    let outcome = Fixture::new().run(&["notes"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    assert!(error["details"]["commands"].to_string().contains("notes create"), "{error}");
}

#[test]
fn an_unknown_command_points_at_search() {
    let outcome = Fixture::new().run(&["frobnicate"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    assert!(error["suggestions"].to_string().contains("cli search"), "{error}");
}

#[test]
fn a_positional_passed_as_a_flag_gets_the_right_form() {
    let outcome = Fixture::new().run(&["notes", "get", "--name", "groceries"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    let first = error["suggestions"][0].as_str().unwrap();
    assert_eq!(first, format!("Pass the name without a flag: {} notes get <name>", mcp::NAME));
}

#[test]
fn errors_list_their_fields_in_contract_order() {
    let outcome = Fixture::new().run(&["notes", "get", "nothing"]);
    let keys = ["\"code\"", "\"message\"", "\"retryable\"", "\"details\"", "\"suggestions\""];
    let positions: Vec<usize> = keys.iter().map(|key| outcome.stderr.find(key).unwrap()).collect();
    assert!(positions.is_sorted(), "{}", outcome.stderr);
}

#[test]
fn text_output_renders_for_people_but_errors_stay_json() {
    let fixture = Fixture::new();
    let outcome = fixture.run(&["schema", "notes", "create", "--output", "text"]);
    assert_eq!(outcome.status, 0);
    assert!(outcome.stdout.starts_with(&format!("{} notes create", mcp::NAME)));
    assert!(serde_json::from_str::<serde_json::Value>(&outcome.stdout).is_err());

    fixture.run(&["schema", "no", "such", "--output", "text"]).assert_failed(3, "NOT_FOUND");
    fixture.run(&["notes", "get", "--output", "text"]).assert_failed(2, "INVALID_ARGUMENT");
}

#[test]
fn docs_search_returns_sections_to_read() {
    let fixture = Fixture::new();
    let result = fixture.run(&["docs", "search", "pagination", "cursor"]).success();
    let first = &result["items"][0];
    assert_eq!(first["path"], "docs/notes.md");
    assert_eq!(first["section"], "Pagination");
    assert!(first["excerpt"].as_str().unwrap().contains("nextCursor"));
    assert_matches_output_schema(&["docs", "search"], &result);

    let document = fixture.run(&["docs", "show", "docs/notes.md"]).success();
    assert!(document["markdown"].as_str().unwrap().contains("## Pagination"));
    let text = fixture.run(&["docs", "show", "docs/notes.md", "--output", "text"]);
    assert!(text.stdout.starts_with("# Notes"));
}

#[test]
fn docs_show_of_an_unknown_path_lists_the_documents() {
    let outcome = Fixture::new().run(&["docs", "show", "docs/nothing.md"]);
    let error = outcome.assert_failed(3, "NOT_FOUND");
    assert!(error["details"]["documents"].to_string().contains("docs/store.md"), "{error}");
}

#[test]
fn context_show_reports_the_store_and_where_its_path_came_from() {
    let mut fixture = Fixture::new();
    let from_env = fixture.run(&["context", "show"]).success();
    assert_eq!(from_env["store"]["source"], "environment");
    assert_eq!(from_env["store"]["path"], fixture.store().display().to_string());
    assert_eq!(from_env["store"]["state"], "absent");
    assert_eq!(from_env["version"], env!("CARGO_PKG_VERSION"));
    assert_matches_output_schema(&["context", "show"], &from_env);

    let from_flag = fixture.run(&["context", "show", "--store", "other.sqlite3"]).success();
    assert_eq!(from_flag["store"]["source"], "flag");
    // A relative --store is relative to the working directory, which the OS may report through
    // a symlink-free path (macOS's /var is /private/var), so compare canonical directories.
    let path = std::path::PathBuf::from(from_flag["store"]["path"].as_str().unwrap());
    assert!(path.is_absolute());
    assert_eq!(path.file_name().unwrap(), "other.sqlite3");
    let directory = path.parent().unwrap().canonicalize().unwrap();
    assert_eq!(directory, fixture.root().canonicalize().unwrap());

    fixture.set_env("MCP_STORE", None);
    let default = fixture.run(&["context", "show"]).success();
    assert_eq!(default["store"]["source"], "default");
    let expected = fixture.path("data").join(mcp::NAME).join("store.sqlite3");
    assert_eq!(default["store"]["path"], expected.display().to_string());
    assert!(!fixture.path("data").exists(), "context show created the data directory");
}
