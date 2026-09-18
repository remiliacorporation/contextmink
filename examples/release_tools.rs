//! Development-only release tooling; never installed with the CLI.
#[path = "../src/digest.rs"]
mod digest;
// Release notes use the same encoding scanner as retrieval. Other decoding
// entrypoints remain available to the product, not this executable.
#[allow(dead_code)]
#[path = "../src/encoding.rs"]
mod encoding;
#[path = "release_tools/notes.rs"]
mod notes;
#[path = "release_tools/package.rs"]
mod package;
#[path = "release_tools/support.rs"]
mod support;
#[path = "release_tools/verify.rs"]
mod verify;

use anyhow::{Result, bail};
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, version] if command == "notes" => print!(
            "{}",
            notes::render(version, &std::fs::read_to_string("CHANGELOG.md")?)?
        ),
        [command, version, path] if command == "notes" => print!(
            "{}",
            notes::render(version, &std::fs::read_to_string(path)?)?
        ),
        [command, stage, archive] if command == "package-project" => {
            package::run(Path::new(stage), Path::new(archive))?
        }
        [command, bundle] if command == "verify-project" => verify::project(Path::new(bundle))?,
        [command, binary] if command == "verify-user" => verify::user(Path::new(binary))?,
        _ => bail!(
            "use cargo run --locked --example release_tools -- notes <version> [changelog] | package-project <stage> <archive> | verify-project <extracted-bundle> | verify-user <extracted-binary>"
        ),
    }
    Ok(())
}
