//! Release helpers for mcp, run as `cargo xtask <operation>`:
//!
//! - `package [<target>]` builds the release binary for a Rust target triple (default: this
//!   machine's) and writes `artifacts/mcp-<target>.tar.gz`: one top-level `mcp-<target>/`
//!   directory holding the executable. Mise's `github:` backend and Homebrew both unpack that
//!   layout without options.
//! - `formula [--artifacts <dir>] [--output <file>]` writes the Homebrew formula for the version
//!   in `crates/mcp/Cargo.toml`, pointing at the release's macOS and Linux archives in
//!   `<dir>` (default `artifacts/`) with their SHA-256 digests. Default output:
//!   `artifacts/mcp.rb`.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The shipped package and executable.
const PACKAGE: &str = "mcp";

/// The targets the Homebrew formula covers, with Homebrew's names for their OS and CPU.
const FORMULA_TARGETS: &[(&str, &str, &str)] = &[
    ("aarch64-apple-darwin", "on_macos", "on_arm"),
    ("x86_64-apple-darwin", "on_macos", "on_intel"),
    ("aarch64-unknown-linux-musl", "on_linux", "on_arm"),
    ("x86_64-unknown-linux-musl", "on_linux", "on_intel"),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("package") => match &args[1..] {
            [] => host_target().and_then(|target| package(&target)),
            [target] => package(target),
            _ => Err(usage()),
        },
        Some("formula") => formula(&args[1..]),
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
    "usage: cargo xtask package [<target>] | cargo xtask formula [--artifacts <dir>] [--output <file>]"
        .to_string()
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

/// What `cargo metadata` says about the workspace and the shipped package.
struct Metadata {
    workspace_root: PathBuf,
    target_directory: PathBuf,
    version: String,
    description: String,
    /// The GitHub repository URL, which is also the formula's homepage and release host.
    repository: String,
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
    let package = metadata["packages"]
        .as_array()
        .and_then(|packages| packages.iter().find(|package| package["name"] == PACKAGE))
        .ok_or_else(|| format!("the workspace has no package `{PACKAGE}`"))?;
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    Ok(Metadata {
        workspace_root: PathBuf::from(text(&metadata["workspace_root"])),
        target_directory: PathBuf::from(text(&metadata["target_directory"])),
        version: text(&package["version"]),
        description: text(&package["description"]),
        repository: text(&package["repository"]),
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

fn formula(args: &[String]) -> Result<(), String> {
    let metadata = metadata()?;
    let mut artifacts = metadata.workspace_root.join("artifacts");
    let mut output = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = rest.next().ok_or_else(usage)?;
        match arg.as_str() {
            "--artifacts" => artifacts = PathBuf::from(value),
            "--output" => output = Some(PathBuf::from(value)),
            _ => return Err(usage()),
        }
    }
    let output = output.unwrap_or_else(|| artifacts.join(format!("{PACKAGE}.rb")));
    let mut digests = Vec::new();
    for (target, _, _) in FORMULA_TARGETS {
        let path = artifacts.join(archive_name(target));
        let contents =
            fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        digests.push(sha256(&contents));
    }
    let rendered =
        render_formula(&metadata.version, &metadata.description, &metadata.repository, &digests);
    fs::write(&output, rendered)
        .map_err(|error| format!("cannot write {}: {error}", output.display()))?;
    println!("{}", output.display());
    Ok(())
}

fn sha256(contents: &[u8]) -> String {
    Sha256::digest(contents).iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// The Homebrew class name for a formula: `my-tool` is `MyTool`.
fn class_name(formula: &str) -> String {
    formula
        .split(['-', '_'])
        .map(|word| {
            let mut characters = word.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect::<String>()
            })
        })
        .collect()
}

fn ruby_string(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"").replace("#{", "\\#{");
    format!("\"{escaped}\"")
}

/// The formula for `version`, with one SHA-256 digest per [`FORMULA_TARGETS`] entry, in order.
/// Release tags are `v<version>`, as Knope creates them.
fn render_formula(
    version: &str,
    description: &str,
    repository: &str,
    digests: &[String],
) -> String {
    let mut formula = format!(
        "# Generated by `cargo xtask formula` in {repository}; do not edit by hand.\n\
         class {} < Formula\n  desc {}\n  homepage {}\n  version {}\n",
        class_name(PACKAGE),
        ruby_string(description),
        ruby_string(repository),
        ruby_string(version),
    );
    for os in ["on_macos", "on_linux"] {
        let _ = write!(formula, "\n  {os} do\n");
        for ((target, _, cpu), digest) in
            FORMULA_TARGETS.iter().zip(digests).filter(|((_, target_os, _), _)| *target_os == os)
        {
            let url = format!("{repository}/releases/download/v{version}/{}", archive_name(target));
            let _ = write!(
                formula,
                "    {cpu} do\n      url {}\n      sha256 {}\n    end\n",
                ruby_string(&url),
                ruby_string(digest)
            );
        }
        formula.push_str("  end\n");
    }
    let _ = write!(
        formula,
        "\n  def install\n    bin.install {package}\n  end\n\n  test do\n    \
         assert_match version.to_s, shell_output(\"#{{bin}}/{PACKAGE} --version\")\n  end\nend\n",
        package = ruby_string(PACKAGE),
    );
    formula
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use flate2::read::GzDecoder;

    use super::*;

    #[test]
    fn class_names_follow_homebrew() {
        assert_eq!(class_name("my-tool"), "MyTool");
        assert_eq!(class_name("tool"), "Tool");
        assert_eq!(class_name("a_b-c"), "ABC");
    }

    #[test]
    fn archive_names_carry_the_target_triple() {
        assert_eq!(
            archive_name("aarch64-apple-darwin"),
            format!("{PACKAGE}-aarch64-apple-darwin.tar.gz")
        );
    }

    #[test]
    fn the_formula_points_each_platform_at_its_archive() {
        let digests: Vec<String> = (0..4).map(|index| format!("{index}").repeat(64)).collect();
        let formula =
            render_formula("1.2.3", "Calls \"MCP\" tools", "https://github.com/o/r", &digests);
        assert!(formula.contains(&format!("class {} < Formula", class_name(PACKAGE))));
        assert!(formula.contains(r#"desc "Calls \"MCP\" tools""#));
        let arm_mac = format!(
            "https://github.com/o/r/releases/download/v1.2.3/{PACKAGE}-aarch64-apple-darwin.tar.gz"
        );
        let mac = formula.find("on_macos do").unwrap();
        let linux = formula.find("on_linux do").unwrap();
        let arm = formula.find(&arm_mac).unwrap();
        assert!(mac < arm && arm < linux);
        assert!(formula.contains(&"3".repeat(64)));
        assert!(formula[linux..].contains("x86_64-unknown-linux-musl"));
        assert!(formula.contains(&format!("#{{bin}}/{PACKAGE} --version")));
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

    #[test]
    fn digests_are_lowercase_hex_sha256() {
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
