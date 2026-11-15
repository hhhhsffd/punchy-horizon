// The original motion probes, evaluated against Horizon's collision/fluid data.
// Distances stop at the first non-air block; water behind a floor is not a dive.
use super::Game;
use horizon_client::mobs::anim::punchy_moves::engine::moves_animation::Input;
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
};
use horizon_world::{
    fluid::{FluidKind, FluidState},
    world_ray::{RayBlockMode, RayFluidMode, RayWorld, clip_world},
};

fn fluid(game: &Game<'_>, p: BlockPos) -> FluidState {
    game.pack
        .assets
        .registry
        .state(game.world.block(p.x, p.y, p.z))
        .and_then(|state| FluidState::from_block(state).ok())
        .unwrap_or(FluidState::EMPTY)
}
fn height(game: &Game<'_>, p: BlockPos, state: FluidState) -> f64 {
    if state
        .kind()
        .same_family(fluid(game, BlockPos::new(p.x, p.y + 1, p.z)).kind())
    {
        1.
    } else {
        f64::from(state.own_height())
    }
}
fn air(game: &Game<'_>, p: BlockPos) -> bool {
    game.res.facts().is_air(game.world.block(p.x, p.y, p.z))
}
fn solid(game: &Game<'_>, p: BlockPos) -> bool {
    let mut rays = super::punchy_camera::Collider(horizon_client::collisions::SceneCollisions {
        world: &game.world,
        binding: game.collision,
        subject: None,
    });
    let state = game.world.block(p.x, p.y, p.z);
    rays.block_shape(state, p, RayBlockMode::Collider)
        .map_or(true, |s| !s.is_empty())
}
fn deep(game: &Game<'_>, p: BlockPos) -> bool {
    (0..2).all(|dy| {
        let p = BlockPos::new(p.x, p.y - dy, p.z);
        let f = fluid(game, p);
        f.kind().same_family(FluidKind::Water) && f.is_source() && !solid(game, p)
    })
}
fn fluid_distance(game: &Game<'_>, position: Vec3, kind: FluidKind) -> Option<f32> {
    let start = (position.y - 0.01).floor() as i32;
    for depth in 0..48 {
        let p = BlockPos::new(
            position.x.floor() as i32,
            start - depth,
            position.z.floor() as i32,
        );
        let f = fluid(game, p);
        if f.kind().same_family(kind) {
            return Some((position.y - f64::from(p.y) - height(game, p, f)).max(0.) as f32);
        }
        if !air(game, p) {
            return None;
        }
    }
    None
}
fn shallow(game: &Game<'_>, p: BlockPos) -> Option<f64> {
    let f = fluid(game, p);
    if f.kind().same_family(FluidKind::Water)
        && !fluid(game, BlockPos::new(p.x, p.y + 1, p.z))
            .kind()
            .same_family(FluidKind::Water)
        && !fluid(game, BlockPos::new(p.x, p.y - 1, p.z))
            .kind()
            .same_family(FluidKind::Water)
        && solid(game, BlockPos::new(p.x, p.y - 1, p.z))
    {
        Some(f64::from(p.y) + height(game, p, f))
    } else {
        None
    }
}
fn shallow_surfaces(game: &Game<'_>, bounds: Aabb, max_below: f64) -> (Option<f32>, Option<f32>) {
    let min = bounds.min_position();
    let max = bounds.max_position();
    if max.x - min.x > 8. || max.z - min.z > 8. {
        return (None, None);
    }
    let mut occupied: Option<f64> = None;
    let mut nearby: Option<f64> = None;
    for x in ((min.x + 0.001).floor() as i32)..=((max.x - 0.001).floor() as i32) {
        for z in ((min.z + 0.001).floor() as i32)..=((max.z - 0.001).floor() as i32) {
            for y in
                ((min.y + 0.001).floor() as i32)..=((max.y - 0.001).min(min.y + 1.).floor() as i32)
            {
                if let Some(surface) = shallow(game, BlockPos::new(x, y, z))
                    && surface > min.y + 0.001
                    && f64::from(y) < max.y - 0.001
                {
                    occupied = Some(occupied.map_or(surface, |old| old.max(surface)));
                }
            }
        }
    }
    let inset_x = ((max.x - min.x) * 0.25).min(0.2);
    let inset_z = ((max.z - min.z) * 0.25).min(0.2);
    for x in [min.x + inset_x, max.x - inset_x, (min.x + max.x) * 0.5] {
        for z in [min.z + inset_z, max.z - inset_z, (min.z + max.z) * 0.5] {
            for y in
                (((min.y - max_below - 1.).floor() as i32)..=((min.y + 0.2).floor() as i32)).rev()
            {
                let p = BlockPos::new(x.floor() as i32, y, z.floor() as i32);
                if fluid(game, p).kind().same_family(FluidKind::Water) {
                    if let Some(surface) = shallow(game, p)
                        && surface >= min.y - max_below - 0.05
                    {
                        nearby = Some(nearby.map_or(surface, |old| old.max(surface)));
                    }
                    break;
                }
                if !air(game, p) {
                    break;
                }
            }
        }
    }
    (occupied.map(|v| v as f32), nearby.map(|v| v as f32))
}
fn trajectory_clear(game: &Game<'_>, mut bounds: Aabb, mut movement: Vec3, gravity: f64) -> bool {
    if !gravity.is_finite() {
        return false;
    }
    for _ in 0..40 {
        // SceneCollisions expands its own budget to the swept volume. Reject
        // pathological velocities/scales before that allocation, not afterwards.
        if horizon_world::block_collision_scan::scan_cells(bounds.expand_towards(movement))
            .is_none_or(|cells| cells > 4096)
        {
            return false;
        }
        let mut scene = horizon_client::collisions::SceneCollisions {
            world: &game.world,
            binding: game.collision,
            subject: None,
        };
        let Ok(resolved) = scene.resolve(bounds, movement, 0., false) else {
            return false;
        };
        let center = bounds.center();
        let feet = Vec3::new(center.x, bounds.min_position().y + 0.001, center.z);
        let destination = Vec3::new(
            feet.x + resolved.x,
            feet.y + resolved.y - 0.002,
            feet.z + resolved.z,
        );
        let mut rays = super::punchy_camera::Collider(scene);
        if let Ok(hit) = clip_world(
            &mut rays,
            feet,
            destination,
            RayBlockMode::Collider,
            RayFluidMode::Any,
            4096,
        ) && !hit.miss
        {
            return deep(game, hit.hit.block);
        }
        let error = (resolved.x - movement.x).powi(2)
            + (resolved.y - movement.y).powi(2)
            + (resolved.z - movement.z).powi(2);
        if error > 1e-8 {
            return false;
        }
        bounds = bounds.move_by(movement);
        movement.y = (movement.y - gravity) * 0.98;
    }
    false
}
pub(super) fn capture(game: &Game<'_>, input: &mut Input, gravity: f64) {
    let Some(p) = &game.player else {
        return;
    };
    let at = p.position;
    let bounds = p.bounds();
    input.water_probes = true;
    let dx = at.x - p.old_position.x;
    let dz = at.z - p.old_position.z;
    input.displacement = Some(dx.hypot(dz) as f32);
    input.vertical_displacement = Some((at.y - p.old_position.y) as f32);
    input.teleported = p.teleported || dx * dx + dz * dz + (at.y - p.old_position.y).powi(2) > 64.;
    input.water_distance = fluid_distance(game, at, FluidKind::Water);
    input.lava_distance = fluid_distance(game, at, FluidKind::Lava);
    input.water_below = input.water_distance.is_some();
    input.lava_below = input.lava_distance.is_some();
    if !input.grounded && input.water_below && !input.in_water && input.vertical_speed < -0.05 {
        input.dive_trajectory_clear = Some(trajectory_clear(game, bounds, p.velocity, gravity));
    }
    let pack = super::punchy_moves::pack(game);
    let grace = punchy_engine_number(
        &pack.entry("GENERAL", "swimming")["surface_grace_distance"],
        1.35,
    )
    .clamp(0.25, 64.);
    (input.occupied_shallow_surface, input.nearby_shallow_surface) =
        shallow_surfaces(game, bounds, f64::from(grace));
    let base = BlockPos::new(
        at.x.floor() as i32,
        at.y.floor() as i32,
        at.z.floor() as i32,
    );
    input.water_surface_y = water_surface(game, bounds, f64::from(grace));
    input.fluid_depth = Some(p.fluid_height(horizon_client::motion::WATER) as f32);
    input.swim_vertical = f32::from(p.jumping) - f32::from(p.shift_key_down);
    input.burning = p.fire_ticks > 0;
    let support = p.ground.supporting_block.unwrap_or(BlockPos::new(
        base.x,
        (at.y - 0.2).floor() as i32,
        base.z,
    ));
    support_input(game, input, support);
    input.ground_clearance = solid_distance(game, at, 64.);
    input.air_gap = input.ground_clearance;
    input.projected_landing_distance = if !input.grounded && input.vertical_speed < 0. {
        input.ground_clearance
    } else {
        Some(0.)
    };
    biome_input(game, input, base);
}
fn punchy_engine_number(v: &serde_json::Value, default: f32) -> f32 {
    horizon_client::mobs::anim::punchy_moves::engine::moves_animation::number(v, default)
}

// Nine samples and at most sixteen rises match findWaterSurfaceY: a centre
// column alone misses ledges, flowing surfaces and the edge of a swimming box.
fn water_surface(game: &Game<'_>, bounds: Aabb, max_below: f64) -> Option<f32> {
    let min = bounds.min_position();
    let max = bounds.max_position();
    let center = bounds.center();
    let inset = ((max.x - min.x) * 0.25).min(0.15);
    let mut highest: Option<f64> = None;
    for x in [center.x, min.x + inset, max.x - inset] {
        for z in [center.z, min.z + inset, max.z - inset] {
            for y in
                (((min.y - max_below - 1.).floor() as i32)..=((min.y + 0.35).floor() as i32)).rev()
            {
                let mut p = BlockPos::new(x.floor() as i32, y, z.floor() as i32);
                if fluid(game, p).kind().same_family(FluidKind::Water) {
                    for _ in 0..16 {
                        let above = BlockPos::new(p.x, p.y + 1, p.z);
                        if !fluid(game, above).kind().same_family(FluidKind::Water) {
                            break;
                        }
                        p = above;
                    }
                    let surface = f64::from(p.y) + height(game, p, fluid(game, p));
                    if surface >= min.y - max_below - 0.05 {
                        highest = Some(highest.map_or(surface, |v| v.max(surface)));
                    }
                    break;
                }
                if !air(game, p) {
                    break;
                }
            }
        }
    }
    highest.map(|v| v as f32)
}

fn solid_distance(game: &Game<'_>, at: Vec3, max_distance: f64) -> Option<f32> {
    let start = Vec3::new(at.x, at.y - 0.001, at.z);
    let end = Vec3::new(at.x, start.y - max_distance, at.z);
    let mut rays = super::punchy_camera::Collider(horizon_client::collisions::SceneCollisions {
        world: &game.world,
        binding: game.collision,
        subject: None,
    });
    clip_world(
        &mut rays,
        start,
        end,
        RayBlockMode::Collider,
        RayFluidMode::None,
        256,
    )
    .ok()
    .map(|hit| {
        if hit.miss {
            max_distance as f32
        } else {
            (at.y - hit.hit.location.y).max(0.) as f32
        }
    })
}

fn support_input(game: &Game<'_>, input: &mut Input, support: BlockPos) {
    let name = &game
        .res
        .facts()
        .get(game.world.block(support.x, support.y, support.z))
        .name;
    input.fence = input.grounded && name.ends_with("_fence");
    input.traversal_support = input.climbing || input.fence || name.ends_with("_stairs");
}

fn biome_input(game: &Game<'_>, input: &mut Input, base: BlockPos) {
    if let Some(chunk) = game.world.chunks.get(&(base.x >> 4, base.z >> 4)) {
        let biomes = game
            .world_biomes
            .as_deref()
            .unwrap_or_else(|| game.res.biomes());
        let biome = biomes.get(chunk.biome(base.x, base.y, base.z));
        input.biome_id = Some(biome.name.clone());
        input.biome_temperature = Some(biome.temperature_at(base, 63));
    }
}

pub(super) fn capture_remote(
    game: &Game<'_>,
    m: &horizon_client::mobs::Mob,
    input: &mut Input,
    grace: f32,
) {
    let at = m.position;
    input.water_probes = true;
    let bounds = m.bounds();
    input.position_y = Some(at.y as f32);
    input.eye_y = Some((at.y + m.dims().2) as f32);
    input.displacement = Some((at.x - m.previous.x).hypot(at.z - m.previous.z) as f32);
    input.vertical_displacement = Some((at.y - m.previous.y) as f32);
    input.teleported = at.subtract(m.previous).length_squared() > 64.;
    input.burning = m.fire_ticks > 0;
    input.freeze_progress = Some((m.ticks_frozen as f32 / 140.).clamp(0., 1.));
    input.swim_vertical = if m.velocity.y.abs() < 0.025 {
        0.
    } else {
        m.velocity.y.signum() as f32
    };
    // The original remote expression context deliberately does not raycast
    // clearance for every avatar.
    input.ground_clearance = Some(if input.grounded { 0. } else { 64. });
    input.air_gap = input.ground_clearance;
    input.projected_landing_distance = if !input.grounded && input.vertical_speed < 0. {
        solid_distance(game, at, 64.)
    } else {
        Some(0.)
    };
    let base = BlockPos::new(
        at.x.floor() as i32,
        at.y.floor() as i32,
        at.z.floor() as i32,
    );
    let support = m
        .supporting
        .map(|p| BlockPos::new(p[0], p[1], p[2]))
        .unwrap_or(BlockPos::new(base.x, (at.y - 0.2).floor() as i32, base.z));
    support_input(game, input, support);
    input.water_distance = fluid_distance(game, at, FluidKind::Water);
    input.lava_distance = fluid_distance(game, at, FluidKind::Lava);
    input.water_below = input.water_distance.is_some();
    input.lava_below = input.lava_distance.is_some();
    if !input.grounded && input.water_below && !input.in_water && input.vertical_speed < -0.05 {
        let gravity = if m.no_gravity {
            0.
        } else {
            m.kind
                .attributes
                .get("minecraft:gravity")
                .copied()
                .unwrap_or(0.08)
        };
        input.dive_trajectory_clear = Some(trajectory_clear(game, bounds, m.velocity, gravity));
    }
    let grace = f64::from(grace.clamp(0.25, 64.));
    (input.occupied_shallow_surface, input.nearby_shallow_surface) =
        shallow_surfaces(game, bounds, grace);
    input.water_surface_y = water_surface(game, bounds, grace);
    input.fluid_depth = Some(m.fluids.water as f32);
    biome_input(game, input, base);
}
