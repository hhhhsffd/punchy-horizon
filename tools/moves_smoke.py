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
import traceback

p = argparse.ArgumentParser(description=__doc__)
for name in ("binary", "ctl", "resources", "pack", "report"):
    p.add_argument("--" + name, type=Path, required=True)
p.add_argument("--swimming", action="store_true", help="also exercise authored underwater swimming")
p.add_argument("--elytra", action="store_true", help="also exercise grounded walking with elytra equipped")
p.add_argument("--faces", action="store_true", help="also exercise the integrated face editor and exports")
p.add_argument("--spatial-options", action="store_true", help="exercise settings inside Spatial GUI's captured options surface")
a = p.parse_args()
for name in ("binary", "ctl", "resources", "pack", "report"):
    setattr(a, name, getattr(a, name).resolve())
root = Path(tempfile.mkdtemp(prefix="punchy-moves-smoke-"))
game = root / "game"
game.mkdir()
shutil.copytree(a.pack, game / "resourcepacks/FluidAndFoolish")
(game / "options.txt").write_text('enableVsync:false\nmaxFps:60\npauseOnLostFocus:false\nguiScale:2\nresourcePacks:["vanilla","file/FluidAndFoolish"]\n')
(game / "horizon-graphics.txt").write_text("lodDistance:0\nhdr:false\npickedShaderPack:\n")
if a.spatial_options:
    (game / "config").mkdir()
    (game / "config/spatial-gui.json5").write_text(json.dumps({
        "allScreens": True, "disableThirdPersonParallax": True,
    }))
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

def raw_ctl(*args):
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

def projected_point(x, y):
    """Invert the live curved-menu pointer mapping without bypassing input."""
    width, height = raw_ctl("ui")["gui_size"]
    guess = [width / 2, height / 2]
    def sample(point):
        # Zero wheel steps moves the real pointer without changing a setting.
        raw_ctl("scroll-at", *point, 0)
        return observe(0.12)["faces"]["pointer"]
    for _ in range(8):
        mapped = sample(guess)
        error = [x - mapped[0], y - mapped[1]]
        if max(map(abs, error)) < 0.6:
            return guess
        dx = sample([guess[0] + 2, guess[1]])
        dy = sample([guess[0], guess[1] + 2])
        a, b = (dx[0] - mapped[0]) / 2, (dy[0] - mapped[0]) / 2
        c, d = (dx[1] - mapped[1]) / 2, (dy[1] - mapped[1]) / 2
        determinant = a * d - b * c
        assert abs(determinant) > 1e-5, (guess, mapped, dx, dy)
        step = [(d * error[0] - b * error[1]) / determinant,
                (a * error[1] - c * error[0]) / determinant]
        guess = [max(0, min(width - 0.01, guess[0] + step[0])),
                 max(0, min(height - 0.01, guess[1] + step[1]))]
    raise AssertionError(f"projected pointer did not reach {(x, y)}: {mapped}")

def ctl(*args):
    if a.spatial_options and args[0] in ("click", "click-at", "scroll-at"):
        ui = raw_ctl("ui")
        if ui["screen"] == "options" and observe(0.08)["camera_claimed"]:
            if args[0] == "click":
                node = next(n for n in ui["widgets"] if n["id"] == args[1])
                x, y, width, height = node["bounds"]
                point = projected_point(x + width / 2, y + height / 2)
                return raw_ctl("click-at", *point, *args[2:])
            point = projected_point(float(args[1]), float(args[2]))
            return raw_ctl(args[0], *point, *args[3:])
    return raw_ctl(*args)

def find(prefix):
    ui = ctl("ui")
    w, h = ui["gui_size"]
    def visible(nodes):
        return next((node for node in nodes if (node.get("label") or "").startswith(prefix)
                     and 38 < node["bounds"][1] + node["bounds"][3] / 2 < h - 38), None)
    if node := visible(ui["widgets"]):
        return node
    ctl("scroll-at", w / 2, h / 2, 250)
    for _ in range(80):
        ui = ctl("ui")
        if node := visible(ui["widgets"]):
            return node
        ctl("scroll-at", w / 2, h / 2, -6)
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
    # F5 intentionally requests vanilla perspective; V activates Moves.
    ctl("key", "v", "tap")
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
    if a.elytra:
        ctl("command", "item replace entity @s armor.chest with minecraft:elytra")
        ctl("command", "tp @s 0 89 0 0 0")
        observe(1.2)  # allow the one-shot equipment reaction to finish
        ctl("key", "w", "down")
        before = observe(0.4)
        after = observe(0.25)
        ctl("release")
        assert after["elytra_equipped"] and after["pose_owned"], after
        assert after["grounded"] and after["movement_state"] == "ground", after
        assert after["gait_phase"] != before["gait_phase"], (before, after)
        # Fluid & Foolish names the normal forward gait "normal_forward".
        assert "normal_forward" in after["layers"], after
        assert not any(name.startswith("elytra_") for name in after["layers"]), after
        first = before["faces"]["own"]["wing_delta"]
        last = after["faces"]["own"]["wing_delta"]
        assert first is not None and last is not None, (before, after)
        assert any(abs(a - b) > 1e-5 for a, b in zip(first, last)), (first, last)
        ctl("screenshot", a.report.parent / "moves-elytra-walking.png")
        result["checks"]["equipped_elytra_keeps_grounded_walking_animated"] = True
        result["checks"]["elytra_wing_anchor_follows_torso_animation"] = True
        ctl("command", "item replace entity @s armor.chest with minecraft:air")
        ctl("command", "tp @s 0 89 0 0 0")
        observe(0.8)
    if a.swimming:
        ctl("command", "fill -8 88 -8 8 94 14 water")
        ctl("command", "gamemode survival")
        ctl("command", "tp @s 0 90 0 0 0")
        ctl("key", "w", "down")
        ctl("key", "ctrl", "down")
        d = observe(1.5)
        ctl("release")
        assert d["suppress_vanilla_swim"], d
        assert any(name.startswith("swimming_") for name in d["layers"]), d
        ctl("screenshot", a.report.parent / "moves-swimming.png")
        ctl("key", "f5", "tap")
        observe(0.5)
        ctl("screenshot", a.report.parent / "moves-swimming-front.png")
        ctl("key", "v", "tap")
        result["checks"]["authored_swimming_suppresses_duplicate_outer_rotation"] = True
        ctl("command", "gamemode creative")
        ctl("command", "fill -8 88 -8 8 94 14 air")
        ctl("command", "fill -4 88 -4 4 88 4 stone")
        ctl("command", "tp @s 0 89 0 0 0")
        assert not observe(1)["suppress_vanilla_swim"]
        result["checks"]["leaving_water_restores_normal_outer_rotation"] = True
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
    own_face = observe(1)["faces"]["own"]
    assert own_face is None or not own_face["animated"]
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
    if a.faces:
        from face_editor_smoke import check_face_editor
        check_face_editor(ctl, observe, find, play, game, a.report, result, a.spatial_options)
    ctl("quit")
    process.wait(timeout=30)
    process = None
    result["success"] = True
except Exception as error:
    result.update(success=False, error=str(error), traceback=traceback.format_exc())
    if process is not None and process.poll() is None:
        try:
            result["failure_ui"] = ctl("ui")
            ctl("screenshot", a.report.with_suffix(".failure.png"))
        except Exception:
            pass
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
