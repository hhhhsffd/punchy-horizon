//! State and numerical rules recovered from Moves' motion helpers.
//! No engine/world access: probes are captured once by the runtime adapter.
use super::{Input, number};
use crate::{Clip, Pose};
use glam::Vec3;
use serde_json::Value;
use std::collections::BTreeMap;

pub const STEP: f32 = 0.05;
pub fn finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}
pub fn unit(value: f32) -> f32 {
    finite(value, 0.).clamp(0., 1.)
}
pub fn wrap(value: f32) -> f32 {
    (finite(value, 0.) + 180.).rem_euclid(360.) - 180.
}
pub fn response(rate: f32) -> f32 {
    1. - (-finite(rate, 6.).clamp(0., 200.) * STEP).exp()
}
pub fn smooth(value: f32) -> f32 {
    let t = unit(value);
    t * t * (3. - 2. * t)
}
pub fn approach(current: &mut f32, target: f32, rate: f32) {
    *current += (target - *current) * response(rate);
    if (*current - target).abs() < 0.0001 {
        *current = target;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Ground,
    Air,
    Falling,
    Crawl,
    Climb,
    Fly,
    Elytra,
    Swim,
    WaterAir,
    WaterContact,
    WaterLoop,
    LavaAir,
    LavaContact,
    Passenger,
    Sit,
}
impl State {
    pub fn ordinary(self) -> bool {
        matches!(self, Self::Ground | Self::Air)
    }
    pub fn hands_only(self) -> bool {
        matches!(self, Self::Swim | Self::Fly)
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Ground => "ground",
            Self::Air => "air",
            Self::Falling => "falling",
            Self::Crawl => "crawling",
            Self::Climb => "climbing",
            Self::Fly => "creative_fly",
            Self::Elytra => "elytra",
            Self::Swim => "swimming",
            Self::WaterAir => "water_air",
            Self::WaterContact => "water_contact",
            Self::WaterLoop => "water_loop",
            Self::LavaAir => "lava_air",
            Self::LavaContact => "lava_contact",
            Self::Passenger => "passenger",
            Self::Sit => "sitting",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Gait {
    pub phase: f32,
    pub previous_phase: f32,
    pub crawl_phase: f32,
    pub directions: [f32; 4],
    pub crawl_directions: [f32; 4],
    pub motion: f32,
    pub sprint: f32,
    pub amplitude: f32,
    pub use_stride_scale: f32,
    pub distance: f32,
    pub sprint_seconds: f32,
    pub rare_run: bool,
    pub crouch: f32,
    pub moving: bool,
    sprint_sequence: u64,
    previous_state: State,
}
impl Default for Gait {
    fn default() -> Self {
        Self {
            phase: 0.,
            previous_phase: 0.,
            crawl_phase: 0.,
            directions: [0.; 4],
            crawl_directions: [1., 0., 0., 0.],
            motion: 0.,
            sprint: 0.,
            amplitude: 1.,
            use_stride_scale: 1.,
            distance: 0.,
            sprint_seconds: 0.,
            rare_run: false,
            crouch: 0.,
            moving: false,
            sprint_sequence: 0,
            previous_state: State::Ground,
        }
    }
}
/// Forward/backward input takes precedence over strafing (including diagonals),
/// exactly as the original ground direction selector. Flight remains analog.
pub fn direction(i: &Input) -> [f32; 4] {
    let mut forward = i.movement[1];
    let mut left = i.movement[0];
    if forward.abs() < 0.01 && left.abs() < 0.01 && i.speed > 0.0125 {
        let (sin, cos) = i.body_yaw.unwrap_or(i.yaw).to_radians().sin_cos();
        forward = i.velocity.z * cos - i.velocity.x * sin;
        left = i.velocity.x * cos + i.velocity.z * sin;
        if forward.abs() < 0.0001 && left.abs() < 0.0001 {
            forward = i.speed;
        }
        if left.abs() > forward.abs() {
            forward = 0.;
        }
    }
    if forward.abs() >= 0.01 {
        [f32::from(forward > 0.), f32::from(forward < 0.), 0., 0.]
    } else if left.abs() >= 0.01 {
        [0., 0., f32::from(left > 0.), f32::from(left < 0.)]
    } else {
        [0.; 4]
    }
}
pub fn flight_direction(i: &Input) -> [f32; 2] {
    let mut forward = i.movement[1];
    let mut left = i.movement[0];
    if forward.abs() + left.abs() < 0.01 && i.speed > 0.0125 {
        let (sin, cos) = i.body_yaw.unwrap_or(i.yaw).to_radians().sin_cos();
        forward = i.velocity.z * cos - i.velocity.x * sin;
        left = i.velocity.x * cos + i.velocity.z * sin;
    }
    let magnitude = forward.hypot(left).max(1.);
    [forward / magnitude, left / magnitude]
}
pub fn swim_intent(i: &Input) -> [f32; 3] {
    if i.remote {
        let (sin, cos) = i.yaw.to_radians().sin_cos();
        let forward = -i.velocity.x * sin + i.velocity.z * cos;
        [
            if forward.abs() < 0.01 {
                0.
            } else {
                forward.signum()
            },
            if i.vertical_speed.abs() < 0.025 {
                0.
            } else {
                i.vertical_speed.signum()
            },
            unit(i.speed / 0.1),
        ]
    } else {
        [
            i.movement[1],
            if i.swim_vertical == 0. {
                0.
            } else {
                i.swim_vertical.signum()
            },
            unit(i.movement[0].abs() + i.movement[1].abs()),
        ]
    }
}
impl Gait {
    pub fn tick(
        &mut self,
        i: &Input,
        previous: &Input,
        state: State,
        walking: &Value,
        run: &Value,
    ) {
        self.previous_phase = self.phase;
        self.moving = i.speed > 0.0125 || i.movement.iter().any(|v| v.abs() > 0.05);
        let compatible = state.ordinary();
        let requested = if compatible { direction(i) } else { [0.; 4] };
        for (current, target) in self.directions.iter_mut().zip(requested) {
            approach(current, target, 8.);
        }
        if state == State::Crawl {
            let requested = direction(i);
            if self.previous_state != State::Crawl {
                self.crawl_phase = 0.;
                self.crawl_directions = if requested.iter().any(|v| *v > 0.) {
                    requested
                } else {
                    [1., 0., 0., 0.]
                };
            } else if requested.iter().any(|v| *v > 0.) {
                for (current, target) in self.crawl_directions.iter_mut().zip(requested) {
                    approach(current, target, 8.);
                }
            }
        }
        self.previous_state = state;
        let scale = if i.using.is_some() && i.riding.is_none() {
            i.use_speed_multiplier.unwrap_or(0.2).clamp(0.1, 1.)
        } else {
            1.
        };
        approach(&mut self.use_stride_scale, scale, 9.);
        approach(
            &mut self.motion,
            if compatible && self.moving {
                unit(i.speed / (0.11 * scale))
            } else {
                0.
            },
            9.,
        );
        let sprinting =
            compatible && self.moving && i.sprinting && !i.on_water && requested[0] > 0.;
        approach(&mut self.sprint, f32::from(sprinting), 9.);
        self.crouch += (f32::from(compatible && i.crouching) - self.crouch).clamp(-0.2, 0.4);
        if i.sprinting && !previous.sprinting {
            self.sprint_sequence = self.sprint_sequence.wrapping_add(1);
            let roll = hash(
                i.random_seed
                    .wrapping_mul(31)
                    .wrapping_add(self.sprint_sequence),
            ) % 10000;
            self.rare_run = roll < (unit(number(&run["chance"], 0.125)) * 10000.) as u64;
            self.sprint_seconds = 0.;
        }
        if sprinting && i.grounded {
            self.sprint_seconds = (self.sprint_seconds + STEP).min(86400.);
        }
        if !i.sprinting && self.sprint < 0.001 {
            self.sprint_seconds = 0.;
            self.rare_run = false;
        }
        let raw = i.displacement.unwrap_or(i.speed);
        self.distance = if raw.is_finite()
            && raw >= number(&walking["minimum_displacement"], 0.0005)
            && raw <= 8.
        {
            raw.min(number(&walking["maximum_displacement"], 0.35).clamp(0., 8.))
        } else {
            0.
        };
        if !matches!(
            state,
            State::Elytra | State::Fly | State::Climb | State::Crawl
        ) {
            let prefix = if i.crouching {
                "crouch_"
            } else if i.sprinting {
                "sprint_"
            } else {
                ""
            };
            let stride = number(
                &walking[format!("{prefix}stride_blocks_per_cycle")],
                if i.crouching { 1.15 } else { 1.67 },
            )
            .clamp(0.05, 100.);
            let cap = number(
                &walking[format!("{prefix}max_cycles_per_second")],
                if i.sprinting {
                    1.5833334
                } else if i.crouching {
                    0.75
                } else {
                    1.25
                },
            )
            .clamp(0., 20.);
            self.phase = (self.phase + (self.distance / stride).min(cap * STEP)).rem_euclid(1.);
        }
        if state == State::Crawl && self.moving {
            self.crawl_phase =
                (self.crawl_phase + STEP * (i.speed * 10.).clamp(0.35, 1.45)).rem_euclid(1.);
        }
        let reference = number(&walking["amplitude_reference_speed"], 0.) * scale;
        let amplitude = if reference > 0. {
            (self.distance / reference)
                .clamp(0., number(&walking["max_amplitude"], 1.2).clamp(0., 4.))
        } else {
            1.
        };
        approach(&mut self.amplitude, amplitude, 9.);
    }
}
pub fn hash(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
pub fn random(seed: &mut u64) -> f32 {
    *seed = hash(seed.wrapping_add(0x9e3779b97f4a7c15));
    (*seed >> 40) as f32 / 16777216.
}
pub fn interval(entry: &Value, seed: &mut u64, default: [f32; 2]) -> f32 {
    let low = number(&entry["min_interval_seconds"], default[0]).clamp(0.05, 86400.);
    let high = number(&entry["max_interval_seconds"], default[1]).clamp(low, 86400.);
    low + (high - low) * random(seed)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tuck {
    #[default]
    Dive,
    Cannonball,
    BellyFlop,
}
impl Tuck {
    pub fn key(self, looped: bool) -> &'static str {
        match (self, looped) {
            (Self::Dive, false) => "air",
            (Self::Dive, true) => "loop",
            (Self::Cannonball, false) => "sneak_air",
            (Self::Cannonball, true) => "sneak_loop",
            (Self::BellyFlop, false) => "late_sneak_air",
            (Self::BellyFlop, true) => "late_sneak_loop",
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct ShallowWater {
    pub active: bool,
    pub surface: Option<f32>,
}
impl ShallowWater {
    pub fn tick(&mut self, i: &Input) {
        let head_above = |surface: Option<f32>| {
            surface
                .zip(i.eye_y)
                .filter(|(surface, eye)| *eye > *surface + 0.02)
                .map(|(surface, _)| surface)
        };
        let surface = if i.underwater {
            None
        } else {
            head_above(i.occupied_shallow_surface).or_else(|| {
                (!i.grounded && self.active)
                    .then(|| head_above(i.nearby_shallow_surface))
                    .flatten()
            })
        };
        self.active = surface.is_some();
        if surface.is_some() {
            self.surface = surface;
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Fluid {
    pub state: Option<State>,
    pub tuck: Tuck,
    pub seconds: f32,
    pub entered: bool,
    exit_lock: bool,
    traversal_grace: u8,
}
impl Fluid {
    pub fn tick(
        &mut self,
        i: &Input,
        previous: &Input,
        water: &Value,
        lava: &Value,
        contact_finished: bool,
        loop_finished: bool,
    ) {
        self.entered = false;
        if i.climbing || i.fence || i.traversal_support {
            self.traversal_grace = 4;
        } else {
            self.traversal_grace = self.traversal_grace.saturating_sub(1);
        }
        if i.riding.is_some() || i.elytra || i.flying || i.climbing || i.crawling {
            self.state = None;
            self.exit_lock |= i.in_water || i.underwater || i.in_lava;
            return;
        }
        let in_water = i.in_water || i.swimming || i.underwater || i.on_water;
        if in_water || i.in_lava || previous.in_water || previous.swimming {
            self.exit_lock = true;
        } else if i.grounded {
            self.exit_lock = false;
        }
        let stroke = i.speed > 0.015
            || i.movement.iter().any(|v| v.abs() > 0.01)
            || i.swim_vertical.abs() > 0.01;
        match self.state {
            Some(State::WaterAir) => {
                if in_water {
                    self.state = Some(if water["contact"].is_string() && self.tuck == Tuck::Dive {
                        State::WaterContact
                    } else {
                        State::WaterLoop
                    });
                    self.seconds = 0.;
                    self.entered = true;
                } else if i.grounded
                    || i.in_lava
                    || i.vertical_speed >= 0.
                    || i.dive_trajectory_clear == Some(false)
                    || !i.water_below
                {
                    self.state = None;
                } else {
                    if i.crouching && self.tuck == Tuck::Dive && water["sneak_air"].is_string() {
                        self.tuck = if self.seconds
                            <= number(&water["sneak_window_seconds"], 0.3).clamp(0., 10.)
                        {
                            Tuck::Cannonball
                        } else if water["late_sneak_air"].is_string() {
                            Tuck::BellyFlop
                        } else {
                            Tuck::Cannonball
                        };
                    }
                    self.seconds += STEP;
                }
            }
            Some(State::WaterContact | State::WaterLoop) => {
                if !in_water || stroke || i.on_water && i.grounded || loop_finished {
                    self.state = None;
                } else if contact_finished && self.state == Some(State::WaterContact) {
                    self.state = Some(State::WaterLoop);
                    self.seconds = 0.;
                    self.entered = true;
                } else {
                    self.seconds += STEP;
                }
            }
            Some(State::LavaAir) => {
                if i.in_lava {
                    self.state = Some(State::LavaContact);
                    self.entered = true;
                    self.seconds = 0.;
                } else if i.grounded || !i.lava_below {
                    self.state = None;
                }
            }
            Some(State::LavaContact) => {
                if !i.in_lava {
                    self.state = None;
                }
            }
            _ => {
                if !i.grounded
                    && !self.exit_lock
                    && self.traversal_grace == 0
                    && !in_water
                    && !i.in_lava
                {
                    let lava_height = i
                        .lava_distance
                        .map(|d| d + i.fall_distance)
                        .unwrap_or(i.fall_distance);
                    let water_height = i.water_distance.unwrap_or(i.fall_distance);
                    // LocomotionController.LAVA_DIVE_ENABLED is false in the
                    // reference. Probe priority still suppresses water dives.
                    let lava_entry = i.lava_below
                        && lava_height >= number(&lava["trigger_distance"], 3.).max(0.5);
                    if !lava_entry
                        && i.water_below
                        && i.vertical_speed < -0.385
                        && water_height >= number(&water["trigger_distance"], 6.).max(6.)
                        && i.dive_trajectory_clear != Some(false)
                    {
                        self.state = Some(State::WaterAir);
                        self.tuck = Tuck::Dive;
                        self.seconds = 0.;
                        self.entered = true;
                        if i.crouching && water["sneak_air"].is_string() {
                            self.tuck = Tuck::Cannonball;
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DirectionLean {
    travel: Option<Vec3>,
    slow_ticks: u8,
    pub pitch: f32,
    pub roll: f32,
}
impl DirectionLean {
    pub fn tick(&mut self, i: &Input, allowed: bool) {
        if !allowed {
            *self = Self::default();
            return;
        }
        let speed = Vec3::new(i.velocity.x, 0., i.velocity.z).length();
        let mut pitch = 0.;
        let mut roll = 0.;
        if speed >= 0.04 {
            let travel = Vec3::new(i.velocity.x, 0., i.velocity.z) / speed;
            if let Some(previous) = self.travel {
                let delta = travel - previous;
                let (sin, cos) = i.body_yaw.unwrap_or(i.yaw).to_radians().sin_cos();
                let weight = unit(speed / 0.215);
                roll = (-(delta.x * cos + delta.z * sin) * 5. * weight).clamp(-3., 3.);
                pitch = (-(-delta.x * sin + delta.z * cos) * 5. * weight).clamp(-3., 3.);
                self.travel = Some(previous.lerp(travel, 0.45));
            } else {
                self.travel = Some(travel);
            }
            self.slow_ticks = 0;
        } else {
            self.slow_ticks = self.slow_ticks.saturating_add(1);
            if self.slow_ticks > 3 {
                self.travel = None;
            }
        }
        self.pitch += (pitch - self.pitch) * 0.4;
        self.roll += (roll - self.roll) * 0.4;
    }
}

/// The foot/leg matching search avoids popping when landing back into a run.
pub fn match_phase(current: f32, mut cost: impl FnMut(f32) -> f32) -> f32 {
    let mut best = current;
    let mut score = cost(current);
    if !score.is_finite() {
        return current;
    }
    for step in 1..64 {
        let candidate = (current + step as f32 / 64.).rem_euclid(1.);
        let next = cost(candidate);
        if next.is_finite() && next < score - 0.0001 {
            best = candidate;
            score = next;
        }
    }
    let center = best;
    for step in -8..=8 {
        let candidate = (center + step as f32 / 512.).rem_euclid(1.);
        let next = cost(candidate);
        if next.is_finite() && next < score - 0.0001 {
            best = candidate;
            score = next;
        }
    }
    best
}
pub fn pose_cost(a: Pose, b: Pose) -> f32 {
    a.euler.distance_squared(b.euler) + 16. * a.position.distance_squared(b.position)
}
pub fn jump_hold(jump: &Clip, contact: Option<&Clip>, before: f32) -> f32 {
    let Some(contact) = contact.filter(|c| !c.tracks.is_empty()) else {
        return jump.length;
    };
    let end = if before > 0. {
        before.min(jump.length)
    } else {
        jump.length
    };
    let mut best = end;
    let mut score = f32::INFINITY;
    for step in 0..=96 {
        let time = end * step as f32 / 96.;
        let cost = contact
            .tracks
            .keys()
            .take(128)
            .map(|bone| {
                pose_cost(
                    jump.sample_model(bone, time),
                    contact.sample_model(bone, 0.),
                )
            })
            .sum::<f32>();
        if cost < score - 0.0001 {
            best = time;
            score = cost;
        }
    }
    best
}
pub fn landing_amplitude(speed: f32, distance: f32) -> f32 {
    if !speed.is_finite() || speed <= 0. {
        return 0.;
    }
    let distance = finite(distance, 5.).clamp(0.05, 1024.);
    let mut travelled = 0.;
    let mut velocity = 0.;
    for _ in 0..1024 {
        if travelled >= distance {
            break;
        }
        velocity = (velocity + 0.08) * 0.98;
        travelled += velocity;
    }
    unit(speed / velocity.max(0.001))
}
pub fn weight_class(distance: f32, speed: f32, settings: &Value, fall: bool) -> u8 {
    let (medium_distance, medium_speed, heavy_distance, heavy_speed) = if fall {
        (
            number(&settings["free_fall_distance"], 7.),
            number(&settings["free_fall_velocity"], 0.72),
            number(&settings["effect_distance"], 13.),
            number(&settings["effect_velocity"], 1.05),
        )
    } else {
        (
            number(&settings["medium_distance"], 4.),
            number(&settings["medium_velocity"], 0.58),
            number(&settings["heavy_distance"], 8.),
            number(&settings["heavy_velocity"], 0.95),
        )
    };
    if distance >= heavy_distance.max(0.5) || speed >= heavy_speed.max(0.05) {
        3
    } else if distance >= medium_distance.max(0.5) || speed >= medium_speed.max(0.05) {
        2
    } else {
        1
    }
}

#[derive(Clone, Debug, Default)]
pub struct Signals {
    pub tick: u64,
    pub values: BTreeMap<String, f64>,
    pub events: BTreeMap<String, u64>,
}
impl Signals {
    pub fn event(&mut self, name: &str) {
        self.events.insert(name.to_owned(), self.tick);
    }
    pub fn since(&self, name: &str) -> u64 {
        self.events
            .get(name)
            .map_or(u64::MAX, |t| self.tick.saturating_sub(*t))
    }
    pub fn capture(&mut self, i: &Input, state: State, gait: &Gait) {
        self.tick = self.tick.wrapping_add(1);
        self.values.clear();
        let flags = [
            ("grounded", i.grounded),
            ("on_ground", i.grounded),
            ("moving", gait.moving),
            ("sprinting", i.sprinting),
            ("running", i.sprinting),
            ("crouching", i.crouching),
            ("sneaking", i.crouching),
            ("swimming", state == State::Swim),
            ("surface_swimming", state == State::Swim && !i.underwater),
            ("submerged_swimming", state == State::Swim && i.underwater),
            ("swim_sprint", state == State::Swim && i.sprinting),
            ("underwater", i.underwater),
            ("in_water", i.in_water),
            ("shallow_water", i.on_water),
            ("climbing", i.climbing),
            ("crawling", i.crawling),
            ("elytra", i.elytra),
            ("creative_flight", i.flying),
            ("fast_flight", i.flying && i.sprinting),
            ("on_fence", i.fence),
            ("mining", i.mining),
            ("using_item", i.using.is_some()),
            (
                "blocking",
                i.using
                    .is_some_and(|(h, _)| i.items[h].ends_with(":shield")),
            ),
            ("water_below", i.water_below),
            ("lava_below", i.lava_below),
            ("music_nearby", i.music),
            ("freezing", i.cold),
            ("fishing", i.fishing),
            ("biting", i.fishing_bite),
            ("rare_run", gait.rare_run),
            ("hurt", i.hurt),
        ];
        for (name, value) in flags {
            self.values.insert(name.into(), f64::from(value));
        }
        let values = [
            ("horizontal_speed", i.speed),
            ("speed", i.speed),
            ("vertical_speed", i.vertical_speed),
            ("velocity_x", i.velocity.x),
            ("velocity_y", i.vertical_speed),
            ("velocity_z", i.velocity.z),
            ("yaw", i.yaw),
            ("pitch", i.pitch),
            ("fall_distance", i.fall_distance),
            ("ground_clearance", i.ground_clearance.unwrap_or(0.)),
            ("walk_forward", i.movement[1]),
            ("walk_left", i.movement[0]),
            ("run_seconds", gait.sprint_seconds),
            ("sprint_elapsed", gait.sprint_seconds),
            ("locomotion_phase", gait.phase),
            ("stride_distance", gait.distance),
            ("limb_speed", gait.motion),
            ("sprint_weight", gait.sprint),
            ("crouch_modifier", smooth(gait.crouch)),
            ("forward", gait.directions[0]),
            ("backward", gait.directions[1]),
            ("left", gait.directions[2]),
            ("right", gait.directions[3]),
            ("swim_vertical", i.swim_vertical),
            ("use_ticks", i.use_ticks.unwrap_or(0) as f32),
            ("attack_strength", i.attack_strength.unwrap_or(1.)),
            ("age", self.tick as f32),
        ];
        for (name, value) in values {
            self.values
                .insert(name.into(), f64::from(finite(value, 0.)));
        }
        for (event, time) in &self.events {
            let since = self.tick.saturating_sub(*time) as f64;
            self.values.insert(format!("since_{event}"), since);
            self.values
                .insert(format!("{event}_seconds"), since * f64::from(STEP));
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    #[test]
    fn shallow_water_latches_only_with_head_clear_and_nearby_airborne_surface() {
        let mut state = ShallowWater::default();
        let mut i = Input {
            grounded: true,
            eye_y: Some(2.),
            occupied_shallow_surface: Some(1.),
            ..Default::default()
        };
        state.tick(&i);
        assert!(state.active);
        i.occupied_shallow_surface = None;
        i.nearby_shallow_surface = Some(1.);
        i.grounded = false;
        state.tick(&i);
        assert!(state.active);
        i.underwater = true;
        state.tick(&i);
        assert!(!state.active);
        i.underwater = false;
        state.tick(&i);
        assert!(!state.active);
        i.occupied_shallow_surface = Some(1.);
        i.eye_y = Some(1.01);
        state.tick(&i);
        assert!(!state.active);
    }
    #[test]
    fn ground_diagonals_prefer_forward_but_velocity_can_strafe() {
        let mut i = Input {
            movement: [1., 1.],
            ..Default::default()
        };
        assert_eq!(direction(&i), [1., 0., 0., 0.]);
        i.movement = [0., 0.];
        i.speed = 0.2;
        i.velocity = Vec3::new(-0.2, 0., 0.);
        assert_eq!(direction(&i), [0., 0., 0., 1.]);
    }
    #[test]
    fn matcher_refines_and_wraps() {
        let phase = match_phase(0.95, |p| wrap((p - 0.1) * 360.).powi(2));
        assert!((phase - 0.1).abs() < 0.003);
    }
    #[test]
    fn dive_requires_clear_high_descent_and_stays_locked_after_fluid_exit() {
        let mut f = Fluid::default();
        let mut i = Input {
            water_below: true,
            water_distance: Some(8.),
            vertical_speed: -0.5,
            dive_trajectory_clear: Some(true),
            ..Default::default()
        };
        let settings = serde_json::json!({"trigger_distance":3,"sneak_air":"tuck"});
        f.tick(&i, &Input::default(), &settings, &Value::Null, false, false);
        assert_eq!(f.state, Some(State::WaterAir));
        i.in_water = true;
        f.tick(&i, &Input::default(), &settings, &Value::Null, false, false);
        assert_eq!(f.state, Some(State::WaterLoop));
        let previous = i.clone();
        i.in_water = false;
        f.tick(&i, &previous, &settings, &Value::Null, false, false);
        f.tick(&i, &i, &settings, &Value::Null, false, false);
        assert_eq!(f.state, None);
    }
    #[test]
    fn teleports_do_not_advance_stride_and_invalid_values_are_finite() {
        let mut gait = Gait::default();
        let i = Input {
            grounded: true,
            speed: 0.2,
            displacement: Some(9.),
            ..Default::default()
        };
        gait.tick(
            &i,
            &Input::default(),
            State::Ground,
            &Value::Null,
            &Value::Null,
        );
        assert_eq!(gait.phase, 0.);
        assert_eq!(wrap(f32::NAN), 0.);
        assert!(landing_amplitude(0.5, f32::NAN).is_finite());
    }
    #[test]
    fn item_use_scales_gait_weight_and_amplitude_not_stride_length() {
        let mut gait = Gait::default();
        let i = Input {
            grounded: true,
            speed: 0.022,
            displacement: Some(0.022),
            using: Some((0, super::super::Use::Other)),
            use_speed_multiplier: Some(0.2),
            ..Default::default()
        };
        gait.tick(
            &i,
            &Input::default(),
            State::Ground,
            &Value::Null,
            &Value::Null,
        );
        assert!((gait.phase - 0.022 / 1.67).abs() < 0.000001);
        assert!((gait.motion - response(9.)).abs() < 0.000001);
        assert!(gait.use_stride_scale < 1.);
    }
    #[test]
    fn crawl_keeps_its_last_direction_while_idle_and_flight_is_analog() {
        let mut gait = Gait::default();
        let mut i = Input {
            movement: [1., 0.],
            ..Default::default()
        };
        gait.tick(
            &i,
            &Input::default(),
            State::Crawl,
            &Value::Null,
            &Value::Null,
        );
        assert_eq!(gait.crawl_directions, [0., 0., 1., 0.]);
        i.movement = [0., 0.];
        gait.tick(
            &i,
            &Input::default(),
            State::Crawl,
            &Value::Null,
            &Value::Null,
        );
        assert_eq!(gait.crawl_directions, [0., 0., 1., 0.]);
        i.movement = [1., 1.];
        let flight = flight_direction(&i);
        assert!((flight[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.000001);
        assert_eq!(flight[0], flight[1]);
    }
    #[test]
    fn reference_disables_lava_dive_and_remembers_climbing_grace() {
        let mut fluid = Fluid::default();
        let mut i = Input {
            lava_below: true,
            lava_distance: Some(8.),
            vertical_speed: -0.5,
            ..Default::default()
        };
        fluid.tick(
            &i,
            &Input::default(),
            &Value::Null,
            &Value::Null,
            false,
            false,
        );
        assert_eq!(fluid.state, None);
        i.lava_below = false;
        i.climbing = true;
        fluid.tick(
            &i,
            &Input::default(),
            &Value::Null,
            &Value::Null,
            false,
            false,
        );
        i.climbing = false;
        i.water_below = true;
        i.water_distance = Some(8.);
        fluid.tick(
            &i,
            &Input::default(),
            &Value::Null,
            &Value::Null,
            false,
            false,
        );
        assert_eq!(fluid.state, None);
        for _ in 0..4 {
            fluid.tick(
                &i,
                &Input::default(),
                &Value::Null,
                &Value::Null,
                false,
                false,
            );
        }
        assert_eq!(fluid.state, Some(State::WaterAir));
    }
}
