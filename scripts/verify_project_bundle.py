"""Exercise the extracted overlay without running an installation command."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1]).resolve(strict=True)
tool = next((bundle / 'tools').iterdir()).name
assert tool in ('papertiger', 'contextmink')
owned = bundle / 'tools' / tool
manifest = json.loads((owned / 'manifest.json').read_text())
assert manifest['schema'] == tool + '.release-manifest.v2'
assert manifest['layout'] == 'project-overlay'
for name, digest in manifest['binary_sha256'].items():
    assert (owned / name).resolve().is_relative_to(owned)
    assert hashlib.sha256((owned / name).read_bytes()).hexdigest() == digest
for path in bundle.rglob('*'):
    if path.is_file():
        relative = path.relative_to(bundle)
        assert relative.parts[0] in ('.agents', '.claude', 'tools')
        assert path.name not in ('project-install.json', 'runtime-install.json')
        assert path.suffix != '.sqlite'
skill = bundle / '.agents/skills' / tool / 'SKILL.md'
assert skill.read_bytes() == (bundle / '.claude/skills' / tool / 'SKILL.md').read_bytes()
assert '<!-- installed-command -->' not in skill.read_text()
env = {k: v for k, v in os.environ.items() if not k.startswith('PAPERTIGER_')}
env['PAPERTIGER_ACTOR'] = 'bundle-smoke'
env['PAPERTIGER_SESSION'] = 'bundle-smoke'
suffix = '.exe' if os.name == 'nt' else ''

with tempfile.TemporaryDirectory(prefix=tool + ' project overlay ') as directory:
    project = Path(directory) / 'established project'
    project.mkdir()
    for name in ('AGENTS.md', 'CLAUDE.md', 'README.md', '.gitignore'):
        (project / name).write_text('Existing project-owned ' + name + '\n')
    baseline = {p.name: p.read_bytes() for p in project.iterdir()}
    shutil.copytree(bundle, project, dirs_exist_ok=True)
    nested = project / 'nested/work'
    nested.mkdir(parents=True)
    binary = project / 'tools' / tool / 'bin' / (tool + suffix)

    def run(*args, cwd=nested, success=True):
        result = subprocess.run([str(binary), *map(str, args)], cwd=cwd, env=env,
                                capture_output=True, text=True)
        assert (result.returncode == 0) == success, (args, result.stdout, result.stderr)
        return result.stdout

    assert run('--version').strip() == tool + ' ' + manifest['version']
    if tool == 'papertiger':
        run('status', success=False)
        assert not (project / 'state/papertiger.sqlite').exists()
        run('init')
        run('plan', 'add', 'work', 'Project work', '--intent', 'Preserve outcomes')
        run('add', 'Existing outcome', '--plan', 'work', '--intent', 'Retain project history', '--intent-source', 'user')
        before = json.loads(run('show', '1', '--json'))['task']
        shutil.copytree(bundle, project, dirs_exist_ok=True)
        assert json.loads(run('show', '1', '--json'))['task'] == before
        run('--project-root', project, 'audit')
        assert not (nested / 'state').exists()
        database = project / 'state/papertiger.sqlite'
        database.unlink()
        run('status', success=False)
        assert not database.exists(), 'reads must not replace missing history'
        metadata = project / 'tools/papertiger/manifest.json'
        bad = json.loads(metadata.read_text())
        bad['binary_sha256']['bin/papertiger' + suffix] = '0' * 64
        metadata.write_text(json.dumps(bad))
        run('status', success=False)
    else:
        (project / 'sample.json').write_text('{"count":17}')
        output = json.loads(run('--json', 'json-select', '../../sample.json', '--fields', 'count'))
        assert output['scope_complete']
    for name, content in baseline.items():
        assert (project / name).read_bytes() == content
print(tool + ': direct project overlay smoke passed')
