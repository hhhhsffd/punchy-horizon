"""Native-input regressions for punch return, sprint hip seams and face shortcuts."""
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
for name in ('binary', 'ctl', 'resources', 'pack', 'report'):
    p.add_argument('--' + name, type=Path, required=True)
a = p.parse_args()
for name in ('binary', 'ctl', 'resources', 'pack', 'report'):
    setattr(a, name, getattr(a, name).resolve())
root = Path(tempfile.mkdtemp(prefix='punchy-motion-smoke-'))
game = root / 'game'
game.mkdir()
shutil.copytree(a.pack, game / 'resourcepacks/FluidAndFoolish')
(game / 'options.txt').write_text('enableVsync:false\nmaxFps:60\npauseOnLostFocus:false\nguiScale:2\nresourcePacks:["vanilla","file/FluidAndFoolish"]\n')
(game / 'horizon-graphics.txt').write_text('lodDistance:0\nhdr:false\npickedShaderPack:\n')
env = dict(os.environ)
for key, folder in [('HORIZON_HOME', 'home'), ('HORIZON_LAUNCHER_HOME', 'launcher'),
                    ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                    ('HORIZON_LAUNCHER_TRASH', 'trash'), ('HORIZON_LAUNCHER_CLIPBOARD', 'clipboard'),
                    ('HORIZON_LAUNCHER_DESKTOP', 'desktop')]:
    path = root / folder
    path.mkdir()
    env[key] = str(path)
socket = root / 'control.sock'
hands = root / 'hands.json'
moves = root / 'moves.json'
env.update(HORIZON_NO_AUDIO='1', HORIZON_LAUNCHER_NO_SELF_UPDATE='1',
           PUNCHY_RENDER_DIAGNOSTICS=str(hands), PUNCHY_MOVES_DIAGNOSTICS=str(moves))
a.report.parent.mkdir(parents=True, exist_ok=True)
log = a.report.with_suffix('.log').open('w')
result = {'scratch': str(root), 'checks': {}}
process = None

def ctl(*args):
    out = subprocess.run([str(a.ctl), '--socket', str(socket), '--timeout', '40', *map(str, args)],
                         env=env, capture_output=True, text=True, timeout=50)
    if out.returncode:
        raise RuntimeError(out.stdout + out.stderr)
    return json.loads(out.stdout)

def read(path):
    return json.loads(path.read_text())

def observe(seconds=.3):
    end = time.monotonic() + seconds
    value = None
    while time.monotonic() < end:
        assert process.poll() is None, 'game exited'
        try:
            value = read(moves)
        except (OSError, json.JSONDecodeError):
            pass
        time.sleep(.02)
    assert value is not None, 'no game diagnostic'
    return value

def play():
    for _ in range(12):
        if ctl('state')['screen'] == 'playing':
            return
        ctl('key', 'escape', 'tap')
    raise AssertionError('menu did not close')

def launch():
    global process
    socket.unlink(missing_ok=True)
    process = subprocess.Popen([str(a.binary), '--game-dir', str(game), '--resources', str(a.resources),
        '--world', 'MotionFixes', '--seed', '0', '--creative', '--test-terrain', '--offscreen',
        '--control', '--control-socket', str(socket), '--render-distance', '3', '--width', '960', '--height', '540'],
        cwd=game, env=env, stdout=log, stderr=subprocess.STDOUT)
    end = time.monotonic() + 55
    while not socket.exists():
        assert process.poll() is None and time.monotonic() < end, 'game did not start'
        time.sleep(.1)
    ctl('wait-ready', '45')
    play()
    observe(1)

def find(prefix):
    w, h = ctl('ui')['gui_size']
    ctl('scroll-at', w / 2, h / 2, 250)
    for _ in range(80):
        for n in ctl('ui')['widgets']:
            if n.get('enabled') and (n.get('label') or '').startswith(prefix) and 38 < n['bounds'][1] + n['bounds'][3] / 2 < h - 38:
                return n
        ctl('scroll-at', w / 2, h / 2, -6)
    raise AssertionError('missing ' + prefix)

def collect(seconds):
    frames = {}
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        try:
            value = read(hands)
            frames[value['frames']] = value
        except (OSError, json.JSONDecodeError):
            pass
        time.sleep(.01)
    return list(frames.values())

def rotate(pose, v):
    x, y, z = pose[3:6]
    a = [v[0], math.cos(x)*v[1] - math.sin(x)*v[2], math.sin(x)*v[1] + math.cos(x)*v[2]]
    b = [math.cos(y)*a[0] + math.sin(y)*a[2], a[1], -math.sin(y)*a[0] + math.cos(y)*a[2]]
    return [math.cos(z)*b[0] - math.sin(z)*b[1], math.sin(z)*b[0] + math.cos(z)*b[1], b[2]]

try:
    launch()
    ctl('command', 'fill -10 88 -10 10 88 40 stone')
    ctl('command', 'tp @s 0 89 0 0 0')
    observe(1)
    ctl('mouse', 'left', 'down')
    rows = collect(2.5)
    ctl('mouse', 'left', 'up')
    exits = [(before, after) for before, after in zip(rows, rows[1:])
             if before['clip'] == 'new_punch_attack' and after['clip'] is None]
    assert exits, 'punch never returned to idle'
    jump = max(math.dist(before['arm_matrices'][0][12:15], after['arm_matrices'][0][12:15]) for before, after in exits)
    assert jump < .06, jump
    result['punch_return_jump_blocks'] = jump
    result['checks']['bare_hand_punch_returns_without_teleport'] = True
    ctl('mouse', 'left', 'down'); observe(.3)
    for _ in range(3):
        ctl('mouse', 'left', 'up'); observe(.06)
        ctl('mouse', 'left', 'down')
        frames = collect(.1)
        assert frames and min(f['influence'] for f in frames if f['clip']) > .5, frames
    ctl('mouse', 'left', 'up'); observe(1.5)
    result['checks']['interrupted_punch_keeps_tuning_ownership'] = True
    ctl('key', 'v', 'tap'); observe(.8)
    ctl('key', 'w', 'down'); ctl('key', 'ctrl', 'down'); observe(1.2)
    errors = []
    for i in range(8):
        d = observe(.07)
        assert d['grounded'] and d['movement_state'] == 'ground', d
        body = d['body']
        assert body['ground_attachment'] > .99, body
        parts, rest = body['parts'], body['rest']
        assert max(map(abs, rest[1][:6])) < 1e-5, rest
        for index in (4, 5):
            v = [rest[index][k] * parts[1][k+6] for k in range(3)]
            offset = rotate(parts[1], v)
            hip = [parts[1][k] + offset[k] for k in range(3)]
            errors.append(math.dist(parts[index][:3], hip))
        if i == 4:
            ctl('screenshot', a.report.parent / 'sprint-hips-attached.png')
    ctl('release')
    assert max(errors) < .05, errors
    result['sprint_max_hip_gap_pixels'] = max(errors)
    result['checks']['sprint_legs_remain_attached_to_posed_hips'] = True
    ctl('key', 'key.keyboard.f6', 'tap')
    assert observe()['faces']['editor']['editor'], 'F6 did not open face editor'
    result['checks']['f6_opens_face_editor_with_spatial_gui'] = True
    ctl('key', 'escape', 'tap')
    ctl('key', 'key.keyboard.f7', 'tap'); observe()
    assert any(n.get('label') == 'Settings' for n in ctl('ui')['widgets'])
    result['checks']['spatial_gui_f7_settings_remains_available'] = True
    ctl('click', find('Settings')['id'])
    w, h = ctl('ui')['gui_size']; ctl('scroll-at', w/2, h/2, 250)
    nodes = ctl('ui')['widgets']
    caption = next(n for n in nodes if n.get('label') == 'Open face editor' and not n['enabled'])
    key = next(n for n in nodes if n['enabled'] and n['bounds'][0] > caption['bounds'][0]
               and abs(n['bounds'][1] - caption['bounds'][1]) < 1)
    assert key['label'] == 'F6', key
    ctl('click', key['id']); ctl('key', 'g', 'tap'); play()
    ctl('key', 'key.keyboard.f6', 'tap')
    assert observe()['faces']['editor'] is None
    ctl('key', 'g', 'tap')
    assert observe()['faces']['editor']['editor']
    result['checks']['face_editor_shortcut_is_rebindable'] = True
    ctl('key', 'escape', 'tap'); play(); ctl('quit'); process.wait(timeout=45)
    launch()
    ctl('key', 'g', 'tap')
    assert observe()['faces']['editor']['editor']
    result['checks']['face_editor_keybind_survives_restart'] = True
    ctl('quit'); process.wait(timeout=45)
    result['success'] = True
except Exception as error:
    result.update(success=False, error=str(error), traceback=traceback.format_exc())
    if process is not None and process.poll() is None:
        try:
            result['failure_ui'] = ctl('ui')
            ctl('screenshot', a.report.with_suffix('.failure.png'))
        except Exception:
            pass
finally:
    if process is not None and process.poll() is None:
        process.terminate()
        try: process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill(); process.wait()
    a.report.write_text(json.dumps(result, indent=2) + '\n')
    log.close()
print(json.dumps(result, indent=2))
raise SystemExit(0 if result['success'] else 1)
