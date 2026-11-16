# Changelog

## 0.2.0

- Add native Rust Moves body animations and shoulder/orbit camera, with separate
  master, body, camera and facial switches and external Fluid & Foolish support.
- Add layered facial animation, custom shapes/looks/reactions, per-eye glow,
  visual lights and trails, and an in-game editor with undo/redo, saved profiles
  and explicit PNG export.
- Prevent vanilla swimming rotation from being applied over authored swim poses.
- Make equipped elytra follow the animated torso and root while walking.
- Preserve Spatial GUI camera/pointer ownership and add right-click decrease,
  reverse selection and back navigation in settings.
- Target Horizon nightly `9f42064` (Minecraft 26.3). See VALIDATION.md for the
  tested scope and PORTING_STATUS.md for remaining parity limitations.

## 0.1.8

- In-Game Live HUD Editor (F9):
  - On-screen translucent card overlay rendered via `native_gui_render_post`.
  - Live adjustment of camera offsets and first-person viewmodels using `Tab`, `↑`/`↓`, `←`/`→`, and `Backspace`/`R`.
  - Immediate visual feedback directly in the world while moving, looking around, or viewing character in third-person.
- Overhauled Settings UI:
  - All 4 parameters in each category visible at once with dedicated `[-]` and `[+]` stepping buttons and exact numerical feedback.
  - Category switching between Moves Camera, Arms Viewmodel, and Held Items.
  - Section-specific and global reset buttons.
- Horizon Native Build Compatibility:
  - Wrapped helper function definitions in `camera.rs`, `fov.rs`, and `gui.rs` conforming to the latest horizon-mixins syntax.
  - Added `native_gui_render_post` mixin injection (14 total injections).
  - Cleaned up unused fields for zero compiler warnings.

## 0.1.7

- Integrate Punchy! Moves third-person dynamic shoulder camera:
  - Over-the-shoulder offset with horizontal, vertical, depth, and FOV adjustments.
  - Multi-sample obstacle collision raycast preventing camera clipping through blocks.
  - Free / orbit camera mode with F8 key toggle and mouse rotation handling.
  - Stationary turn-in-place easing with body yaw smoothing and 50-degree head constraint.
  - Humanoid locomotion and pose interpolation data structures.
  - Injections for `native_camera_pose`, `native_compute_fov`, and `turn_player`.

## 0.1.6

- Add independent first-person arm and item X/Y/Z position and scale settings.
- Save adjustments between sessions; reset either group or all adjustments.
- Apply adjustments to animated and Horizon hand poses, including map geometry.

## 0.1.5

- Update to Horizon Nightly 2026-11-05 (`66f3981`).
- Adapt first-person geometry to the hand/item shader ID contract.
- Preserve current hand-pose hooks, shield sizing and held-item resolution.
- Validate the new pin with a full native build and 12 fresh graphical checks.

## 0.1.4

- Simplify the project page around features, installation and compatibility.
- Remove personal maintainer branding from the project description and documentation.

## 0.1.3

- Extract the native port into a portable standalone source repository.
- Run engine tests using embedded resources and checked-in numerical fixtures.
- Package the native bundle with the pinned Horizon validator from an explicit source checkout.
- Retain the behavior of the locally tested 0.1.2 runtime.

## 0.1.2 (local runtime baseline)

- Connect consumption and mining start-loop-end controllers.
- Track successful item/block/entity uses and creative instant breaks.
- Keep live outgoing animation poses during interruptions.
- Respect declared loops, continuous expression clocks and timeline hand visibility.
- Keep camera/body animation shared across single-arm actions.
