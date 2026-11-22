//! Configured and interpolated secret values never reach `mcp`'s own output.

use serde_json::json;

use crate::support::{Fixture, HttpServer, stdio_server};

const SENTINEL: &str = "SENTINEL-SECRET-4f1c9b";

fn assert_hidden(outcome: &crate::support::Outcome, what: &str) {
    for stream in [&outcome.stdout, &outcome.stderr] {
        assert!(!stream.contains(SENTINEL), "{what} showed a secret:\n{stream}");
    }
}

#[test]
fn secrets_stay_out_of_every_failure_and_inspection() {
    let server = HttpServer::start(&["--require-header", "Authorization=Bearer right"]);
    let mut fixture = Fixture::new();
    let mut interpolated = stdio_server(&["--fail-start", "token=${SECRET}"]);
    interpolated["env"] = json!({ "API_KEY": SENTINEL });
    let mut literal = stdio_server(&["--fail-start", "unused"]);
    literal["env"] = json!({ "API_KEY": SENTINEL });
    fixture.write_project(&json!({ "mcpServers": {
        "interpolated": interpolated,
        "literal": literal,
        "header": { "url": server.url, "headers": { "Authorization": format!("Bearer {SENTINEL}") } },
        "query": { "url": "http://127.0.0.1:9/mcp?key=${SECRET}" },
        "missing": { "command": "/nonexistent/${SECRET}/server" },
        "talkative": stdio_server(&["--instructions", "Use key ${SECRET}"]),
    }}));
    fixture.set_env("SECRET", Some(SENTINEL));

    for server in ["interpolated", "literal", "header", "query", "missing", "talkative"] {
        for args in [
            vec!["call", "--timeout", "10", server, "echo"],
            vec!["tools", "info", "--timeout", "10", server, "echo"],
            vec!["servers", "info", "--timeout", "10", server],
        ] {
            let outcome = fixture.run(&args);
            assert_hidden(&outcome, &args.join(" "));
        }
    }
    for args in
        [vec!["servers", "list"], vec!["tools", "list", "--timeout", "10"], vec!["context", "show"]]
    {
        assert_hidden(&fixture.run(&args), &args.join(" "));
    }
    let failed = fixture.run(&["call", "interpolated", "echo"]);
    assert!(failed.stderr.contains("<redacted>"), "{}", failed.stderr);
    let flagged = fixture.run(&["call", "talkative", "echo", "--text", "ok", "--count", SENTINEL]);
    assert_hidden(&flagged, "a malformed flag value");
    assert_eq!(flagged.status, 2);
}
