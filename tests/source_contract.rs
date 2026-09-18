use std::fs;
use std::path::{Path, PathBuf};

fn rust_sources(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn ignored_rust_results_require_local_guardrail_justification() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_sources(&root, &mut files);
    files.sort();

    for file in files {
        let source = fs::read_to_string(&file).unwrap();
        for (index, line) in source.lines().enumerate() {
            if line.contains("let _ =") || line.contains(".ok()") {
                assert!(
                    line.contains("guardrail: allow-ignore-result"),
                    "{}:{} ignores a Rust result without a local guardrail justification: {}",
                    file.display(),
                    index + 1,
                    line.trim()
                );
            }
        }
    }
}

#[test]
fn repository_does_not_track_python_scripts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Cargo's source archive has no Git index; its scripts directory is checked
    // directly. In a checkout also catch scripts added outside that directory.
    for entry in fs::read_dir(root.join("scripts")).unwrap() {
        let path = entry.unwrap().path();
        assert!(
            !path.extension().is_some_and(|e| e == "py" || e == "pyw"),
            "Python script remains at {}",
            path.display()
        );
    }
    if root.join(".git").exists() {
        let output = std::process::Command::new("git")
            .args(["ls-files", "-z", "--", "*.py", "*.pyw"])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success());
        for path in output.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
            let path = std::str::from_utf8(path).unwrap();
            assert!(
                !root.join(path).exists(),
                "tracked Python script remains: {path}"
            );
        }
    }
}
