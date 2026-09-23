---
name: contextmink-bridge
description: Run project Bash scripts from Windows agents and preserve arguments across Git Bash/native process boundaries. Skip ordinary native commands and sessions already running in Bash.
---

# Contextmink Bridge

Use the bridge when a Windows-native session needs a project's Bash script.
Keep the project's `.sh` workflow; no PowerShell or cmd wrapper is needed.
The bridge selects Git Bash and protects forwarded slash-bearing arguments from
MSYS rewriting. A bare `bash` or `sh` on PATH may select WSL or Cygwin instead.

<!-- installed-command -->

For a project installation, resolve
`tools/contextmink/bin/contextmink-bridge.exe` from the project root. Invoke it
with PowerShell's `&`; run from the consuming project. If absent, report that a
complete Windows Contextmink release is needed rather than guessing another
executable.

```powershell
& $bridge --script scripts/verify.sh
& $bridge --cwd $projectRoot --script scripts/build.sh -- release
```

Here `$bridge` is the resolved executable and `$projectRoot` is an absolute path.
Relative script paths resolve from the bridge's repository/config root; use an
absolute script path when that differs from the intended project. `--cwd` sets
the child's working directory. Keep native `cargo`, `git`, and Contextmink
commands direct unless they need a fragile argument transport. When bounded
output is needed, Contextmink `capture --script` already owns the same boundary.

Use live `--help` for Git Bash utilities (`--login`), literal leading `--`, or
argument transport. PowerShell 5.1 can damage quotes before the bridge receives
them: use `--argv-b64` or `--argfile` for those arguments. Do not construct a
`bash -c` command string or globally disable MSYS path conversion.

Exit status is the child's status; diagnose a refusal before retrying. The bridge
does not retain a full output log. Redirect long build output on its first run
when later inspection may matter. Do not rebuild or replace the running bridge
through itself: run its replacement build directly with native Cargo.
