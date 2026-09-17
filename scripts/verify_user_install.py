"""Exercise an extracted native release in an isolated home and existing project."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def snapshot(root):
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in root.rglob("*") if p.is_file()}


binary = Path(sys.argv[1]).resolve(strict=True)
tool = binary.stem
assert tool in ("papertiger", "contextmink")
with tempfile.TemporaryDirectory(prefix=f"{tool} personal smoke ") as directory:
    root = Path(directory)
    home = root / "home"
    project = root / "established project"
    home.mkdir()
    project.mkdir()
    for name in ("AGENTS.md", "CLAUDE.md", ".gitignore", "README.md"):
        (project / name).write_text(f"Existing {name}\n", encoding="utf-8")
    (home / "AGENTS.md").write_text("Existing personal instructions\n", encoding="utf-8")
    original_project = snapshot(project)
    original_home = snapshot(home)
    env = {k: v for k, v in os.environ.items() if not k.startswith("PAPERTIGER_")}

    def run(exe, *args):
        result = subprocess.run([str(exe), *map(str, args)], cwd=project,
                                env=env, capture_output=True, text=True)
        assert result.returncode == 0, result.stderr
        return result.stdout

    run(binary, "setup-user", "--home", home, "--dry-run")
    assert snapshot(home) == original_home
    result = json.loads(run(binary, "setup-user", "--home", home, "--json"))
    assert result["schema"] == f"{tool}.user_setup.v1"
    installed = snapshot(home)
    run(binary, "setup-user", "--home", home)
    assert snapshot(home) == installed
    runtime = home / ".local" / "share" / tool / "bin" / binary.name
    assert run(runtime, "--version") == run(binary, "--version")
    skill = (home / ".agents" / "skills" / tool / "SKILL.md").read_text(encoding="utf-8")
    # The installer canonicalizes Windows short-path aliases.
    assert runtime.resolve().as_posix() in skill
    assert "<!-- installed-command -->" not in skill
    assert skill.startswith("---\nname:")
    assert (home / ".claude" / "skills" / tool / "SKILL.md").read_text(encoding="utf-8") == skill
    database = home / ".local/share/papertiger/state/papertiger.sqlite"
    if tool == "papertiger":
        plans = json.loads(run(runtime, "--db", database, "plan", "list", "--json"))
        assert [p["slug"] for p in plans["plans"]] == ["personal"]
        run(runtime, "--db", database, "audit")
    else:
        result = json.loads(run(runtime, "--json", "files", ".", "--limit", "10"))
        assert result["scope_complete"]
    run(binary, "uninstall-user", "--home", home, "--dry-run")
    assert snapshot(home) == installed
    run(binary, "uninstall-user", "--home", home)
    assert not runtime.exists()
    assert not (home / ".agents" / "skills" / tool / "SKILL.md").exists()
    if tool == "papertiger":
        assert hashlib.sha256(database.read_bytes()).hexdigest() == installed[str(database.relative_to(home))]
    run(binary, "setup-user", "--home", home)
    assert snapshot(project) == original_project
    assert (home / "AGENTS.md").read_text(encoding="utf-8") == "Existing personal instructions\n"
print(f"{tool}: personal extracted-install smoke passed")
