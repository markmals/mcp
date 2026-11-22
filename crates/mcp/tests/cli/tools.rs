//! `tools list`, `search`, and `info`.

use serde_json::json;

use crate::support::{Fixture, assert_matches_output_schema, stdio_server};

const TOOLS: [&str; 10] = [
    "echo",
    "fail",
    "counter",
    "slow",
    "environment",
    "log",
    "collide",
    "broken_schema",
    "protocol_error",
    "invalid_params",
];

#[test]
fn list_pages_through_every_tool_of_every_page_the_server_serves() {
    let fixture = Fixture::seeded();
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut args = vec!["tools", "list", "--limit", "3"];
        if let Some(cursor) = &cursor {
            args.extend(["--cursor", cursor.as_str()]);
        }
        let page = fixture.run(&args).success();
        assert_eq!(page["complete"], true);
        assert_matches_output_schema(&["tools", "list"], &page);
        seen.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["name"].as_str().unwrap().to_string()),
        );
        match page["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => break,
        }
    }
    assert_eq!(seen, TOOLS);
}

#[test]
fn list_shows_one_line_of_each_description() {
    let fixture = Fixture::seeded();
    let page = fixture.run(&["tools", "list"]).success();
    let echo = &page["items"][0];
    assert_eq!(echo["server"], "demo");
    assert_eq!(echo["title"], "Echo");
    assert_eq!(echo["summary"], "Echoes its arguments back.");
    assert_eq!(echo["summaryTruncated"], true);
    assert_eq!(echo["annotations"]["readOnlyHint"], true);
}

#[test]
fn servers_that_cannot_answer_are_reported_without_failing_the_list() {
    let fixture = Fixture::new();
    fixture.write_project(&json!({ "mcpServers": {
        "demo": stdio_server(&[]),
        "broken": stdio_server(&["--fail-start", "no"]),
        "quiet": stdio_server(&["--no-tools"]),
    }}));
    let page = fixture.run(&["tools", "list"]).success();
    assert_eq!(page["complete"], false);
    let unavailable: Vec<(&str, &str)> = page["unavailable"]
        .as_array()
        .unwrap()
        .iter()
        .map(|server| (server["server"].as_str().unwrap(), server["code"].as_str().unwrap()))
        .collect();
    assert_eq!(unavailable, [("broken", "UNAVAILABLE"), ("quiet", "PRECONDITION_FAILED")]);
    assert_eq!(page["items"].as_array().unwrap().len(), TOOLS.len());

    let only = fixture.run(&["tools", "list", "--server", "demo"]).success();
    assert_eq!(only["complete"], true);
    fixture.run(&["tools", "list", "--server", "nope"]).assert_failed(3, "NOT_FOUND");
}

#[test]
fn search_ranks_tools_and_gives_the_command_to_inspect_each() {
    let fixture = Fixture::seeded();
    let matches = fixture.run(&["tools", "search", "count", "calls"]).success();
    assert_eq!(matches["items"][0]["tool"], "counter");
    assert_eq!(matches["items"][0]["command"], format!("{} tools info demo counter", mcp::NAME));
    assert_matches_output_schema(&["tools", "search"], &matches);
    let none = fixture.run(&["tools", "search", "zebra", "xylophone"]).success();
    assert_eq!(none["items"], json!([]));
}

#[test]
fn info_returns_the_tool_as_declared_with_its_flags() {
    let fixture = Fixture::seeded();
    let info = fixture.run(&["tools", "info", "demo", "echo"]).success();
    assert_eq!(info["tool"]["name"], "echo");
    assert_eq!(info["tool"]["inputSchema"]["required"], json!(["text"]));
    let flags = info["flags"].as_array().unwrap();
    let file_path = flags.iter().find(|flag| flag["property"] == "filePath").unwrap();
    assert_eq!(file_path["flag"], "--file-path");
    let tags = flags.iter().find(|flag| flag["property"] == "tags").unwrap();
    assert_eq!((tags["type"].as_str(), tags["itemType"].as_str()), (Some("array"), Some("string")));
    assert_eq!(info["example"], format!("{} call demo echo --text <text>", mcp::NAME));
    assert_matches_output_schema(&["tools", "info"], &info);

    let text = fixture.run(&["tools", "info", "demo", "echo", "--output", "text"]);
    assert!(text.stdout.contains("--file-path <string>"), "{}", text.stdout);
}

#[test]
fn an_unknown_tool_is_not_found_and_lists_the_server_s_tools() {
    let fixture = Fixture::seeded();
    let error = fixture.run(&["tools", "info", "demo", "nope"]).assert_failed(3, "NOT_FOUND");
    assert_eq!(error["details"]["tools"].as_array().unwrap().len(), TOOLS.len());
}
