//! `servers list`, `info`, and `disconnect`, and the sessions behind them.

use serde_json::json;

use crate::support::{Fixture, assert_matches_output_schema, stdio_server};

#[test]
fn list_reports_each_server_without_starting_anything() {
    let fixture = Fixture::seeded();
    let list = fixture.run(&["servers", "list"]).success();
    let demo = &list["items"][0];
    assert_eq!(demo["name"], "demo");
    assert_eq!(demo["source"], "project");
    assert_eq!(demo["transport"], "stdio");
    assert_eq!(demo["approval"], "unapproved");
    assert_eq!(demo["usable"], true);
    assert_eq!(demo["connected"], false);
    assert!(demo["target"].as_str().unwrap().ends_with("mcp-test-server"));
    assert_matches_output_schema(&["servers", "list"], &list);
    assert!(!fixture.path("run/mcp").exists(), "servers list started the daemon");
}

#[test]
fn info_connects_and_reports_what_the_server_declared() {
    let fixture = Fixture::new();
    fixture.write_project(&json!({ "mcpServers": {
        "demo": stdio_server(&["--instructions", "Use echo.", "--stderr-lines", "3"]),
    }}));
    let info = fixture.run(&["servers", "info", "demo"]).success();
    assert_eq!(info["live"]["server"]["info"]["name"], "mcp-test-server");
    assert_eq!(info["live"]["server"]["instructions"], "Use echo.");
    assert!(info["live"]["server"]["capabilities"]["tools"].is_object());
    assert_eq!(
        info["live"]["server"]["stderr"],
        json!(["stderr line 1", "stderr line 2", "stderr line 3"])
    );
    assert_eq!(info["live"]["session"]["reused"], false);
    assert!(info["fingerprint"].as_str().unwrap().starts_with("sha256:"));
    assert_matches_output_schema(&["servers", "info"], &info);

    let again = fixture.run(&["servers", "info", "demo"]).success();
    assert_eq!(again["live"]["session"]["reused"], true);
    assert_eq!(again["live"]["session"]["id"], info["live"]["session"]["id"]);
    let list = fixture.run(&["servers", "list"]).success();
    assert_eq!(list["items"][0]["connected"], true);
}

#[test]
fn info_on_a_server_that_cannot_start_succeeds_and_says_why() {
    let fixture = Fixture::new();
    fixture.write_project(&json!({ "mcpServers": {
        "broken": stdio_server(&["--fail-start", "missing database"]),
    }}));
    let info = fixture.run(&["servers", "info", "broken"]).success();
    assert!(info["live"].is_null());
    assert!(info["problem"].as_str().is_some_and(|problem| !problem.is_empty()), "{info}");
    let error = fixture.run(&["tools", "info", "broken", "echo"]).assert_failed(5, "UNAVAILABLE");
    assert_eq!(error["retryable"], true);
    assert!(error["details"]["stderr"].to_string().contains("missing database"), "{error}");
}

#[test]
fn an_unknown_server_is_not_found_and_lists_the_configured_ones() {
    let fixture = Fixture::seeded();
    let error = fixture.run(&["servers", "info", "nope"]).assert_failed(3, "NOT_FOUND");
    assert_eq!(error["details"]["servers"], json!(["demo"]));
}

#[test]
fn disconnect_ends_the_session_so_the_next_command_starts_a_new_one() {
    let fixture = Fixture::seeded();
    let first = fixture.run(&["call", "demo", "counter"]).success();
    assert_eq!(first["result"]["structuredContent"]["count"], 1);

    let preview = fixture.run(&["servers", "disconnect", "--all", "--dry-run"]).success();
    assert_eq!(preview["status"], "dry-run");
    assert_eq!(preview["items"], json!([{ "server": "demo", "outcome": "disconnected" }]));
    let still = fixture.run(&["call", "demo", "counter"]).success();
    assert_eq!(still["result"]["structuredContent"]["count"], 2, "a dry run ended the session");

    let receipt = fixture.run(&["servers", "disconnect", "demo"]).success();
    assert_eq!(receipt["items"][0]["outcome"], "disconnected");
    assert_matches_output_schema(&["servers", "disconnect"], &receipt);
    let fresh = fixture.run(&["call", "demo", "counter"]).success();
    assert_eq!(fresh["result"]["structuredContent"]["count"], 1);
    assert_eq!(fresh["session"]["reused"], false);
}

#[test]
fn disconnect_without_a_daemon_starts_none() {
    let fixture = Fixture::seeded();
    let receipt = fixture.run(&["servers", "disconnect", "demo"]).success();
    assert_eq!(receipt["items"][0]["outcome"], "not-connected");
    assert!(!fixture.path("run/mcp").exists(), "disconnect started the daemon");
    fixture.run(&["servers", "disconnect"]).assert_failed(2, "INVALID_ARGUMENT");
}

#[test]
fn a_changed_configuration_starts_a_new_session() {
    let fixture = Fixture::seeded();
    fixture.run(&["call", "demo", "counter"]).success();
    fixture.write_project(
        &json!({ "mcpServers": { "demo": stdio_server(&["--instructions", "v2"]) } }),
    );
    let after = fixture.run(&["call", "demo", "counter"]).success();
    assert_eq!(after["session"]["reused"], false);
    assert_eq!(after["result"]["structuredContent"]["count"], 1);
}
