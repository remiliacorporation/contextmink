use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;

fn fixture_root(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!("contextmink-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root); // guardrail: allow-ignore-result best-effort cleanup of reused test fixture
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test-profile\"\n",
    )
    .unwrap();
    root
}

fn run_json(root: &Path, args: &[&str]) -> (std::process::ExitStatus, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_contextmink"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "failed to parse contextmink JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status, value)
}

fn has_output_cap(value: &Value, dimension: &str, limit: u64) -> bool {
    value["caps"].as_array().is_some_and(|caps| {
        caps.iter().any(|cap| {
            cap["boundary"] == "output" && cap["dimension"] == dimension && cap["limit"] == limit
        })
    })
}

#[test]
fn json_find_value_cap_is_disclosed_without_limiting_search() {
    let root = fixture_root("json-find-value-cap");
    fs::write(
        root.join("fixture.json"),
        r#"{"long":"prefix-after-render-cap"}"#,
    )
    .unwrap();

    let (status, value) = run_json(
        &root,
        &[
            "--json",
            "json-find",
            "fixture.json",
            "--path-contains",
            "$.long",
            "--value-contains",
            "after-render-cap",
            "--max-value-chars",
            "8",
            "--fail-if-truncated",
        ],
    );

    assert!(!status.success());
    assert_eq!(value["result"]["total"], 1);
    assert!(
        value["matches"][0]["value"]
            .as_str()
            .unwrap()
            .ends_with("...")
    );
    assert!(has_output_cap(&value, "value_characters", 8));
    assert_eq!(value["scope_complete"], true);
    assert_eq!(value["output_truncated"], true);
    assert_eq!(value["complete"], false);
}

#[test]
fn json_select_value_cap_is_disclosed_in_json_payload() {
    let root = fixture_root("json-select-value-cap");
    fs::write(
        root.join("fixture.json"),
        r#"{"rows":[{"message":"abcdefghijklmnopqrstuvwxyz"}]}"#,
    )
    .unwrap();

    let (status, value) = run_json(
        &root,
        &[
            "--json",
            "json-select",
            "fixture.json",
            "--array",
            "rows",
            "--fields",
            "message",
            "--max-value-chars",
            "10",
        ],
    );

    assert!(status.success());
    assert!(
        value["rows"][0]["fields"]["message"]
            .as_str()
            .unwrap()
            .ends_with("...")
    );
    assert!(has_output_cap(&value, "value_characters", 10));
    assert_eq!(value["output_truncated"], true);
    assert_eq!(value["complete"], false);
}

#[test]
fn sqlite_value_cap_is_disclosed_in_json_payload() {
    let root = fixture_root("sqlite-value-cap");
    let db = root.join("fixture.sqlite");
    let connection = Connection::open(&db).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE values_table(value TEXT NOT NULL);\n\
             INSERT INTO values_table(value) VALUES ('abcdefghijklmnopqrstuvwxyz');",
        )
        .unwrap();
    drop(connection);

    let (status, value) = run_json(
        &root,
        &[
            "--json",
            "sqlite",
            "fixture.sqlite",
            "--sql",
            "SELECT value FROM values_table",
            "--max-value-chars",
            "10",
        ],
    );

    assert!(status.success());
    assert!(
        value["rows"][0]["fields"]["value"]
            .as_str()
            .unwrap()
            .ends_with("...\"")
    );
    assert!(has_output_cap(&value, "value_characters", 10));
    assert_eq!(value["output_truncated"], true);
    assert_eq!(value["complete"], false);
}
