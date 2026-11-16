"""Native face editor checks driven through real game input, in the smoke world."""
import json
import math


def check_face_editor(ctl, observe, find, play, game, report, result, spatial_options=False):
    def editor():
        value = observe(0.15)["faces"]["editor"]
        assert value and value["editor"], value
        return value

    def point(x, y, button="left"):
        value = editor()
        args = ("right",) if button == "right" else ()
        return ctl("click-at", x * value["scale"], y * value["scale"], *args)

    def shortcut(key):
        ctl("key", "ctrl", "down")
        ctl("key", key, "tap")
        ctl("key", "ctrl", "up")

    ctl("options", "mods")
    ctl("click", find("Settings")["id"])
    ctl("click", find("Open face editor")["id"])
    value = editor()
    if spatial_options:
        assert observe()["camera_claimed"], observe()
        result["checks"]["face_editor_spatial_options_camera_ownership"] = True
    assert value["tab"] == 0, value
    original = value["editor"]["marks"]
    cell = min((568 - 32) / 24, (value["height"] - 270) / 8)
    # Erase a detected front-face mark through the same pointer path as the user.
    selected = next(( (x, y) for y, line in enumerate(original.splitlines())
                      for x, mark in enumerate(line.split()[0]) if mark in "epb" ), None)
    assert selected is not None, original
    x, y = selected
    point(30, 72)  # Erase toolbar button; content starts 30 virtual pixels down.
    point(16 + (8 + x + 0.5) * cell, 118 + (y + 0.5) * cell)
    assert editor()["editor"]["marks"] != original
    shortcut("z")
    assert editor()["editor"]["marks"] == original
    shortcut("y")
    assert editor()["editor"]["marks"] != original
    shortcut("z")
    assert editor()["editor"]["marks"] == original
    result["checks"]["face_editor_pointer_marks_undo_redo"] = True
    ctl("screenshot", report.parent / "face-editor-paint.png")

    point(150, 14)  # Style / glow
    value = editor()
    assert value["tab"] == 1
    old_brows = value["editor"]["settings"]["brows"]
    old_interval = value["editor"]["settings"]["blink_seconds"]
    point(120, 54 + 6 * 26 + 10)  # Brows
    point(120, 54 + 10 * 26 + 10, "right")  # Blink interval, decreasing
    value = editor()
    assert value["editor"]["settings"]["brows"] != old_brows
    assert math.isclose(value["editor"]["settings"]["blink_seconds"], old_interval - 0.25, abs_tol=1e-5)
    point(550, 14)  # Save
    assert not editor()["editor"]["dirty"]
    ctl("key", "escape", "tap")
    assert observe()["faces"]["editor"] is None
    data = game / "horizon-mods/.horizon-mod-data/punchy"
    profile = json.loads((data / "moves_face_profile").read_text())
    assert profile["brows"] != old_brows
    assert (data / "moves_face_marks").read_text() == original
    ctl("click", find("Open face editor")["id"])
    value = editor()
    assert math.isclose(value["editor"]["settings"]["blink_seconds"], old_interval - 0.25, abs_tol=1e-5)
    point(150, 14)
    point(120, 54 + 6 * 26 + 10)
    point(120, 54 + 10 * 26 + 10)
    point(550, 14)
    result["checks"]["face_editor_right_click_save_and_reopen"] = True

    point(250, 14)  # Reactions
    settings = editor()["editor"]["settings"]
    old_enabled = settings["reactions"].get("fading", True)
    old_priority = settings["trigger_priorities"].get("fading", 1190)
    point(120, 64)
    point(120, 54 + 2 * 26 + 10)
    settings = editor()["editor"]["settings"]
    assert settings["reactions"]["fading"] != old_enabled
    assert settings["trigger_priorities"]["fading"] == old_priority + 10
    shortcut("z")
    shortcut("z")
    point(550, 14)
    result["checks"]["face_editor_reaction_enable_and_priority"] = True

    point(350, 14)  # Looks
    value = editor()
    assert value["tab"] == 3
    for _ in range(12):
        point(120, 54 + 26 + 10)
        if editor()["editor"]["settings"]["looks"]["neutral"]["right_form"] == "heart":
            break
    else:
        raise AssertionError("named look did not reach the heart eye shape")
    point(550, 14)
    ctl("screenshot", report.parent / "face-editor-looks.png")
    result["checks"]["face_editor_named_look_editing"] = True

    point(450, 14)  # Shapes
    assert editor()["tab"] == 4
    point(30, 120)
    assert editor()["editor"]["settings"]["shapes"]["user_shape"]["grids"]["8x6"][0][0] == "#"
    point(30, 120, "right")
    assert editor()["editor"]["settings"]["shapes"]["user_shape"]["grids"]["8x6"][0][0] == "."
    point(30, 120)
    point(250, editor()["height"] - 35)
    assert editor()["editor"]["settings"]["looks"]["neutral"]["right_form"] == "user_shape"
    ctl("screenshot", report.parent / "face-editor-shapes.png")
    shortcut("z")  # discard the assignment, retaining the earlier colored heart
    point(550, 14)
    result["checks"]["face_editor_custom_shape_drawing_and_assignment"] = True

    point(150, 14)
    for _ in range(3):
        ctl("scroll-at", 60, 100, -1)
    assert editor()["page"] == 3
    for index in (44, 47, 49, 51):  # Left iris: glow, bloom, glare, trail
        point(120, 54 + (index % 13) * 26 + 10)
    settings = editor()["editor"]["settings"]
    assert settings["glow"]["pupils"]
    assert all(settings["glow_effects"][name] for name in ("bloom", "glare", "trail"))
    point(550, 14)
    ctl("screenshot", report.parent / "face-editor-glow.png")
    result["checks"]["face_editor_per_eye_glow_controls"] = True
    # Export uses the editor's actual PNG encoder and skin metadata path.
    point(50, 14)
    point(480, 72)
    exported = list(game.rglob("punchy/exports/face-*.png"))
    # Player data can live under HORIZON_HOME rather than the game directory.
    if not exported:
        exported = list(game.parent.rglob("punchy/exports/face-*.png"))
    assert exported and exported[-1].read_bytes().startswith(b"\x89PNG\r\n\x1a\n"), editor()
    result["checks"]["face_editor_png_skin_export"] = True
    ctl("key", "escape", "tap")
    play()
    if not observe()["camera"]["active"]:
        ctl("key", "v", "tap")
    ctl("key", "f4", "tap")
    assert observe()["camera"]["orbit"]
    ctl("turn", 180, 0)
    observe(1)
    ctl("screenshot", report.parent / "face-custom-front.png")
    ctl("command", "time set midnight")
    observe(1)
    ctl("screenshot", report.parent / "face-custom-glow-front-night.png")
    ctl("turn", -180, 0)
    ctl("key", "f4", "tap")
    value = observe(1)
    assert value["faces"]["own"]["animated"], value
    ctl("screenshot", report.parent / "face-custom-glow-in-game.png")
    ctl("key", "key.keyboard.f12", "tap")
    value = observe(2)
    assert value["faces"]["own"]["animated"] and value["faces"]["own"]["uploaded"], value
    assert not value["faces"]["library_warnings"], value
    ctl("options", "mods")
    ctl("click", find("Settings")["id"])
    ctl("click", find("Open face editor")["id"])
    settings = editor()["editor"]["settings"]
    assert settings["shapes"]["user_shape"]["grids"]["8x6"][0][0] == "#"
    assert settings["glow"]["pupils"] and settings["glow_effects"]["trail"]
    result["checks"]["face_profile_survives_resource_reload"] = True
    ctl("key", "escape", "tap")
    play()
