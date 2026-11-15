use horizon_client::mobs::anim::punchy_moves::engine as punchy_engine;
// Game adapter for the frame-based Moves camera. UI cameras retain ownership.
use super::{Game, Screen, perspective::CameraType};
use glam::{DVec3, IVec3};
use horizon_core::{
    geometry::{Vec3, VoxelShape},
    position::BlockPos,
};
use horizon_world::{
    collision_context::CollisionContext,
    registry::BlockStateId,
    world_ray::{RayBlockMode, RayFluidMode, RayWorld, clip_world},
};
use punchy_engine::moves_camera::{self as camera, CameraWorld};
use std::{cell::RefCell, time::Instant};

pub(super) use super::punchy_camera_settings::{settings_buttons, settings_click};

#[derive(Default)]
struct State {
    controller: camera::CameraController,
    input: camera::CameraInput,
    at: Option<Instant>,
    orbit: bool,
    output: Option<camera::CameraOutput>,
    look: Option<camera::LookAngles>,
    command: Option<camera::CameraCommand>,
    perspective: Option<camera::Perspective>,
    persist_enabled: Option<bool>,
    observed_enabled: Option<bool>,
    targeting: camera::CameraTargeting,
}
thread_local! {static STATE: RefCell<State> = RefCell::new(State::default());}

pub(super) fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::default());
}
pub(super) fn perspective_changed() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.look = s.controller.on_vanilla_perspective_change().or(s.look);
        s.perspective = None;
        s.command = None;
        s.orbit = false;
        s.output = None;
        s.targeting.clear();
    });
}
pub(super) fn toggle() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.command = match s.command {
            Some(camera::CameraCommand::Toggle) => None,
            Some(camera::CameraCommand::SetEnabled(value)) => {
                Some(camera::CameraCommand::SetEnabled(!value))
            }
            None => Some(camera::CameraCommand::Toggle),
        };
    });
}
/// Settings callbacks only have an immutable Game. The mutable frame hook owns
/// perspective/look handoffs; a settings screen never snaps the camera itself.
pub(super) fn settings_changed(game: &Game<'_>) {
    let enabled = camera::CameraSettings::read(|key| game.native_setting("punchy", key)).enabled;
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.observed_enabled.is_some_and(|old| old != enabled) {
            s.command = Some(camera::CameraCommand::SetEnabled(enabled));
        }
        s.observed_enabled = Some(enabled);
    });
}
pub(super) fn toggle_orbit() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.orbit = !s.orbit;
    });
}
pub(super) fn orbiting() -> bool {
    STATE.with(|s| s.borrow().controller.orbit_angles().is_some())
}
pub(super) fn hides_avatar(game: &Game<'_>) -> bool {
    owns_frame(game)
        && STATE.with(|s| {
            s.borrow()
                .output
                .as_ref()
                .is_some_and(|o| o.solved && (o.hide_local_player || o.avatar.skip_avatar))
        })
}
pub(super) fn diagnostic() -> serde_json::Value {
    STATE.with(|s|s.borrow().output.map_or(serde_json::Value::Null, |o|serde_json::json!({
        "active":o.active,"solved":o.solved,"position":o.position.to_array(),"yaw":o.rotation.yaw,"pitch":o.rotation.pitch,
        "orbit":o.orbit_active,"map":o.map_view_active,"activation_blend":o.activation_blend,
        "avatar_alpha":o.avatar.alpha,"fov":o.values.fov,"collision_fraction":o.collision_fraction
    })))
}
pub(super) fn turn(game: &Game<'_>, x: f64, y: f64) -> bool {
    if !owns_frame(game) {
        return false;
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let input = s.input;
        s.controller.turn_free_camera(&input, x as f32, y as f32)
    })
}
pub(super) fn fov(game: &Game<'_>, degrees: &mut f32) {
    if !owns_frame(game) {
        return;
    }
    STATE.with(|s| {
        if let Some(output) = s.borrow().output.as_ref().filter(|o| o.solved) {
            *degrees *= output.fov_multiplier;
        }
    });
}
pub(super) fn frame(game: &mut Game<'_>) {
    if !super::punchy_moves::enabled(game) || game.player.is_none() {
        reset();
        return;
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(enabled) = s.persist_enabled.take() {
            game.native_set_setting(
                "punchy",
                "moves_camera_enabled",
                if enabled { "true" } else { "false" },
            );
            s.observed_enabled = Some(enabled);
        }
    });
    settings_changed(game);
    if !available(game) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.at = Some(Instant::now());
            s.output = None;
            s.targeting.clear();
        });
        return;
    }
    let (look, perspective) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        (s.look.take(), s.perspective.take())
    });
    if let Some(look) = look {
        game.yaw = look.yaw;
        game.pitch = look.pitch;
        game.rope_rot_old = (look.yaw, look.pitch);
        if let Some(model) = game.player_model.as_mut() {
            model.head_yaw = look.yaw;
            model.previous_head_yaw = look.yaw;
            model.head_pitch = look.pitch;
            model.previous_head_pitch = look.pitch;
            model.yaw = look.yaw;
            model.previous_yaw = look.yaw;
        }
    }
    if let Some(perspective) = perspective {
        let next = match perspective {
            camera::Perspective::FirstPerson => CameraType::FirstPerson,
            camera::Perspective::ThirdPersonBack => CameraType::ThirdPersonBack,
            camera::Perspective::ThirdPersonFront => CameraType::ThirdPersonFront,
        };
        if game.camera_type.first_person() != next.first_person() {
            game.check_entity_post_effect(next.first_person());
        }
        game.camera_type = next;
    }
}

fn available(game: &Game<'_>) -> bool {
    super::punchy_moves::enabled(game)
        && game
            .player
            .as_ref()
            .is_some_and(|p| !p.health.dead_or_dying())
        && !game.spectator()
        && game.sleeping.is_none()
        && !game.studio.flying()
        && game.camera_entity().is_none()
        && game.screen == Screen::Playing
        && !game.screen_over_playing()
        && game.captured
}

// Unlike camera_allowed, this must also run in first person for a queued V
// command or a returning spyglass. A rear-view foreign camera retains its lease
// through its closing animation and must not inherit our FOV/avatar state.
fn owns_frame(game: &Game<'_>) -> bool {
    available(game)
        && (game.camera_type != CameraType::ThirdPersonBack
            || super::punchy_moves::camera_allowed(game))
}

fn profile(value: &serde_json::Value) -> Option<camera::CameraProfile> {
    use punchy_engine::moves_animation::number;
    let value = value.get("camera").unwrap_or(value);
    let p = camera::CameraProfile {
        horizontal: number(&value["horizontal"], f32::NAN),
        vertical: number(&value["vertical"], f32::NAN),
        depth: number(&value["depth"], f32::NAN),
        fov: number(&value["fov"], f32::NAN),
        response: number(&value["response"], 5.),
    };
    p.is_complete().then_some(p)
}

pub(super) fn pose(
    game: &Game<'_>,
    partial: f32,
    position: &mut [f64; 3],
    yaw: &mut f32,
    pitch: &mut f32,
) {
    if !owns_frame(game) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.at = Some(Instant::now());
            s.output = None;
            s.targeting.clear();
            s.controller.reset_dynamic_transitions();
        });
        return;
    }
    let Some(p) = &game.player else {
        return;
    };
    let pack = super::punchy_moves::pack(game);
    let at = DVec3::new(p.position.x, p.position.y, p.position.z);
    let old = DVec3::new(
        game.previous_position.x,
        game.previous_position.y,
        game.previous_position.z,
    );
    let feet = old.lerp(at, f64::from(partial.clamp(0., 1.)));
    let eye = feet + DVec3::Y * f64::from(p.dimensions.eye_height);
    let half = f64::from(p.dimensions.width) / 2.;
    let held = game.held().map_or("", |s| &*s.item);
    let used = game.used_hand().and_then(|hand| {
        if hand == horizon_client::first_person::Hand::Off {
            game.offhand.as_ref()
        } else {
            game.held()
        }
    });
    let using = used.map_or("", |stack| &*stack.item);
    let below = game.world.block(
        at.x.floor() as i32,
        at.y.floor() as i32 - 1,
        at.z.floor() as i32,
    );
    let mut settings = camera::CameraSettings::read(|key| game.native_setting("punchy", key));
    settings.enabled &= super::punchy_moves::enabled(game);
    let dynamic = game
        .native_setting("punchy", "moves_dynamic_framing")
        .as_deref()
        != Some("false");
    if !dynamic {
        settings.fence_camera = false;
        settings.proximity_camera = false;
        settings.elytra_camera = false;
        settings.ladder_camera = false;
        settings.map_camera = false;
        settings.bow_and_crossbow_camera_fix = false;
        settings.spear_and_trident_camera_fix = false;
    }
    let map_item = if held == "minecraft:filled_map" {
        held
    } else {
        game.offhand.as_ref().map_or("", |stack| &*stack.item)
    };
    let map = pack.entry_for_item(map_item, "map_view");
    let number = punchy_engine::moves_animation::number;
    let map_pitch = |key: &str, field: &str, fallback| {
        game.native_setting("punchy", key)
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or_else(|| number(&map[field], fallback))
            .clamp(-90., 90.)
    };
    let proximity = &pack.config["cameras"]["proximity"];
    let passenger = pack.entry("GENERAL", "passenger_state");
    let vehicle = game.riding.and_then(|id| {
        game.mobs
            .iter()
            .find(|m| m.id == id)
            .map(|m| &*m.kind.id)
            .or_else(|| {
                game.boats
                    .iter()
                    .find(|b| b.id == id)
                    .map(|_| "minecraft:boat")
            })
            .or_else(|| {
                game.carts
                    .iter()
                    .find(|c| c.id == id)
                    .map(|_| "minecraft:minecart")
            })
    });
    let passenger = vehicle
        .filter(|vehicle| {
            passenger["entities"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(*vehicle)))
        })
        .and_then(|_| profile(&passenger))
        .filter(|_| dynamic);
    let entry = |name| pack.entry("GENERAL", name);
    let profiles = if dynamic {
        camera::CameraProfiles {
            fence: profile(&entry("fence")),
            proximity: profile(&pack.config["cameras"]["proximity"]),
            elytra: profile(&entry("elytra")),
            climbing: profile(&entry("climbing")),
            ranged: profile(&pack.config["cameras"]["aiming"]),
            held_throwable: profile(&pack.entry_for_item(held, "throwable_camera")),
            map: profile(&map),
            timed_throwable: None,
        }
    } else {
        camera::CameraProfiles::default()
    };
    // Pick can call back into camera targeting; never hold STATE while doing it.
    let mining_target = game.keys.held(super::Mapping::Attack) && game.pick().is_some();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let now = Instant::now();
        let elapsed =
            s.at.replace(now)
                .map_or(0.05, |last| now.duration_since(last).as_secs_f32());
        let input = camera::CameraInput {
            elapsed_seconds: elapsed,
            perspective: match game.camera_type {
                CameraType::FirstPerson => camera::Perspective::FirstPerson,
                CameraType::ThirdPersonBack => camera::Perspective::ThirdPersonBack,
                CameraType::ThirdPersonFront => camera::Perspective::ThirdPersonFront,
            },
            player_present: true,
            focused_entity_present: true,
            detached: true,
            local_player_view: true,
            command: s.command.take(),
            toggle_orbit: std::mem::take(&mut s.orbit),
            yaw: *yaw,
            pitch: *pitch,
            player_look: Some(camera::LookAngles {
                yaw: game.yaw,
                pitch: game.pitch,
            }),
            // Camera already eases eye height (crouch/swim/scale). Reach uses
            // the actual eye below, never this animated/render-only pivot.
            pivot: DVec3::from_array(game.view.eye),
            eye,
            bounds_min: feet - DVec3::new(half, 0., half),
            bounds_max: feet + DVec3::new(half, f64::from(p.dimensions.height), half),
            entity_width: f64::from(p.dimensions.width),
            entity_height: f64::from(p.dimensions.height),
            eye_height: f64::from(p.dimensions.eye_height),
            block_reach: game.block_reach,
            vanilla_fov: game.settings.fov,
            using_spyglass: using == "minecraft:spyglass",
            grounded: p.ground.on_ground,
            supported_by_fence: game.res.facts().get(below).name.ends_with("_fence"),
            mining_target,
            sprinting: p.sprinting,
            fall_flying: p.fall_flying,
            climbing: p.on_climbable,
            swimming: p.swimming,
            riding: game.riding.is_some(),
            passenger,
            holding_filled_map: held == "minecraft:filled_map"
                || game
                    .offhand
                    .as_ref()
                    .is_some_and(|s| &*s.item == "minecraft:filled_map"),
            using_bow: using == "minecraft:bow",
            holding_charged_crossbow: [game.held(), game.offhand.as_ref()]
                .into_iter()
                .flatten()
                .any(|stack| {
                    &*stack.item == "minecraft:crossbow"
                        && stack.data().is_some_and(|d| !d.charged.is_empty())
                }),
            charging_spear_or_trident: using.ends_with("_spear") || using == "minecraft:trident",
            holding_throwable: matches!(
                held,
                "minecraft:egg"
                    | "minecraft:snowball"
                    | "minecraft:ender_pearl"
                    | "minecraft:splash_potion"
                    | "minecraft:lingering_potion"
            ),
            map_enter_pitch: map_pitch("moves_camera_map_enter", "look_down_enter", 42.),
            map_exit_pitch: map_pitch("moves_camera_map_exit", "look_down_exit", 32.),
            proximity: camera::ProximitySettings {
                instant_distance: number(&proximity["instant_distance"], 1.),
                enter_distance: number(&proximity["enter_distance"], 2.2),
                mining_hold_seconds: number(&proximity["mining_hold_seconds"], 2.),
                blend_in_response: number(&proximity["blend_in_response"], 5.),
                blend_out_response: number(&proximity["blend_out_response"], 3.2),
            },
            profiles,
            ..Default::default()
        };
        let output = s.controller.update(&input, &mut settings, &mut World(game));
        if output.solved {
            *position = output.position.to_array();
            *yaw = output.rotation.yaw;
            *pitch = output.rotation.pitch;
        }
        s.input = input;
        s.look = output.apply_player_look.or(s.look);
        s.perspective = output.requested_perspective.or(s.perspective);
        if output.settings_changed {
            s.persist_enabled = Some(settings.enabled);
            s.observed_enabled = Some(settings.enabled);
        }
        s.output = Some(output);
    });
    capture_frame(game, partial, *position, *yaw, *pitch);
}

fn identity(game: &Game<'_>) -> camera::FrameIdentity {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    game.started.hash(&mut hash);
    game.dim.id().hash(&mut hash);
    camera::FrameIdentity {
        world: hash.finish(),
        player: game.remote.as_ref().map_or(0, |r| r.entity_id) as u64,
    }
}

fn capture_frame(game: &Game<'_>, partial: f32, position: [f64; 3], yaw: f32, pitch: f32) {
    let enabled = STATE.with(|s| s.borrow().output.is_some_and(|o| o.solved && o.active));
    let Some(renderer) = game.renderer.as_ref() else {
        return;
    };
    // Same reverse-Z projection and effect matrices as Game's displayed frame,
    // captured before block-outline extraction, not from last frame's HUD.
    let fov = game.fov_degrees(partial, position);
    let far = horizon_client::camera::depth_far(game.terrain_distance(), game.settings.cloud_range);
    let projection = glam::Mat4::perspective_rh(
        fov.to_radians(),
        renderer.config.width as f32 / renderer.config.height.max(1) as f32,
        far,
        0.05,
    );
    let view = game.bob_matrix(partial)
        * game.spinning_matrix(partial)
        * horizon_client::camera::view_matrix(yaw, pitch);
    STATE.with(|s| {
        s.borrow_mut().targeting.capture(
            enabled,
            identity(game),
            DVec3::from_array(position),
            view,
            projection,
        );
    });
}

/// Some(Miss) is authoritative. None alone means vanilla retains ownership.
#[allow(
    dead_code,
    reason = "Center picking is not registered until all native interaction queries agree"
)]
fn target(game: &Game<'_>) -> Option<camera::TargetHit> {
    if !owns_frame(game) || game.forced_hit.is_some() {
        return None;
    }
    let eye = DVec3::from_array(game.eye_position()?);
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let output = s.output.filter(|o| o.solved && o.active)?;
        let input = camera::PickInput {
            enabled: true,
            identity: identity(game),
            camera_position: output.position,
            camera_forward: output.rotation.basis().0,
            eye,
            block_reach: game.block_reach,
            entity_reach: game.entity_reach,
        };
        s.targeting.pick(&input, &mut World(game))
    })
}

#[allow(dead_code, reason = "Reserved for the matching center-picking helper")]
pub(super) fn pick_block(game: &Game<'_>) -> Option<Option<super::Hit>> {
    target(game).map(|hit| match hit {
        camera::TargetHit::Block(hit) => Some(super::Hit {
            pos: hit.block.to_array(),
            face: hit.normal.to_array(),
            point: hit.position.to_array(),
        }),
        _ => None,
    })
}

#[allow(dead_code, reason = "Reserved for the matching center-picking helper")]
pub(super) fn pick_entity(game: &Game<'_>) -> Option<Option<(f64, Option<i32>)>> {
    target(game).map(|hit| match hit {
        camera::TargetHit::Entity(hit) => {
            let distance = STATE.with(|s| {
                s.borrow()
                    .output
                    .map_or(0., |o| o.position.distance(hit.position))
            });
            let id = hit.entity as u32 as i32;
            Some((
                distance,
                (!game.interactions.contains_key(&id)).then_some(id),
            ))
        }
        _ => None,
    })
}

thread_local! {static AVATAR_PASS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };}
pub(super) fn avatar_geometry(
    game: &Game<'_>,
    camera: DVec3,
    partial: f64,
    vertices: &mut Vec<horizon_client::mesher::Vertex>,
) -> std::ops::ControlFlow<()> {
    if !owns_frame(game) {
        return std::ops::ControlFlow::Continue(());
    }
    let visibility = STATE.with(|s| s.borrow().output.filter(|o| o.solved).map(|o| o.avatar));
    let Some(visibility) = visibility.filter(|v| v.is_translucent()) else {
        return std::ops::ControlFlow::Continue(());
    };
    if AVATAR_PASS.with(|flag| flag.replace(true)) {
        return std::ops::ControlFlow::Continue(());
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            AVATAR_PASS.with(|flag| flag.set(false));
        }
    }
    let _guard = Guard;
    let start = vertices.len();
    game.player_model_geometry(camera, partial, vertices);
    // Vertex's RGBA8 uses the same high-byte alpha as ARGB; RGB is untouched.
    // Only vertices emitted by this local-avatar call (all its layers) change.
    for vertex in &mut vertices[start..] {
        vertex.color = visibility.apply_argb(vertex.color);
        vertex.layer |= horizon_client::gfx::OIT_ENTITY;
    }
    std::ops::ControlFlow::Break(())
}

struct World<'a, 'r>(&'a Game<'r>);
impl CameraWorld for World<'_, '_> {
    fn pick_entity(
        &mut self,
        from: DVec3,
        to: DVec3,
        max_distance_squared: f64,
    ) -> Option<camera::EntityHit> {
        let delta = to - from;
        let direction = delta.try_normalize()?;
        let reach = delta.length().min(max_distance_squared.max(0.).sqrt());
        let game = self.0;
        let mut best: Option<camera::EntityHit> = None;
        let mut offer = |id: u32, bounds: horizon_core::geometry::Aabb| {
            if Some(id) == game.riding || Some(id) == game.remote_camera_id() {
                return;
            }
            if let Some(distance) =
                super::ray_box(from.to_array(), direction.to_array(), reach, bounds)
            {
                let position = from + direction * distance;
                if best.is_none_or(|old| {
                    from.distance_squared(position) < from.distance_squared(old.position)
                }) {
                    best = Some(camera::EntityHit {
                        position,
                        entity: u64::from(id),
                    });
                }
            }
        };
        for mob in game.mobs.iter().filter(|m| !m.removed) {
            offer(mob.id, mob.bounds());
        }
        // Use Horizon's own interaction boxes rather than parallel geometry.
        let ids = game
            .boats
            .iter()
            .map(|v| v.id)
            .chain(game.carts.iter().map(|v| v.id))
            .chain(game.armor_stands.iter().map(|v| v.id))
            .chain(game.cushions.iter().map(|v| v.id))
            .chain(game.item_frames.iter().map(|v| v.id))
            .chain(game.paintings.iter().map(|v| v.id))
            .chain(game.interactions.keys().map(|id| *id as u32));
        for id in ids {
            if let Some((lo, hi, _)) = game.object_target(id as i32) {
                offer(
                    id,
                    horizon_core::geometry::Aabb::new(
                        Vec3::new(lo[0], lo[1], lo[2]),
                        Vec3::new(hi[0], hi[1], hi[2]),
                    ),
                );
            }
        }
        best
    }
    fn clip(&mut self, from: DVec3, to: DVec3, kind: camera::ClipKind) -> Option<camera::BlockHit> {
        if kind == camera::ClipKind::Outline {
            let delta = to - from;
            let hit = self.0.clip_ray(
                from.to_array(),
                delta.try_normalize()?.to_array(),
                delta.length(),
            )?;
            return Some(camera::BlockHit {
                position: DVec3::from_array(hit.point),
                block: IVec3::from_array(hit.pos),
                normal: IVec3::from_array(hit.face),
            });
        }
        let mut ray = Collider(horizon_client::collisions::SceneCollisions {
            world: &self.0.world,
            binding: self.0.collision,
            subject: None,
        });
        let hit = clip_world(
            &mut ray,
            Vec3::new(from.x, from.y, from.z),
            Vec3::new(to.x, to.y, to.z),
            RayBlockMode::Collider,
            RayFluidMode::None,
            4096,
        )
        .ok()?;
        (!hit.miss).then(|| camera::BlockHit {
            position: DVec3::new(hit.hit.location.x, hit.hit.location.y, hit.hit.location.z),
            block: IVec3::ZERO,
            normal: IVec3::ZERO,
        })
    }
}
pub(super) struct Collider<'a, 'r>(pub(super) horizon_client::collisions::SceneCollisions<'a, 'r>);
impl RayWorld for Collider<'_, '_> {
    type Error = std::io::Error;
    type Block = BlockStateId;
    type Fluid = horizon_world::fluid::FluidState;
    fn block_state(&mut self, p: BlockPos) -> Result<BlockStateId, Self::Error> {
        Ok(self.0.world.block(p.x, p.y, p.z))
    }
    fn fluid_state(&mut self, p: BlockPos) -> Result<Self::Fluid, Self::Error> {
        self.0
            .binding
            .registry()
            .state(self.0.world.block(p.x, p.y, p.z))
            .map(horizon_world::fluid::FluidState::from_block)
            .unwrap_or_else(|| Ok(horizon_world::fluid::FluidState::EMPTY))
    }
    fn block_shape(
        &mut self,
        state: BlockStateId,
        p: BlockPos,
        _: RayBlockMode,
    ) -> Result<VoxelShape, Self::Error> {
        let binding = self.0.binding;
        binding
            .collision_shape_with_context(&mut self.0, state, p, &CollisionContext::empty(false))
            .map_err(|e| std::io::Error::other(format!("{e:?}")))
    }
    fn interaction_shape(
        &mut self,
        _: BlockStateId,
        _: BlockPos,
    ) -> Result<VoxelShape, Self::Error> {
        Ok(VoxelShape::empty())
    }
    fn fluid_is_source(&mut self, fluid: Self::Fluid) -> Result<bool, Self::Error> {
        Ok(fluid.is_source())
    }
    fn fluid_is_empty(&mut self, fluid: Self::Fluid) -> Result<bool, Self::Error> {
        Ok(fluid.kind() == horizon_world::fluid::FluidKind::Empty)
    }
    fn fluid_is_water(&mut self, fluid: Self::Fluid) -> Result<bool, Self::Error> {
        Ok(fluid.kind().template() == horizon_world::fluid::FluidTemplate::Water)
    }
    fn fluid_shape(&mut self, fluid: Self::Fluid, p: BlockPos) -> Result<VoxelShape, Self::Error> {
        let above = self.fluid_state(BlockPos::new(p.x, p.y + 1, p.z))?;
        let height = if fluid.kind().same_family(above.kind()) {
            1.
        } else {
            f64::from(fluid.own_height())
        };
        if height <= 0. {
            return Ok(VoxelShape::empty());
        }
        VoxelShape::from_grid([vec![0., 1.], vec![0., height], vec![0., 1.]], &[true])
            .map_err(|e| std::io::Error::other(format!("{e:?}")))
    }
}
