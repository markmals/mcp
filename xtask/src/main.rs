//! Release helpers for mcp, run as `cargo xtask <operation>`:
//!
//! - `package [<target>]` builds the release binary for a Rust target triple (default: this
//!   machine's) and writes `artifacts/mcp-<target>.tar.gz`: one top-level `mcp-<target>/`
//!   directory holding the executable, which Mise's `github:` backend unpacks without options.
//!
//! Homebrew does not use these archives: `markmals/homebrew-tap` builds `mcp` from the release
//! tag and bottles it.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::Value;

/// The shipped package and executable.
const PACKAGE: &str = "mcp";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("package") => match &args[1..] {
            [] => host_target().and_then(|target| package(&target)),
            [target] => package(target),
            _ => Err(usage()),
        },
        _ => Err(usage()),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> String {
    "usage: cargo xtask package [<target>]".to_string()
}

/// The target triple `rustc` builds for by default on this machine.
fn host_target() -> Result<String, String> {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let output = Command::new(rustc)
        .arg("-vV")
        .output()
        .map_err(|error| format!("cannot run rustc -vV: {error}"))?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_string)
        .ok_or_else(|| "rustc -vV did not name the host target".to_string())
}

/// What `cargo metadata` says about the workspace.
struct Metadata {
    workspace_root: PathBuf,
    target_directory: PathBuf,
}

fn metadata() -> Result<Metadata, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cargo metadata printed invalid JSON: {error}"))?;
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    Ok(Metadata {
        workspace_root: PathBuf::from(text(&metadata["workspace_root"])),
        target_directory: PathBuf::from(text(&metadata["target_directory"])),
    })
}

fn package(target: &str) -> Result<(), String> {
    let metadata = metadata()?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--release", "--locked", "--package", PACKAGE, "--target", target])
        .current_dir(&metadata.workspace_root)
        .status()
        .map_err(|error| format!("cannot run cargo build: {error}"))?;
    if !status.success() {
        return Err(format!("cargo build for {target} failed"));
    }
    let binary = metadata.target_directory.join(target).join("release").join(PACKAGE);
    let artifacts = metadata.workspace_root.join("artifacts");
    fs::create_dir_all(&artifacts)
        .map_err(|error| format!("cannot create {}: {error}", artifacts.display()))?;
    let archive = artifacts.join(archive_name(target));
    write_archive(&binary, target, &archive)
        .map_err(|error| format!("cannot write {}: {error}", archive.display()))?;
    println!("{}", archive.display());
    Ok(())
}

/// `mcp-<target>.tar.gz`: the Rust target triple in the name is what Mise's `github:` backend
/// matches against the installing machine's OS, CPU, and C library.
fn archive_name(target: &str) -> String {
    format!("{PACKAGE}-{target}.tar.gz")
}

/// Writes `binary` into `archive` as `mcp-<target>/mcp`, mode 0755.
fn write_archive(binary: &Path, target: &str, archive: &Path) -> io::Result<()> {
    let entry = format!("{PACKAGE}-{target}/{PACKAGE}");
    let contents = fs::read(binary)?;
    let mut builder =
        tar::Builder::new(GzEncoder::new(File::create(archive)?, Compression::best()));
    let mut header = tar::Header::new_gnu();
    header.set_size(u64::try_from(contents.len()).map_err(io::Error::other)?);
    header.set_mode(0o755);
    header.set_mtime(
        fs::metadata(binary)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
    );
    header.set_cksum();
    builder.append_data(&mut header, entry, contents.as_slice())?;
    builder.into_inner()?.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use flate2::read::GzDecoder;

    use super::*;

    #[test]
    fn archive_names_carry_the_target_triple() {
        assert_eq!(
            archive_name("aarch64-apple-darwin"),
            format!("{PACKAGE}-aarch64-apple-darwin.tar.gz")
        );
    }

    #[test]
    fn archives_hold_one_executable_under_a_top_level_directory() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("binary");
        fs::write(&binary, b"executable").unwrap();

        let target = "aarch64-unknown-linux-musl";
        let tarball = directory.path().join(archive_name(target));
        write_archive(&binary, target, &tarball).unwrap();
        let mut archive = tar::Archive::new(GzDecoder::new(File::open(&tarball).unwrap()));
        let mut entries = archive.entries().unwrap();
        let mut entry = entries.next().unwrap().unwrap();
        assert_eq!(entry.path().unwrap(), Path::new(&format!("{PACKAGE}-{target}/{PACKAGE}")));
        assert_eq!(entry.header().mode().unwrap(), 0o755);
        let mut contents = String::new();
        entry.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "executable");
        assert!(entries.next().is_none());
    }
}
