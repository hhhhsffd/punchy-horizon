# Moves port status

Native Rust work in progress, targeting PunchyMoves/Expressions behavior. No
Java runtime is used. Original first-person Punchy resources and notices are
retained; Fluid & Foolish remains an external, user-installed resource pack.
**Exact behavioral/visual parity is not established.**

## Current implementation

- External version-2 configuration and Bedrock animation loading, compatibility
  overrides, handed/one-handed clips, variants, overlays, filters and retention.
- Native `BodyPose` composition starting from Horizon's actual limb poses,
  root/waist transforms, item offsets, and preservation of foreign pose owners.
- Ground, directional/crouched movement, jump/fall/landing, crawl, climb,
  swimming, creative flight, elytra and passenger selection.
- World-backed fluid/depth/trajectory probes; shallow-water head-clearance
  latching, airborne surface grace, swim clip phase continuity and playback.
- Accepted use captures retain the item before consumption/replacement.
  Accepted attack observation separates primary actions from mining/use swings.
  Shield/swap statuses, equipment changes and other action signals feed the
  controller rather than relying solely on raw mouse presses.
- In-game facial adapter: original skin retention, separate avatar aliases,
  checked atlas placement, source restoration, resource/world cleanup, and
  per-frame animation using model age plus the game's partial-tick clocks.
- Facial observations: movement event ages and weights from the body controller,
  sampled external `face_*` channels, accepted interactions, use/release, fishing,
  level changes, status effects, vehicles, weather, biome and nearby-entity cues.
- Facial blink/gaze/reaction arbitration and easing, settings redetection that
  retains marks, remote embedded appearance, and a native full-bright skin pass.
  Facial avatars use the native CPU body path to preserve pass ordering;
  avatars without facial geometry retain GPU skinning.
- Fractional, seam-split face geometry on the posed head, separate eye/iris/
  glint/lid depths, hat-glint movement, unclipped no-white-eye translation,
  blink masking of colored shapes, and stable skin backings without duplicate
  animated raster pixels underneath the mesh.
- External `faces/shapes`, `faces/looks`, and `faces/triggers.json` loading,
  multicolor/mirrored grids, inherited looks, coordinate conversion and
  configurable reaction priorities. User style overrides take precedence.
- Head-attached emissive glow, procedural bloom/glare, world-space trail/tracer
  history and native visual colored lights. Lights have no gameplay effects.
- Native face editor in the Punchy options page: painting/front/side marks,
  undo/redo, live preview, style/color/per-eye glow controls, reaction/priority
  editing, named-look editing and custom shape drawing. Saves persist marks
  and profiles; explicit exports write PNG skins or text marks to
  `punchy/exports` in the active player directory. Escape/Back discards unsaved
  edits. The editor draws inside Spatial GUI's captured options layer.
- Suppression of duplicate vanilla swimming root rotation only while a Moves
  body pose owns an authored root; live entity physics are untouched.
- Elytra wings inherit the animated torso/root delta through the native
  equipment renderer, addressing the reported fixed wings while the body walks.
  Gait/equipment diagnostics support the future regression scenario.
- Shoulder/orbit camera, settings/presets, collision clipping, activation/exit,
  F5 override, spyglass handoff, deferred perspective/look updates, FOV, and
  local-avatar visibility/fading hooks.
- Separate Moves/body/camera/framing/face switches, persistent settings,
  backwards right-click adjustments and back navigation. No custom emote wheel.
- Spatial GUI menu/pointer/camera ownership guards; no F6–F8 default conflicts.

The pinned Windows GNU combined build and engine/pack tests pass. In-game
regression checks cover the facial editor, swimming rotation and wing attachment.
See VALIDATION.md for the precise tested scope and prebuilt artifact details.

## Remaining implementation/parity boundaries

- Complete compatibility semantics, every original sequence transition,
  retrigger/overlay interaction, numerical tuning and third-party override.
- Passenger/rowing IK and exact third-person grips, all equipment/render layers,
  custom models, large/scaled avatars and every native-mod interaction.
- Original-artwork matching and detailed visual comparison of special-eye
  shapes/marks, layered head geometry, bloom/glare/trails and the native editor
  remain unverified. Procedural effects do not bundle original artwork.
- Remote data is limited to what Horizon receives; original emote networking,
  remote fishing-bite state and studio integration are not reproduced.
- Camera-center targeting has a solver and adapter code, but its picking helper
  hooks are deliberately not registered: changing only block selection and
  pick-block would disagree with Horizon's separate attack/use entity queries.
  Gameplay still uses Horizon's player-eye aim and reach.
- Detailed in-game behavioral/visual comparison against the original, beyond
  the automated smoke scenarios.

The user authorized in-game testing and a GitHub update on 2026-11-15 after the
implementation hold. Current results are recorded in VALIDATION.md.
Accepting all clips is not proof that every original behavior selects and
composes those clips identically.
