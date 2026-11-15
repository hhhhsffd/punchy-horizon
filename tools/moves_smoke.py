#!/usr/bin/env python3
"""Run the combined Moves/Spatial GUI game in an isolated throwaway world."""
import argparse
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

p = argparse.ArgumentParser(description=__doc__)
for name in ("binary", "ctl", "resources", "pack", "report"):
    p.add_argument("--" + name, type=Path, required=True)
a = p.parse_args()
for name in vars(a):
    setattr(a, name, getattr(a, name).resolve())
root = Path(tempfile.mkdtemp(prefix="punchy-moves-smoke-"))
game = root / "game"
game.mkdir()
shutil.copytree(a.pack, game / "resourcepacks/FluidAndFoolish")
(game / "options.txt").write_text('enableVsync:false\nmaxFps:60\npauseOnLostFocus:false\nguiScale:2\nresourcePacks:["vanilla","file/FluidAndFoolish"]\n')
(game / "horizon-graphics.txt").write_text("lodDistance:0\nhdr:false\npickedShaderPack:\n")
env = dict(os.environ)
for key, folder in [("HORIZON_HOME", "home"), ("HORIZON_LAUNCHER_HOME", "launcher"),
    ("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"),
    ("HORIZON_LAUNCHER_TRASH", "trash"), ("HORIZON_LAUNCHER_CLIPBOARD", "clipboard"),
    ("HORIZON_LAUNCHER_DESKTOP", "desktop")]:
    path = root / folder
    path.mkdir()
    env[key] = str(path)
socket = root / "control.sock"
diagnostic = root / "moves.json"
env.update(HORIZON_NO_AUDIO="1", HORIZON_LAUNCHER_NO_SELF_UPDATE="1", PUNCHY_MOVES_DIAGNOSTICS=str(diagnostic))
a.report.parent.mkdir(parents=True, exist_ok=True)
result = {"scratch": str(root), "checks": {}}
log = a.report.with_suffix(".log").open("w")
process = None

def ctl(*args):
    r = subprocess.run([str(a.ctl), "--socket", str(socket), "--timeout", "40", *map(str, args)],
        env=env, capture_output=True, text=True, timeout=50)
    if r.returncode:
        raise RuntimeError(" ".join(map(str, args)) + ": " + r.stdout + r.stderr)
    return json.loads(r.stdout)

def observe(seconds=0.3):
    end = time.monotonic() + seconds
    latest = None
    while time.monotonic() < end:
        if process.poll() is not None:
            raise RuntimeError("game exited; see log")
        try:
            latest = json.loads(diagnostic.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            pass
        time.sleep(0.04)
    if latest is None:
        raise RuntimeError("no Moves diagnostic")
    return latest

def find(prefix):
    ui = ctl("ui")
    w, h = ui["gui_size"]
    ctl("scroll-at", w / 2, h / 2, 50)
    for _ in range(24):
        ui = ctl("ui")
        for node in ui["widgets"]:
            if (node.get("label") or "").startswith(prefix) and 38 < node["bounds"][1] + node["bounds"][3] / 2 < h - 38:
                return node
        ctl("scroll-at", w / 2, h / 2, -2)
    raise AssertionError("missing widget " + prefix)

def play():
    for _ in range(12):
        if ctl("state")["screen"] == "playing":
            return
        ctl("key", "escape", "tap")
    raise AssertionError("did not return to playing")

try:
    process = subprocess.Popen([str(a.binary), "--game-dir", str(game), "--resources", str(a.resources),
        "--world", "MovesSmoke", "--seed", "0", "--creative", "--test-terrain", "--offscreen",
        "--control", "--control-socket", str(socket), "--render-distance", "3", "--width", "960", "--height", "540"],
        cwd=game, env=env, stdout=log, stderr=subprocess.STDOUT)
    end = time.monotonic() + 55
    while not socket.exists():
        if process.poll() is not None or time.monotonic() > end:
            raise RuntimeError("game did not start; see log")
        time.sleep(0.1)
    ctl("wait-ready", "45")
    play()
    ctl("command", "tp @s 0 90 0 0 0")
    ctl("command", "fill -4 88 -4 4 88 4 stone")
    d = observe(2)
    assert d["clips"] == 268 and not d["warnings"], d
    assert d["pose_owned"], d
    result["checks"]["selected_external_pack_and_body_pose"] = True
    ctl("key", "f5", "tap")
    d = observe(2)
    assert d["camera"]["active"] and all(math.isfinite(v) for v in d["camera"]["position"]), d
    result["checks"]["shoulder_camera_renders"] = True
    ctl("key", "f4", "tap")
    assert observe()["camera"]["orbit"]
    ctl("key", "f4", "tap")
    assert not observe()["camera"]["orbit"]
    result["checks"]["orbit_toggle"] = True
    face = observe()["faces"]["own"]
    assert face and face["eyes"] > 0 and face["animated"] and face["uploaded"], face
    result["checks"]["in_game_face_skin_uploaded"] = True
    ctl("key", "e", "tap")
    d = observe(1)
    assert not d["pose_owned"] and d["camera_claimed"], d
    ctl("screenshot", a.report.parent / "spatial-inventory.png")
    ctl("key", "escape", "tap")
    assert observe(2)["camera"]["active"]
    result["checks"]["spatial_gui_camera_handoff"] = True
    ctl("options", "mods")
    ctl("click", find("Settings")["id"])
    ctl("click", find("[+] Horizontal")["id"], "right")
    value = float((game / "horizon-mods/.horizon-mod-data/punchy/moves_camera_horizontal").read_text())
    assert math.isclose(value, 1.07, abs_tol=1e-5), value
    result["checks"]["right_click_numeric_decreases"] = True
    section = find("\u25b6 Section:")
    ctl("click", section["id"], "right")
    assert "Held Items" in find("\u25b6 Section:")["label"]
    result["checks"]["right_click_section_cycles_backwards"] = True
    w, h = ctl("ui")["gui_size"]
    ctl("click-at", w - 2, h / 2, "right")
    ctl("click", find("Settings")["id"])
    result["checks"]["right_click_empty_space_goes_back"] = True
    ctl("click", find("Facial animation:")["id"])
    play()
    assert not observe(1)["faces"]["own"]["animated"]
    result["checks"]["face_toggle_restores_original_skin"] = True
    ctl("options", "mods")
    ctl("click", find("Settings")["id"])
    ctl("click", find("Facial animation:")["id"])
    play()
    assert observe(1)["faces"]["own"]["animated"]
    ctl("options", "mods")
    ctl("click", find("Settings")["id"])
    node = find("Moves master:")
    ctl("click", node["id"])
    assert not observe()["enabled"] and not observe()["pose_owned"]
    ctl("click", find("Moves master:")["id"])
    play()
    assert observe(2)["enabled"] and observe()["pose_owned"]
    result["checks"]["moves_master_toggle"] = True
    ctl("quit")
    process.wait(timeout=30)
    process = None
    result["success"] = True
except Exception as error:
    result.update(success=False, error=str(error))
finally:
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    a.report.write_text(json.dumps(result, indent=2) + "\n")
    log.close()
print(json.dumps(result, indent=2))
raise SystemExit(0 if result["success"] else 1)
