//! The shipped CLI reference is the registry, rendered. `mise run cli:reference` regenerates it.

use std::path::Path;

#[test]
fn the_shipped_cli_reference_matches_the_registry() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/cli.md");
    let generated = mcp::reference::render();
    if std::env::var_os("MCP_UPDATE_REFERENCE").is_some() {
        std::fs::write(&path, &generated).expect("docs/cli.md is writable");
        return;
    }
    let shipped = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert!(
        shipped == generated,
        "docs/cli.md is out of date with the command registry; run `mise run cli:reference`"
    );
}
