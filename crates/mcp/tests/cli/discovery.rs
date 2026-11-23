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
    assert!(!outcome.stdout.contains("--timeout"));
}

#[test]
fn version_names_the_release() {
    let outcome = Fixture::new().run(&["--version"]);
    assert_eq!(outcome.status, 0);
    assert_eq!(outcome.stdout.trim(), format!("{} {}", mcp::NAME, env!("CARGO_PKG_VERSION")));
}

#[test]
fn cli_search_returns_a_bounded_ranked_page_with_exact_commands() {
    let result = Fixture::new().run(&["cli", "search", "approve", "a", "server"]).success();
    let items = result["items"].as_array().unwrap();
    assert!(!items.is_empty() && items.len() <= 5);
    assert_eq!(items[0]["command"], format!("{} servers approve", mcp::NAME));
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
        let mut args = vec!["cli", "search", "server", "tool", "--limit", "2"];
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
    for search in [&["cli", "search", "server"][..], &["docs", "search", "server"][..]] {
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
    assert!(commands.contains(&"call") && commands.contains(&"servers.approve"));

    let entry = fixture.run(&["schema", "call"]).success();
    assert_eq!(entry["id"], "call");
    assert_eq!(entry["dryRun"], true);
    assert_eq!(entry["effect"]["kind"], "calls");
    let arguments = entry["arguments"].as_array().unwrap();
    let server = arguments.iter().find(|argument| argument["name"] == "server").unwrap();
    assert_eq!(server["required"], true);
    let trailing = arguments.iter().find(|argument| argument["name"] == "arguments").unwrap();
    assert_eq!(trailing["kind"], "trailing");
    assert!(entry["output"].is_object());
    assert!(entry["errors"].as_array().unwrap().iter().any(|error| error["code"] == "TOOL_ERROR"));
    assert_eq!(fixture.run(&["schema", "tools.info"]).success()["id"], "tools.info");
}

#[test]
fn an_unknown_command_in_schema_is_not_found_with_suggestions() {
    let outcome = Fixture::new().run(&["schema", "tools", "rename"]);
    let error = outcome.assert_failed(3, "NOT_FOUND");
    assert!(error["suggestions"].to_string().contains("schema tools"), "{error}");
}

#[test]
fn missing_input_fails_at_once_naming_the_missing_argument() {
    let outcome = Fixture::new().run(&["tools", "info", "demo"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    assert_eq!(error["retryable"], false);
    assert!(error["details"]["missing"].to_string().contains("TOOL"), "{error}");
    assert!(error["details"]["arguments"].is_array());
    assert!(error["suggestions"].to_string().contains("schema tools info"));
}

#[test]
fn an_unaccepted_value_lists_what_is_accepted() {
    let outcome = Fixture::new().run(&["context", "show", "--output", "yaml"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    assert!(error["details"]["acceptedValues"].to_string().contains("text"), "{error}");
}

#[test]
fn an_out_of_range_limit_is_an_invalid_argument() {
    let outcome = Fixture::new().run(&["servers", "list", "--limit", "500"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    assert_eq!(error["details"]["value"], "500");
}

#[test]
fn a_resource_without_an_operation_lists_its_operations() {
    let outcome = Fixture::new().run(&["servers"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    assert!(error["details"]["commands"].to_string().contains("servers approve"), "{error}");
}

#[test]
fn an_unknown_command_points_at_search() {
    let outcome = Fixture::new().run(&["frobnicate"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    assert!(error["suggestions"].to_string().contains("cli search"), "{error}");
}

#[test]
fn a_positional_passed_as_a_flag_gets_the_right_form() {
    let outcome = Fixture::new().run(&["servers", "info", "--name", "demo"]);
    let error = outcome.assert_failed(2, "INVALID_ARGUMENT");
    let first = error["suggestions"][0].as_str().unwrap();
    assert_eq!(first, format!("Pass the name without a flag: {} servers info <name>", mcp::NAME));
}

#[test]
fn errors_list_their_fields_in_contract_order() {
    let outcome = Fixture::new().run(&["servers", "info", "nothing"]);
    let keys = ["\"code\"", "\"message\"", "\"retryable\"", "\"details\"", "\"suggestions\""];
    let positions: Vec<usize> = keys.iter().map(|key| outcome.stderr.find(key).unwrap()).collect();
    assert!(positions.is_sorted(), "{}", outcome.stderr);
}

#[test]
fn text_output_renders_for_people_but_errors_stay_json() {
    let fixture = Fixture::new();
    let outcome = fixture.run(&["schema", "tools", "info", "--output", "text"]);
    assert_eq!(outcome.status, 0);
    assert!(outcome.stdout.starts_with(&format!("{} tools info", mcp::NAME)));
    assert!(serde_json::from_str::<serde_json::Value>(&outcome.stdout).is_err());

    fixture.run(&["schema", "no", "such", "--output", "text"]).assert_failed(3, "NOT_FOUND");
    fixture.run(&["servers", "info", "--output", "text"]).assert_failed(2, "INVALID_ARGUMENT");
}

#[test]
fn docs_search_returns_sections_to_read() {
    let fixture = Fixture::new();
    let result = fixture.run(&["docs", "search", "flags", "kebab-case"]).success();
    let first = &result["items"][0];
    assert_eq!(first["path"], "docs/tools.md");
    assert_eq!(first["section"], "Flags");
    assert!(first["excerpt"].as_str().unwrap().contains("kebab-case"));
    assert_matches_output_schema(&["docs", "search"], &result);

    let document = fixture.run(&["docs", "show", "docs/approval.md"]).success();
    assert!(document["markdown"].as_str().unwrap().contains("## Turning it on"));
    let text = fixture.run(&["docs", "show", "docs/approval.md", "--output", "text"]);
    assert!(text.stdout.starts_with("# Approval"));
}

#[test]
fn docs_show_of_an_unknown_path_lists_the_documents() {
    let outcome = Fixture::new().run(&["docs", "show", "docs/nothing.md"]);
    let error = outcome.assert_failed(3, "NOT_FOUND");
    assert!(error["details"]["documents"].to_string().contains("docs/sessions.md"), "{error}");
}

#[test]
fn context_show_reports_files_workspace_approval_and_daemon_without_starting_anything() {
    let fixture = Fixture::seeded();
    let before = fixture.snapshot();
    let context = fixture.run(&["context", "show"]).success();
    assert_eq!(context["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(context["workspace"]["path"], fixture.project().display().to_string());
    assert_eq!(context["workspace"]["reason"], "project-file");
    let files = context["files"].as_array().unwrap();
    assert_eq!(files[0]["source"], "user");
    assert_eq!(files[0]["path"], fixture.path("config/.mcp.json").display().to_string());
    assert_eq!(files[0]["exists"], false);
    assert_eq!(files[1]["source"], "project");
    assert_eq!(files[1]["servers"], 1);
    assert_eq!(context["approval"]["required"], false);
    assert_eq!(context["approval"]["file"], fixture.approvals_file().display().to_string());
    assert_eq!(context["daemon"]["state"], "stopped");
    assert_matches_output_schema(&["context", "show"], &context);
    assert_eq!(fixture.snapshot(), before, "context show changed the fixture");
    assert!(!fixture.path("run/mcp").exists(), "context show started the daemon");
}

#[test]
fn context_show_reports_an_invalid_file_and_approval_setting_instead_of_failing() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.project().join(".mcp.json"), "{").unwrap();
    fixture.set_env("MCP_REQUIRE_APPROVAL", Some("yes"));
    let context = fixture.run(&["context", "show"]).success();
    assert!(context["files"][1]["problem"].as_str().unwrap().contains("invalid JSON"));
    assert!(context["approval"]["problem"].as_str().unwrap().contains("MCP_REQUIRE_APPROVAL"));
}
