use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

pub fn snapshot(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, String>) -> Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            ensure!(
                !kind.is_symlink(),
                "unexpected link {}; use a fresh release stage",
                entry.path().display()
            );
            if kind.is_dir() {
                visit(root, &entry.path(), files)?;
            } else {
                ensure!(
                    kind.is_file(),
                    "unexpected file type at {}",
                    entry.path().display()
                );
                files.insert(
                    entry.path().strip_prefix(root)?.to_path_buf(),
                    crate::digest::sha256(&fs::read(entry.path())?),
                );
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

pub fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for path in snapshot(source)?.keys() {
        let destination = target.join(path);
        fs::create_dir_all(
            destination
                .parent()
                .context("copy destination needs a parent")?,
        )?;
        fs::copy(source.join(path), destination)?;
    }
    Ok(())
}

pub fn json(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}

pub fn run(binary: &Path, cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("execute {}", binary.display()))?;
    ensure!(
        output.status.success(),
        "{} {args:?} failed: {}\n{}",
        binary.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}

pub fn path_text(path: &Path) -> Result<String> {
    let path = fs::canonicalize(path)?;
    let text = path
        .to_str()
        .context("release path must be UTF-8")?
        .replace('\\', "/");
    Ok(text.strip_prefix("//?/").unwrap_or(&text).to_owned())
}

pub fn temp_root(label: &str) -> Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "contextmink {label} {} {}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root)?;
    Ok(root)
}

pub fn cleanup_temp(root: &Path) -> Result<()> {
    ensure!(
        root.parent() == Some(std::env::temp_dir().as_path())
            && root
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("contextmink ")),
        "refuse cleanup outside this tool's temporary directory"
    );
    fs::remove_dir_all(root).with_context(|| {
        format!(
            "remove temporary release smoke directory {}",
            root.display()
        )
    })
}
