//! `mcp call`: flags, input, results, failures, and sessions.

use std::time::{Duration, Instant};

use serde_json::json;

use crate::support::{Fixture, HttpServer, assert_matches_output_schema, stdio_server};

fn arguments(result: &serde_json::Value) -> serde_json::Value {
    result["result"]["structuredContent"]["arguments"].clone()
}

#[test]
fn flags_become_the_tool_s_input_by_property_type() {
    let fixture = Fixture::seeded();
    let result = fixture
        .run(&[
            "call",
            "demo",
            "echo",
            "--text",
            "42",
            "--count",
            "3",
            "--ratio=0.5",
            "--loud",
            "--tags",
            "a",
            "--tags",
            "b",
            "--options",
            r#"{"k":1}"#,
            "--mode",
            "fancy",
            "--file-path",
            "x.txt",
        ])
        .success();
    assert_eq!(
        arguments(&result),
        json!({"text": "42", "count": 3, "ratio": 0.5, "loud": true, "tags": ["a", "b"],
               "options": {"k": 1}, "mode": "fancy", "filePath": "x.txt"})
    );
    assert_eq!(result["status"], "completed");
    assert_eq!(result["inputValidation"]["status"], "passed");
    assert_eq!(result["result"]["isError"], false);
    assert_matches_output_schema(&["call"], &result);
}

#[test]
fn words_after_the_tool_belong_to_the_tool_even_when_they_name_mcp_options() {
    let fixture = Fixture::seeded();
    let result = fixture
        .run(&[
            "call",
            "--timeout",
            "30",
            "demo",
            "collide",
            "--output",
            "o",
            "--input",
            "i",
            "--file_path",
            "a",
            "--filePath",
            "b",
        ])
        .success();
    assert_eq!(
        arguments(&result),
        json!({"output": "o", "input": "i", "file_path": "a", "filePath": "b"})
    );
    let error = fixture
        .run(&["call", "demo", "echo", "--text", "x", "--output", "text"])
        .assert_failed(2, "INVALID_ARGUMENT");
    assert!(error["suggestions"].to_string().contains("before the server"), "{error}");
}

#[test]
fn input_comes_from_json_a_file_or_stdin_with_flags_over_it() {
    let fixture = Fixture::seeded();
    let inline = fixture
        .run(&["call", "--input", r#"{"text":"a","count":1}"#, "demo", "echo", "--count", "2"])
        .success();
    assert_eq!(arguments(&inline), json!({"text": "a", "count": 2}));
    std::fs::write(fixture.project().join("input.json"), r#"{"text":"from file"}"#).unwrap();
    let file = fixture.run(&["call", "--input", "@input.json", "demo", "echo"]).success();
    assert_eq!(arguments(&file)["text"], "from file");
    let piped = fixture
        .run_with_stdin(&["call", "--input", "-", "demo", "echo"], r#"{"text":"piped"}"#)
        .success();
    assert_eq!(arguments(&piped)["text"], "piped");
    fixture.run(&["call", "--input", "[1]", "demo", "echo"]).assert_failed(2, "INVALID_ARGUMENT");
}

#[test]
fn input_that_does_not_fit_the_schema_fails_before_the_tool_runs() {
    let fixture = Fixture::seeded();
    let error =
        fixture.run(&["call", "demo", "echo", "--count", "2"]).assert_failed(2, "INVALID_ARGUMENT");
    assert_eq!(error["details"]["missing"], json!(["--text"]));
    assert!(error["suggestions"].to_string().contains("tools info demo echo"), "{error}");
    let error = fixture
        .run(&["call", "demo", "echo", "--text", "x", "--mode", "loud"])
        .assert_failed(2, "INVALID_ARGUMENT");
    assert_eq!(error["details"]["violations"][0]["flag"], "--mode");
    fixture.run(&["call", "demo", "echo", "--nope", "1"]).assert_failed(2, "INVALID_ARGUMENT");
    fixture.run(&["call", "demo", "echo", "--count", "three"]).assert_failed(2, "INVALID_ARGUMENT");
}

#[test]
fn dry_run_checks_and_shows_the_input_without_calling_the_tool() {
    let fixture = Fixture::seeded();
    let preview = fixture.run(&["call", "--dry-run", "demo", "counter"]).success();
    assert_eq!(preview["status"], "dry-run");
    assert_eq!(preview["arguments"], json!({}));
    assert!(preview["result"].is_null());
    assert!(!preview["caveats"].as_array().unwrap().is_empty());
    assert_matches_output_schema(&["call"], &preview);
    let called = fixture.run(&["call", "demo", "counter"]).success();
    assert_eq!(called["result"]["structuredContent"]["count"], 1, "the dry run called the tool");
}

#[test]
fn a_session_and_its_state_survive_between_invocations() {
    let fixture = Fixture::seeded();
    let first = fixture.run(&["call", "demo", "counter"]).success();
    let second = fixture.run(&["call", "demo", "counter"]).success();
    assert_eq!(first["session"]["reused"], false);
    assert_eq!(second["session"]["reused"], true);
    assert_eq!(second["result"]["structuredContent"]["count"], 2);
    assert_eq!(
        first["result"]["structuredContent"]["pid"],
        second["result"]["structuredContent"]["pid"]
    );
}

#[test]
fn help_after_the_tool_prints_its_flags_as_text() {
    let fixture = Fixture::seeded();
    let outcome = fixture.run(&["call", "demo", "echo", "--help"]);
    assert_eq!(outcome.status, 0, "{}", outcome.stderr);
    assert!(outcome.stdout.starts_with(&format!("{} call demo echo --text <text>", mcp::NAME)));
    assert!(outcome.stdout.contains("--loud <true|false>"), "{}", outcome.stdout);
}

#[test]
fn text_output_prints_the_text_content() {
    let fixture = Fixture::seeded();
    let outcome = fixture.run(&["call", "--output", "text", "demo", "echo", "--text", "hi"]);
    assert_eq!(outcome.stdout, "{\"text\":\"hi\"}\n");
}

#[test]
fn a_tool_error_fails_with_its_whole_result() {
    let fixture = Fixture::seeded();
    let error = fixture
        .run(&["call", "demo", "fail", "--reason", "disk full"])
        .assert_failed(6, "TOOL_ERROR");
    assert_eq!(error["details"]["result"]["isError"], true);
    assert_eq!(error["details"]["result"]["content"][0]["text"], "failed: disk full");
    assert!(error["message"].as_str().unwrap().contains("failed: disk full"));
}

#[test]
fn protocol_errors_keep_their_code_and_data() {
    let fixture = Fixture::seeded();
    let error = fixture.run(&["call", "demo", "protocol_error"]).assert_failed(6, "SERVER_ERROR");
    assert_eq!(error["details"]["code"], -32603);
    assert_eq!(error["details"]["data"], json!({"hint": "test"}));
    fixture.run(&["call", "demo", "invalid_params"]).assert_failed(2, "INVALID_ARGUMENT");
    fixture.run(&["call", "demo", "nope"]).assert_failed(3, "NOT_FOUND");
}

#[test]
fn a_schema_mcp_cannot_compile_is_sent_unchecked_and_says_so() {
    let fixture = Fixture::seeded();
    let result = fixture.run(&["call", "demo", "broken_schema", "--x", "1"]).success();
    assert_eq!(result["inputValidation"]["status"], "skipped");
    assert!(result["inputValidation"]["reason"].is_string());
}

#[test]
fn a_slow_tool_times_out_and_the_session_keeps_working() {
    let fixture = Fixture::seeded();
    let started = Instant::now();
    let error = fixture
        .run(&["call", "--timeout", "1", "demo", "slow", "--seconds", "30"])
        .assert_failed(5, "TIMEOUT");
    assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
    assert_eq!(error["retryable"], false);
    assert!(error["suggestions"].to_string().contains("verify its effect"), "{error}");
    fixture.run(&["call", "demo", "counter"]).success();
}

#[test]
fn a_stdio_server_starts_in_the_workspace_with_the_invocation_s_environment_and_its_env() {
    let mut fixture = Fixture::new();
    let mut server = stdio_server(&[]);
    server["env"] = json!({ "CONFIGURED": "from config" });
    fixture.write_project(&json!({ "mcpServers": { "demo": server } }));
    fixture.set_env("INHERITED", Some("from caller"));
    let value = |name: &str| {
        fixture.run(&["call", "demo", "environment", "--name", name]).success()["result"]["structuredContent"]["value"]
            .clone()
    };
    assert_eq!(value("CONFIGURED"), "from config");
    assert_eq!(value("INHERITED"), "from caller");
}

#[test]
fn http_servers_get_their_headers_and_a_refusal_is_unauthenticated() {
    let server = HttpServer::start(&["--require-header", "Authorization=Bearer sesame"]);
    let mut fixture = Fixture::new();
    fixture.write_user(&json!({ "mcpServers": {
        "remote": { "type": "http", "url": server.url, "headers": { "Authorization": "Bearer ${TOKEN}" } },
    }}));
    fixture.set_env("TOKEN", Some("wrong"));
    let error =
        fixture.run(&["call", "remote", "echo", "--text", "x"]).assert_failed(3, "UNAUTHENTICATED");
    assert!(error["message"].as_str().unwrap().contains("OAuth"), "{error}");
    fixture.run(&["servers", "disconnect", "--all"]).success();

    fixture.set_env("TOKEN", Some("sesame"));
    let first = fixture.run(&["call", "remote", "counter"]).success();
    let second = fixture.run(&["call", "remote", "counter"]).success();
    assert_eq!(second["session"]["reused"], true);
    assert_eq!(
        second["result"]["structuredContent"]["count"],
        first["result"]["structuredContent"]["count"].as_i64().unwrap() + 1
    );
}

#[test]
fn an_unreachable_http_server_is_unavailable() {
    let fixture = Fixture::new();
    fixture
        .write_project(&json!({ "mcpServers": { "gone": { "url": "http://127.0.0.1:9/mcp" } } }));
    let error =
        fixture.run(&["call", "--timeout", "10", "gone", "echo"]).assert_failed(5, "UNAVAILABLE");
    assert_eq!(error["retryable"], true);
}
