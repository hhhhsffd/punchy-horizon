# Punchy + Moves for Horizon (Rust fork)

A native animation mod for [Horizon](https://github.com/horizon-rust/horizon)
(Minecraft Java 26.3 in Rust), bringing Punchy-style first-person hand and item
animations to the client.

This fork adds a data-driven third-person Moves implementation in Rust.
It is **not yet a complete, verified conversion of every original PunchyMoves
feature**; see [PORTING_STATUS.md](https://github.com/hhhhsffd/punchy-horizon/blob/main/PORTING_STATUS.md). No Java code, JAR, JVM,
Fabric, or bundled Fluid & Foolish assets are used by the new integration.

## Features

- **Combat animations** for attacks and shield use.
- **Mining animations** with start, loop and end phases, including creative instant breaks.
- **Eating and drinking** with complete start-loop-end sequences.
- **Item inspection** and animated interactions such as shovel use.
- **Smooth transitions** that keep the outgoing animation moving when an action changes.
- **Adjustable viewmodel**: separate arm and item position (X/Y/Z) and scale.
- **Punchy! Moves third-person shoulder camera**: over-the-shoulder dynamic camera with obstacle ray clipping, configurable distance, height, shoulder horizontal offset, and FOV.
- **In-Game Live HUD Editor (F9)**: real-time on-screen editor overlay rendered directly in your world. Adjust camera and arm viewmodels live while seeing your character move!
- **Free/orbit camera mode (F4)**: orbit without turning the player, only during third-person gameplay.
- **Stationary turn-in-place easing**: body yaw smoothing and 50-degree head rotation constraint matching Punchy! Moves locomotion.
- **Self-contained resources**: 223 bundled clips, geometry and tuning; no separate Punchy JAR or Fabric install.
- **External Moves packs**: reads `assets/punchy_moves/animations/**/*.json`
  and `compat/**/*.json` from Horizon's selected/resolved resource packs.
- **Moves master switch**: disable Moves without losing first-person Punchy;
  separate switches for body animations, shoulder camera, and dynamic framing.
- **Pack-driven poses**: directional walking/crawling, sprint overlays, jump/fall,
  swimming, elytra, climbing, riding, item actions, emotes, and contextual camera presets.
- **In-game facial animation (work in progress)**: per-player animated skin aliases,
  blink/gaze/reaction channels, external clip face channels, and an emissive face
  pass. Original skin pixels remain untouched; remote skins with embedded facial
  metadata retain their owner's style. Exact visual parity is not established.
- **Face editor and rendering**: fractional face layers,
  external custom shapes/looks/reactions, per-eye glow effects and colored
  visual lights, and an integrated face editor with persistent marks, editable
  looks/shapes and explicit PNG export. Open **Open face editor** from the
  native settings page.
- **Elytra wing attachment**: wings follow the animated torso
  and root while walking, through the native wing equipment renderer.
- **Swimming rotation fix**: authored swim poses suppress the duplicate vanilla
  outer rotation while retaining normal physics and water-exit behavior.
- **Spatial GUI coexistence**: leaves menu camera/FOV and pointer mapping alone,
  releases body poses in menus, preserves other emote owners, and avoids F6–F8.

Settings: **Options > Mods > Punchy + Moves**. Hand poses default to **Punchy**.
- **F10**: inspect held item
- **F12**: reload animations and external Moves resources
- **F4**: toggle Moves free/orbit camera mode
- **F9**: toggle In-Game Live HUD Editor
- **V**: toggle the shoulder camera and switch perspective
- **M / N**: select next / play selected external Moves emote

The custom emote wheel is intentionally out of scope. Bindings are exposed on
the native mod's settings page. Rebind any old
F6/F7 assignments inherited from an earlier installation to avoid Spatial GUI.

## In-Game Live HUD Editor (F9)

Press **F9** while playing in any world or server to bring up the live interactive editor:
- **`Tab`**: switch between **Moves 3rd-Person Camera**, **1st-Person Arms Viewmodel**, and **1st-Person Held Items**.
- **`↑` / `↓`** (or `Page Up` / `Page Down`): select which parameter to adjust.
- **`←` / `→`** (or `-` / `=`, `[` / `]`): adjust the parameter in real time. The camera and arms move right before your eyes!
- **`Backspace`**: reset the selected parameter to its default.
- **`R`**: reset all parameters in the current category to defaults.
- **`F9`**: close the editor.

## Overhauled Settings Menu

Open **Options > Mods > Punchy + Moves > Settings** to view and configure all settings with full visibility:
- **Dedicated `[-]` / `[+]` stepping buttons**: every parameter shows its name, exact current value, and step delta (e.g. `[-] Horizontal: +1.17 blk (-0.10)` / `[+] Horizontal: +1.17 blk (+0.10)`).
- **Active Section switcher**: click to cycle between **Moves 3rd-Person Camera**, **Arms Viewmodel**, and **Held Items Viewmodel**. All 4 parameters for the active section are displayed simultaneously.
- **Reset buttons**: reset individual sections or all mod settings to defaults with one click.
- **Master toggles**: easily toggle preview, in-game live HUD editor, shoulder camera, free orbit mode, and hand pose styles.
- **Right-click numbers** to decrease (on either `[-]` or `[+]`); right-click
  speed, section, or emote selection to cycle backwards. Toggles work in either
  direction. Right-click empty space or the footer to go back. Destructive
  reset/reload/play actions are not activated by right-click. Key capture still
  accepts right mouse bindings. Values save through Horizon's native settings.

## Installation

0.2.0 is a **work-in-progress preview**, not complete original-mod parity.
The pinned Windows GNU combined build and both isolated runtime smoke suites
passed; see VALIDATION.md for the precise scope. Build the bundle from source
using DEVELOPMENT.md if no downloadable release is available.

1. Use `dist/punchy-rust-preview-0.2.0.hnmod` from this fork.
2. Add it to your Horizon instance's native mods and enable Punchy.
3. Build the game with native mods in the launcher, then launch it.
4. Download [Fluid & Foolish 1.0.1](https://modrinth.com/resourcepack/fluid-foolish)
   yourself, put the ZIP in the instance's `resourcepacks` folder, and enable it
   in **Options > Resource Packs**. Normal pack reloads refresh Moves automatically.
   The mod never downloads, embeds, or enables this pack for you.

Without the external pack, first-person Punchy and the shoulder camera still
work; Moves animations have no external clips and show a missing-pack message.

Horizon still prepares its own Minecraft resources from your Minecraft installation.
Native mods run with the same access as the game.

## Compatibility and development

Targets **`nightly-2026-11-13-9f42064`**, the latest published game nightly
checked on 2026-11-16, Minecraft 26.3. Horizon main has newer, unpublished
commits; “latest” here means the launcher's published game, not main.
Pins: Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0`,
mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`.
Spatial GUI reference: **0.2.3** on the same pins.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

See [DEVELOPMENT.md](DEVELOPMENT.md) for packaging and [VALIDATION.md](VALIDATION.md)
for checks actually performed. Neither compiling nor reading all clips proves
full original-mod behavioral or visual parity.

## Credits and license

Community port based on [Punchy](https://modrinth.com/mod/punchy-fpa) by Dev Punchy Man.
Includes adapted implementation and original animation data. This is an unofficial project.
Original notices are retained in [LICENSE-Punchy](LICENSE-Punchy) and
[LICENSE-Horizon](LICENSE-Horizon); see [LICENSE](LICENSE) and
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
