//! Approval of a project's servers: off by default, bound to the repository and the
//! configuration as written when it is on.

use serde_json::json;

use crate::support::{Fixture, assert_matches_output_schema, stdio_server};

fn requiring() -> Fixture {
    let mut fixture = Fixture::seeded();
    fixture.set_env("MCP_REQUIRE_APPROVAL", Some("1"));
    fixture
}

fn fingerprint(fixture: &Fixture, server: &str) -> String {
    fixture.run(&["servers", "info", server]).success()["fingerprint"].as_str().unwrap().to_string()
}

#[test]
fn without_approval_required_project_servers_run_as_configured() {
    let fixture = Fixture::seeded();
    fixture.run(&["call", "demo", "counter"]).success();
}

#[test]
fn an_unapproved_project_server_is_refused_while_approval_is_required() {
    let fixture = requiring();
    let error = fixture.run(&["call", "demo", "counter"]).assert_failed(3, "PRECONDITION_FAILED");
    assert_eq!(error["details"]["approval"], "unapproved");
    assert_eq!(error["details"]["server"], "demo");
    assert!(error["suggestions"][0].as_str().unwrap().contains("servers approve demo"), "{error}");
    let list = fixture.run(&["servers", "list"]).success();
    assert_eq!(list["items"][0]["usable"], false);
    let tools = fixture.run(&["tools", "list"]).success();
    assert_eq!(tools["unavailable"][0]["code"], "PRECONDITION_FAILED");
    let info = fixture.run(&["servers", "info", "demo"]).success();
    assert!(info["live"].is_null());
    assert!(!fixture.path("run/mcp").exists(), "a refused server reached the daemon");
}

#[test]
fn an_approval_covers_the_configuration_it_was_given_for() {
    let fixture = requiring();
    fixture.approve("demo", &fingerprint(&fixture, "demo"));
    assert_eq!(fixture.run(&["servers", "list"]).success()["items"][0]["approval"], "approved");
    fixture.run(&["call", "demo", "counter"]).success();

    fixture.write_project(
        &json!({ "mcpServers": { "demo": stdio_server(&["--instructions", "changed"]) } }),
    );
    let error = fixture.run(&["call", "demo", "counter"]).assert_failed(3, "PRECONDITION_FAILED");
    assert_eq!(error["details"]["approval"], "changed");
}

#[test]
fn reformatting_the_file_keeps_the_approval() {
    let fixture = requiring();
    fixture.approve("demo", &fingerprint(&fixture, "demo"));
    let path = fixture.project().join(".mcp.json");
    let compact: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    std::fs::write(&path, compact.to_string()).unwrap();
    fixture.run(&["call", "demo", "counter"]).success();
}

#[test]
fn user_servers_never_need_approval() {
    let mut fixture = Fixture::new();
    fixture.write_user(&json!({ "mcpServers": { "mine": stdio_server(&[]) } }));
    fixture.set_env("MCP_REQUIRE_APPROVAL", Some("1"));
    fixture.run(&["call", "mine", "counter"]).success();
    let receipt = fixture.run(&["servers", "approve", "mine"]).success();
    assert_eq!(receipt["items"][0]["outcome"], "not-required");
    assert!(!fixture.approvals_file().exists());
}

#[test]
fn approve_dry_run_lists_what_the_person_would_be_asked_and_changes_nothing() {
    let fixture = requiring();
    let before = fixture.snapshot();
    let receipt = fixture.run(&["servers", "approve", "--all", "--dry-run"]).success();
    assert_eq!(receipt["status"], "dry-run");
    assert_eq!(receipt["items"][0]["outcome"], "approved");
    assert!(receipt["items"][0]["target"].as_str().unwrap().ends_with("mcp-test-server"));
    assert_matches_output_schema(&["servers", "approve"], &receipt);
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn approving_an_approved_server_asks_no_one() {
    let fixture = requiring();
    fixture.approve("demo", &fingerprint(&fixture, "demo"));
    let receipt = fixture.run(&["servers", "approve", "demo"]).success();
    assert_eq!(receipt["status"], "completed");
    assert_eq!(receipt["items"][0]["outcome"], "already-approved");
}

#[test]
fn revoke_removes_the_approval_and_ends_the_session() {
    let fixture = requiring();
    fixture.approve("demo", &fingerprint(&fixture, "demo"));
    fixture.run(&["call", "demo", "counter"]).success();
    let preview = fixture.run(&["servers", "revoke", "demo", "--dry-run"]).success();
    assert_eq!(preview["items"][0]["outcome"], "revoked");
    fixture.run(&["call", "demo", "counter"]).success();

    let receipt = fixture.run(&["servers", "revoke", "demo"]).success();
    assert_eq!(
        receipt["items"][0],
        json!({ "server": "demo", "outcome": "revoked", "disconnected": true })
    );
    assert_matches_output_schema(&["servers", "revoke"], &receipt);
    fixture.run(&["call", "demo", "counter"]).assert_failed(3, "PRECONDITION_FAILED");
    let again = fixture.run(&["servers", "revoke", "demo"]).success();
    assert_eq!(again["items"][0]["outcome"], "not-approved");
}

#[test]
fn worktrees_of_one_clone_share_approvals() {
    let mut fixture = requiring();
    fixture.approve("demo", &fingerprint(&fixture, "demo"));
    let git = fixture.project().join(".git");
    std::fs::create_dir_all(git.join("worktrees/feature")).unwrap();
    std::fs::write(git.join("worktrees/feature/commondir"), "../..\n").unwrap();
    let worktree = fixture.path("feature");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", git.join("worktrees/feature").display()),
    )
    .unwrap();
    std::fs::copy(fixture.project().join(".mcp.json"), worktree.join(".mcp.json")).unwrap();
    fixture.run_in(worktree);
    fixture.run(&["call", "demo", "counter"]).success();
}

#[test]
fn an_invalid_approval_setting_is_a_precondition_failure() {
    let mut fixture = Fixture::seeded();
    fixture.set_env("MCP_REQUIRE_APPROVAL", Some("true"));
    let error = fixture.run(&["call", "demo", "counter"]).assert_failed(3, "PRECONDITION_FAILED");
    assert_eq!(error["details"]["variable"], "MCP_REQUIRE_APPROVAL");
}

#[test]
fn approve_needs_names_or_all() {
    let fixture = requiring();
    fixture.run(&["servers", "approve"]).assert_failed(2, "INVALID_ARGUMENT");
    fixture.run(&["servers", "approve", "demo", "--all"]).assert_failed(2, "INVALID_ARGUMENT");
    fixture.run(&["servers", "approve", "nope"]).assert_failed(3, "NOT_FOUND");
}

/// CI's Linux runners have no polkit authentication agent, so the request cannot be put to a
/// person; it must fail without recording an approval.
#[cfg(target_os = "linux")]
#[test]
fn approve_without_a_way_to_ask_records_nothing() {
    let fixture = requiring();
    let error = fixture
        .run(&["servers", "approve", "demo", "--timeout", "5"])
        .assert_failed(3, "PERMISSION_DENIED");
    assert_eq!(error["details"]["reason"], "cannot-ask");
    assert!(!fixture.approvals_file().exists());
}
