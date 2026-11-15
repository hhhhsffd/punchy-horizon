#!/usr/bin/env python3
"""Stage/check/build the source bundle alone or together with Spatial GUI."""
import argparse
import filecmp
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import zipfile
import uuid

ROOT = Path(__file__).resolve().parents[1]
COMPAT = json.loads((ROOT / "compatibility.json").read_text())

def run(args, **kwargs):
    print("Running:", " ".join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), check=True, **kwargs)

def sync_stage(fresh, stage):
    """Preserve unchanged source mtimes so Cargo can reuse earlier builds."""
    allowed = (ROOT / ".build").resolve()
    for path in (fresh, stage):
        if not path.resolve().is_relative_to(allowed) or path.resolve() == allowed:
            raise SystemExit("Stage must stay inside " + str(allowed))
    for path in fresh.rglob("*"):
        dest = stage / path.relative_to(fresh)
        if path.is_dir():
            dest.mkdir(parents=True, exist_ok=True)
        elif not dest.is_file() or not filecmp.cmp(path, dest, shallow=False):
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, dest)
    for path in sorted(stage.rglob("*"), reverse=True):
        if not (fresh / path.relative_to(stage)).exists():
            if path.is_dir():
                path.rmdir()
            else:
                path.unlink()
    shutil.rmtree(fresh)

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("action", choices=["plan", "check", "build"])
    p.add_argument("--horizon-source", type=Path, required=True)
    p.add_argument("--mixins", type=Path, required=True)
    p.add_argument("--spatial-gui", type=Path, help="Spatial GUI checkout or extracted mod folder")
    p.add_argument("--target-dir", type=Path)
    args = p.parse_args()
    source = args.horizon_source.resolve()
    # Source archives may have a revision marker; git checkouts must match.
    marker = source / ".horizon-revision"
    if marker.exists():
        revision = marker.read_text().strip()
    elif (source / ".git").exists():
        revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    else:
        p.error("Source export needs a .horizon-revision marker")
    if revision != COMPAT["horizon_revision"]:
        p.error("Wrong Horizon revision: " + revision)
    run([sys.executable, ROOT / "tools/package.py"])
    build = ROOT / ".build"
    mods = build / ("combined-mods" if args.spatial_gui else "mods")
    mods.mkdir(parents=True, exist_ok=True)
    bundle = ROOT / "dist/punchy-rust-preview-0.2.0.hnmod"
    with zipfile.ZipFile(bundle) as archive:
        for name in archive.namelist():
            if name.startswith("mixins/"):
                target = mods / Path(name).relative_to("mixins")
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(archive.read(name))
    if args.spatial_gui:
        spatial = args.spatial_gui.resolve()
        if (spatial / "mixins/spatial_gui").is_dir():
            spatial /= "mixins/spatial_gui"
        shutil.copytree(spatial, mods / "spatial_gui", dirs_exist_ok=True)
    suffix = "combined" if args.spatial_gui else "punchy"
    if args.action == "plan":
        run([args.mixins.resolve(), "plan", "--workspace", source, "--mods", mods, "--out", build / f"{suffix}-plan.json"])
        return
    stage = build / f"{suffix}-stage"
    fresh = build / f"{suffix}-stage-new-{uuid.uuid4().hex}"
    run([args.mixins.resolve(), "apply", "--workspace", source, "--mods", mods, "--out", fresh])
    sync_stage(fresh, stage)
    env = dict(os.environ)
    env["CARGO_TARGET_DIR"] = str((args.target_dir or build / "cargo").resolve())
    run(["cargo", args.action, "--profile", "fast", "-p", "horizon-official", "--bin", "horizon-official"],
        cwd=stage, env=env)

if __name__ == "__main__":
    main()
