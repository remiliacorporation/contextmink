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
