"""Turn a staged native release into an overlay containing only tool-owned paths."""
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tarfile
import zipfile

stage, archive = map(Path, sys.argv[1:])
source = Path(__file__).resolve().parents[1]
stage = stage.resolve(strict=True)
manifest = json.loads((stage / 'manifest.json').read_text(encoding='utf-8-sig'))
tool = manifest['name']
assert tool in ('papertiger', 'contextmink')
assert not (stage / 'tools').exists(), 'stage must be a fresh native release'
children = list(stage.iterdir())
owned = stage / 'tools' / tool
(owned / 'bin').mkdir(parents=True)
names = manifest.get('binaries', [manifest.get('binary')])
if manifest.get('bridge_binary'):
    names.append(manifest['bridge_binary'])
for child in children:
    assert child.resolve().is_relative_to(stage)
    child.rename(owned / ('bin/' + child.name if child.name in names else child.name))
manifest['schema'] = tool + '.release-manifest.v2'
manifest['layout'] = 'project-overlay'
manifest['binary_sha256'] = {
    'bin/' + name: hashlib.sha256((owned / 'bin' / name).read_bytes()).hexdigest()
    for name in names
}
if 'binaries' in manifest:
    manifest['binaries'] = ['bin/' + name for name in names]
else:
    manifest['binary'] = 'bin/' + manifest['binary']
    if 'bridge_binary' in manifest:
        manifest['bridge_binary'] = 'bin/' + manifest['bridge_binary']
(owned / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
template = source / ('templates/papertiger' if tool == 'papertiger' else 'templates/skills/contextmink')
for directory in ('.agents', '.claude'):
    skill = stage / directory / 'skills' / tool
    if directory == '.agents':
        shutil.copytree(template, skill)
    else:
        skill.mkdir(parents=True)
        shutil.copyfile(template / 'SKILL.md', skill / 'SKILL.md')
    body = (skill / 'SKILL.md').read_text(encoding='utf-8').replace('<!-- installed-command -->\n', '')
    (skill / 'SKILL.md').write_text(body, encoding='utf-8')
if tool == 'contextmink':
    shutil.copyfile(source / 'templates/AGENTS.contextmink.md', owned / 'agent_integration.md')
paths = sorted(p for p in stage.rglob('*') if p.is_file())
assert all(p.relative_to(stage).parts[0] in ('.agents', '.claude', 'tools') for p in paths)
if archive.name.endswith('.zip'):
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as output:
        for path in paths:
            output.write(path, path.relative_to(stage).as_posix())
else:
    with tarfile.open(archive, 'w:gz') as output:
        for path in paths:
            output.add(path, arcname=path.relative_to(stage).as_posix(), recursive=False)
print(f'{tool}: packaged skills and tools at project-relative paths')
