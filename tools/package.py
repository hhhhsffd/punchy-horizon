#!/usr/bin/env python3
"""Package source/data and validate with the pinned Horizon bundle parser."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
COMPAT = json.loads((ROOT / "compatibility.json").read_text())
PIN = COMPAT["horizon_revision"]
MIXINS = COMPAT["horizon_mixins_revision"]
VERSION = tomllib.loads((ROOT / "mixins/punchy/horizon-mixin.toml").read_text())["mod"]["version"]
def engine_files():
    """Follow out-of-line engine modules, including newly ported submodules."""
    source = ROOT / "mixins/punchy"
    found = set()
    pending = ["engine.rs"]
    while pending:
        name = pending.pop()
        if name in found:
            continue
        path = source / name
        if not path.is_file():
            raise SystemExit("Missing engine module: " + name)
        found.add(name)
        for module in re.finditer(
            r'(?m)^(?:#\[path\s*=\s*"([^"]+)"\]\s*)?(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_]\w*)\s*;',
            path.read_text(),
        ):
            pending.append((Path(name).parent / (module.group(1) or module.group(2) + ".rs")).as_posix())
    return tuple(sorted(found))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--horizon-source', type=Path,
                        help='Horizon source checked out at the tested commit')
    parser.add_argument('--offline', action='store_true')
    parser.add_argument('--validate', action='store_true', help='Validate with Horizon Bundle::read')
    args = parser.parse_args()
    if args.validate and not args.horizon_source:
        parser.error('--validate requires --horizon-source')
    source = ROOT / 'mixins/punchy'
    engine = engine_files()
    manifest = {'schema': 1, 'id': 'punchy', 'version': VERSION,
                'game': '26.3', 'horizon': PIN, 'mixins': MIXINS}
    files = {'horizon-native.json': (json.dumps(manifest, indent=2) + '\n').encode()}
    prefix = 'mixins/punchy/'
    for path in sorted(source.iterdir()):
        if path.is_file() and path.suffix in ('.rs', '.toml') and path.name not in engine:
            files[prefix + path.name] = path.read_bytes()
    for name in engine:
        files[prefix + 'engine/' + name] = (source / name).read_bytes()
    files[prefix + 'engine/Cargo.toml'] = b'''[package]
name = "punchy-engine"
version = "3.1.0"
edition = "2024"
publish = false
[lib]
path = "engine.rs"
[dependencies]
glam = "0.30"
serde_json = "1.0.151"
zip = { version = "9.1.3", default-features = false, features = ["deflate"] }
'''
    for path in sorted((source / 'data').rglob('*')):
        if path.is_file():
            files[prefix + 'engine/data/' + path.relative_to(source / 'data').as_posix()] = path.read_bytes()
    for name in ('LICENSE', 'LICENSE-Punchy', 'LICENSE-Horizon', 'THIRD_PARTY_NOTICES.md', 'README.md'):
        bundle_name = 'NOTICE.md' if name == 'THIRD_PARTY_NOTICES.md' else name
        files[prefix + bundle_name] = (ROOT / name).read_bytes()
    output = ROOT / 'dist' / ('punchy-rust-preview-' + VERSION + '.hnmod')
    output.parent.mkdir(exist_ok=True)
    with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in sorted(files.items()):
            info = zipfile.ZipInfo(name, (2026, 11, 3, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(info, data)
    if args.validate:
        horizon = args.horizon_source.resolve()
        dependency = horizon / 'crates/horizon-mods'
        validator = ROOT / '.build/validator'
        (validator / 'src').mkdir(parents=True, exist_ok=True)
        (validator / 'Cargo.toml').write_text('''[package]
name = "punchy-package-validator"
version = "0.1.0"
edition = "2024"
publish = false
[dependencies]
horizon-mods = { path = ''' + json.dumps(str(dependency)) + ''', features = ["index"] }\n''')
        shutil.copyfile(ROOT / 'tools/validator-src/main.rs', validator / 'src/main.rs')
        command = ['cargo', 'run', '--manifest-path', str(validator / 'Cargo.toml')]
        if args.offline:
            command.append('--offline')
        subprocess.run(command + ['--', str(output)], check=True, env=dict(os.environ))
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    output.with_suffix(".hnmod.sha256").write_text(f"{digest}  {output.name}\n")
    print(json.dumps({'file': output.name, 'sha256': hashlib.sha256(output.read_bytes()).hexdigest(),
                      'files': len(files), 'parser_validated': args.validate}, indent=2))


if __name__ == '__main__':
    main()
