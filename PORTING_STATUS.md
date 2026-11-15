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
  Glowing avatars use the native CPU body path to preserve pass ordering;
  ordinary avatars retain GPU skinning.
- Shoulder/orbit camera, settings/presets, collision clipping, activation/exit,
  F5 override, spyglass handoff, deferred perspective/look updates, FOV, and
  local-avatar visibility/fading hooks.
- Separate Moves/body/camera/framing/face switches, persistent settings,
  backwards right-click adjustments and back navigation. No custom emote wheel.
- Spatial GUI menu/pointer/camera ownership guards; no F6–F8 default conflicts.

These are implementation paths, **not current build/runtime validation claims**.
See VALIDATION.md for the current test results and earlier baseline.

## Remaining implementation/parity boundaries

- Complete compatibility semantics, every original sequence transition,
  retrigger/overlay interaction, numerical tuning and third-party override.
- Passenger/rowing IK and exact third-person grips, all equipment/render layers,
  custom models, large/scaled avatars and every native-mod interaction.
- Procedural special-eye shapes/marks and rasterized eyes do not reproduce all
  original artwork or layered depth effects. No-white eye translation remains
  clipped to its original raster box. Custom pack facial shapes/reactions,
  glow anchors/dynamic lights, and a fully integrated face editor are incomplete.
- Remote data is limited to what Horizon receives; original emote networking,
  remote fishing-bite state and studio integration are not reproduced.
- Camera-center targeting has a solver and adapter code, but its picking helper
  hooks are deliberately not registered: changing only block selection and
  pick-block would disagree with Horizon's separate attack/use entity queries.
  Gameplay still uses Horizon's player-eye aim and reach.
- Full current combined compilation and in-game behavioral/visual comparison.

Testing resumed on 2026-11-15 at the user's request; remaining checks are recorded
in VALIDATION.md.
Accepting all clips is not proof that every original behavior selects and
composes those clips identically.
