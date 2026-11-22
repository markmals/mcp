//! Which servers `mcp` reads, from which file, and when it refuses them.

use serde_json::json;

use crate::support::{Fixture, stdio_server};

#[test]
fn a_project_server_replaces_the_user_server_of_the_same_name() {
    let fixture = Fixture::new();
    fixture.write_user(&json!({ "mcpServers": {
        "shared": { "command": "user-command" },
        "personal": stdio_server(&[]),
    }}));
    fixture.write_project(&json!({ "mcpServers": { "shared": stdio_server(&[]) } }));
    let list = fixture.run(&["servers", "list"]).success();
    let items = list["items"].as_array().unwrap();
    let names: Vec<&str> = items.iter().map(|item| item["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["personal", "shared"]);
    assert_eq!(items[0]["source"], "user");
    assert_eq!(items[1]["source"], "project");
    let info = fixture.run(&["servers", "info", "shared"]).success();
    assert_eq!(info["replaces"], fixture.path("config/.mcp.json").display().to_string());
}

#[test]
fn user_servers_apply_in_every_directory() {
    let mut fixture = Fixture::new();
    fixture.write_user(&json!({ "mcpServers": { "personal": stdio_server(&[]) } }));
    let elsewhere = fixture.path("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    fixture.run_in(elsewhere.clone());
    let context = fixture.run(&["context", "show"]).success();
    assert_eq!(context["workspace"]["reason"], "working-directory");
    assert_eq!(context["workspace"]["path"], elsewhere.display().to_string());
    let result = fixture.run(&["call", "personal", "echo", "--text", "hi"]).success();
    assert_eq!(result["result"]["structuredContent"]["arguments"]["text"], "hi");
}

#[test]
fn the_nearest_project_file_applies_from_a_subdirectory() {
    let mut fixture = Fixture::seeded();
    let nested = fixture.project().join("src/deep");
    std::fs::create_dir_all(&nested).unwrap();
    fixture.run_in(nested);
    let context = fixture.run(&["context", "show"]).success();
    assert_eq!(context["workspace"]["path"], fixture.project().display().to_string());
    let environment = fixture.run(&["call", "demo", "environment", "--name", "PWD"]).success();
    let cwd = environment["result"]["structuredContent"]["cwd"].as_str().unwrap();
    assert_eq!(std::path::Path::new(cwd).canonicalize().unwrap(), fixture.project());
}

#[test]
fn an_invalid_file_blocks_every_command_that_resolves_servers() {
    let fixture = Fixture::new();
    fixture.write_user(&json!({ "mcpServers": { "demo": stdio_server(&[]) } }));
    std::fs::write(fixture.project().join(".mcp.json"), r#"{"mcpServers": {}, "extra": 1}"#)
        .unwrap();
    for args in [&["servers", "list"][..], &["tools", "list"], &["call", "demo", "echo"]] {
        let error = fixture.run(args).assert_failed(3, "PRECONDITION_FAILED");
        assert_eq!(
            error["details"]["file"],
            fixture.project().join(".mcp.json").display().to_string()
        );
        assert!(error["message"].as_str().unwrap().contains("`extra`"), "{error}");
    }
}

#[test]
fn a_field_mcp_does_not_implement_makes_only_that_server_unusable() {
    let fixture = Fixture::new();
    fixture.write_project(&json!({ "mcpServers": {
        "demo": stdio_server(&[]),
        "odd": { "command": "x", "cwd": "/tmp" },
        "legacy": { "type": "sse", "url": "http://127.0.0.1:1/sse" },
    }}));
    let list = fixture.run(&["servers", "list"]).success();
    let odd = list["items"].as_array().unwrap().iter().find(|item| item["name"] == "odd").unwrap();
    assert_eq!(odd["usable"], false);
    assert!(odd["problem"].as_str().unwrap().contains("`cwd`"));
    let error = fixture.run(&["call", "odd", "echo"]).assert_failed(3, "PRECONDITION_FAILED");
    assert_eq!(error["details"]["field"], "cwd");
    let error = fixture.run(&["call", "legacy", "echo"]).assert_failed(3, "PRECONDITION_FAILED");
    assert_eq!(error["details"]["field"], "type");
    fixture.run(&["call", "demo", "echo", "--text", "x"]).success();
}

#[test]
fn variables_expand_from_the_invocation_and_an_unset_one_is_named() {
    let mut fixture = Fixture::new();
    let mut server = stdio_server(&[]);
    server["env"] = json!({ "GREETING": "${GREETING:-hello}", "TARGET": "${TARGET}" });
    fixture.write_project(&json!({ "mcpServers": { "demo": server } }));
    let error = fixture.run(&["call", "demo", "echo"]).assert_failed(3, "PRECONDITION_FAILED");
    assert_eq!(error["details"]["variable"], "TARGET");

    fixture.set_env("TARGET", Some("world"));
    let environment = |name: &str| {
        fixture.run(&["call", "demo", "environment", "--name", name]).success()["result"]["structuredContent"]
            ["value"]
            .clone()
    };
    assert_eq!(environment("GREETING"), "hello");
    assert_eq!(environment("TARGET"), "world");
}
