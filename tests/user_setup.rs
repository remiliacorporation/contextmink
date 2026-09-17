use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
const TOOL: &str = "contextmink";
const BINARY: &str = env!("CARGO_BIN_EXE_contextmink");
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "personal-{TOOL}-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(BINARY)
            .args(args)
            .arg("--home")
            .arg(&self.0)
            .env_remove("PAPERTIGER_DB")
            .output()
            .unwrap()
    }
    fn install(&self) -> Value {
        let result = self.run(&["setup-user", "--json"]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice(&result.stdout).unwrap()
    }
    fn skill(&self) -> PathBuf {
        self.0.join(format!(".agents/skills/{TOOL}/SKILL.md"))
    }
    fn runtime(&self) -> PathBuf {
        self.0.join(format!(
            ".local/share/{TOOL}/bin/{TOOL}{}",
            std::env::consts::EXE_SUFFIX
        ))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.0.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            result.extend(snapshot(&p));
        } else {
            result.insert(p.clone(), fs::read(p).unwrap());
        }
    }
    result
}
#[test]
fn personal_install_is_guidance_free_idempotent_and_reversible() {
    let f = Fixture::new();
    fs::write(f.0.join("AGENTS.md"), "Existing user guidance\n").unwrap();
    let original = snapshot(&f.0);
    let preview = f.run(&["setup-user", "--dry-run", "--json"]);
    assert!(preview.status.success());
    assert_eq!(snapshot(&f.0), original);
    f.install();
    let skill = fs::read_to_string(f.skill()).unwrap();
    assert!(skill.starts_with("---\nname:"));
    assert!(!skill.contains("<!-- installed-command -->"));
    assert!(skill.contains(".local/share/"));
    assert!(!skill.contains("//?/"));
    assert_eq!(
        fs::read(f.skill()).unwrap(),
        fs::read(f.0.join(format!(".claude/skills/{TOOL}/SKILL.md"))).unwrap()
    );
    let installed = snapshot(&f.0);
    f.install();
    assert_eq!(snapshot(&f.0), installed);
    let version = Command::new(f.runtime()).arg("--version").output().unwrap();
    assert!(
        version.status.success(),
        "{}",
        String::from_utf8_lossy(&version.stderr)
    );
    assert!(f.run(&["uninstall-user", "--dry-run"]).status.success());
    assert_eq!(snapshot(&f.0), installed);
    assert!(f.run(&["uninstall-user"]).status.success());
    assert!(!f.runtime().exists());
    assert!(!f.skill().exists());
    assert_eq!(
        fs::read(f.0.join("AGENTS.md")).unwrap(),
        original[&f.0.join("AGENTS.md")]
    );
    f.install();
}
#[test]
fn personal_collision_and_divergence_refuse_before_writes() {
    let f = Fixture::new();
    fs::create_dir_all(f.skill().parent().unwrap()).unwrap();
    fs::write(f.skill(), "Locally owned skill").unwrap();
    let before = snapshot(&f.0);
    assert!(!f.run(&["setup-user"]).status.success());
    assert_eq!(snapshot(&f.0), before);
    assert!(f.run(&["setup-user", "--replace-managed"]).status.success());
    fs::write(f.skill(), "Modified installed skill").unwrap();
    let before = snapshot(&f.0);
    assert!(!f.run(&["setup-user"]).status.success());
    assert!(!f.run(&["uninstall-user"]).status.success());
    assert_eq!(snapshot(&f.0), before);
    let runtime = Command::new(f.runtime()).arg("--version").output().unwrap();
    assert!(
        !runtime.status.success(),
        "divergent skill/runtime pair must fail closed"
    );
    assert!(f.run(&["setup-user", "--replace-managed"]).status.success());
}
#[test]
fn personal_receipt_cannot_claim_foreign_paths_or_downgrade() {
    let f = Fixture::new();
    f.install();
    let path = f.0.join(format!(".local/share/{TOOL}/user-install.json"));
    let mut receipt: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    receipt["files"]["AGENTS.md"] = Value::String("0".repeat(64));
    fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    let before = snapshot(&f.0);
    assert!(!f.run(&["uninstall-user"]).status.success());
    assert_eq!(snapshot(&f.0), before);
    receipt["files"]
        .as_object_mut()
        .unwrap()
        .remove("AGENTS.md");
    receipt["version"] = Value::String("999.0.0".into());
    fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    let before = snapshot(&f.0);
    assert!(!f.run(&["setup-user", "--replace-managed"]).status.success());
    assert_eq!(snapshot(&f.0), before);
}
#[test]
fn missing_runtime_repairs_but_installed_runtime_cannot_overwrite_itself() {
    let f = Fixture::new();
    f.install();
    let output = Command::new(f.runtime())
        .arg("setup-user")
        .arg("--home")
        .arg(&f.0)
        .output()
        .unwrap();
    assert!(!output.status.success());
    fs::remove_file(f.runtime()).unwrap();
    f.install();
    assert!(f.runtime().exists());
}
#[cfg(unix)]
#[test]
fn symlinked_skill_parent_refuses_without_touching_target() {
    let f = Fixture::new();
    let other = Fixture::new();
    std::os::unix::fs::symlink(&other.0, f.0.join(".agents")).unwrap();
    assert!(!f.run(&["setup-user", "--replace-managed"]).status.success());
    assert!(snapshot(&other.0).is_empty());
    fs::remove_file(f.0.join(".agents")).unwrap();
}

#[test]
fn incomplete_receipt_refuses_without_adopting_files() {
    let f = Fixture::new();
    f.install();
    let path = f.0.join(format!(".local/share/{TOOL}/user-install.json"));
    let mut receipt: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    receipt["files"]
        .as_object_mut()
        .unwrap()
        .remove(&format!(".agents/skills/{TOOL}/SKILL.md"));
    fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    let before = snapshot(&f.0);
    assert!(!f.run(&["setup-user", "--replace-managed"]).status.success());
    assert!(!f.run(&["uninstall-user"]).status.success());
    assert_eq!(snapshot(&f.0), before);
}

#[test]
fn owned_personal_router_upgrades_to_complete_skill_without_replacement_flag() {
    use sha2::Digest;
    let f = Fixture::new();
    f.install();
    let relative = format!(".claude/skills/{TOOL}/SKILL.md");
    let legacy =
        b"---\nname: legacy-router\ndescription: Former router\n---\nRead the canonical skill.\n";
    fs::write(f.0.join(&relative), legacy).unwrap();
    let path = f.0.join(format!(".local/share/{TOOL}/user-install.json"));
    let mut receipt: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    receipt["files"][&relative] = Value::String(format!("{:x}", sha2::Sha256::digest(legacy)));
    fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    let before = snapshot(&f.0);
    assert!(f.run(&["setup-user", "--dry-run"]).status.success());
    assert_eq!(snapshot(&f.0), before);
    f.install();
    assert_eq!(
        fs::read(f.skill()).unwrap(),
        fs::read(f.0.join(relative)).unwrap()
    );
}
