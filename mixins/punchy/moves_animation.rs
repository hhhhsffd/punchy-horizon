//! Native, pack-driven Moves animation arbitration.
//! World observations are plain inputs; this module never loads Java or assets.
use crate::{Clip, Pose, moves_pack::Pack};
use glam::{EulerRot, Mat4, Quat, Vec3};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[path = "moves_motion.rs"]
pub mod motion;
#[path = "moves_sequence.rs"]
mod sequence;
#[path = "moves_anim_turn.rs"]
mod turning;
use motion::{STEP, State};
use sequence::Channel;

pub const MODEL_BONES: [&str; 10] = [
    "root",
    "waist",
    "body",
    "head",
    "right_arm",
    "left_arm",
    "right_leg",
    "left_leg",
    "right_item",
    "left_item",
];

/// Speeds/displacements are blocks per simulation tick, angles are degrees.
/// Optional probes distinguish an unavailable observation from a measured zero.
/// Event flags (`attack`, `used`, `hurt`, etc.) are pulses, not held keys.
#[derive(Clone, Debug, Default)]
pub struct Input {
    pub yaw: f32,
    pub pitch: f32,
    /// [left, forward]; diagonal ground movement uses forward/back precedence.
    pub movement: [f32; 2],
    pub speed: f32,
    pub vertical_speed: f32,
    pub fall_distance: f32,
    pub grounded: bool,
    pub sprinting: bool,
    pub crouching: bool,
    pub swimming: bool,
    pub underwater: bool,
    pub crawling: bool,
    pub climbing: bool,
    pub flying: bool,
    pub elytra: bool,
    pub riding: Option<String>,
    pub cold: bool,
    pub hungry: bool,
    pub nausea: bool,
    pub music: bool,
    pub fence: bool,
    pub on_water: bool,
    pub water_below: bool,
    pub lava_below: bool,
    pub block_distance: f32,
    pub vehicle_yaw: f32,
    pub vehicle_velocity: Vec3,
    pub hurt: bool,
    pub attack: bool,
    pub mining: bool,
    pub used: Option<usize>,
    pub using: Option<(usize, Use)>,
    /// [main, off] registry IDs; an empty string means empty hand.
    pub items: [String; 2],
    pub main_left: bool,
    pub charged: bool,
    pub fishing: bool,
    pub armor_changed: Option<bool>,
    // Extra observations needed by the recovered state machines.
    pub velocity: Vec3,
    pub body_yaw: Option<f32>,
    pub displacement: Option<f32>,
    pub vertical_displacement: Option<f32>,
    pub selected_slot: u8,
    pub force_equip: bool,
    pub swapped: bool,
    pub remote: bool,
    pub use_ticks: Option<u32>,
    pub use_speed_multiplier: Option<f32>,
    pub use_released: bool,
    pub use_event: Option<String>,
    /// Item before an accepted use consumes/replaces the held stack.
    pub accepted_item: Option<String>,
    pub attack_strength: Option<f32>,
    pub attack_presses: u8,
    pub external_attack: bool,
    pub interact: Option<usize>,
    pub hurt_from_back: bool,
    pub shield_hit: Option<usize>,
    pub shield_broken: Option<usize>,
    pub crossbow_shot: Option<usize>,
    pub firework_boost: bool,
    pub fishing_bite: bool,
    pub in_water: bool,
    pub in_lava: bool,
    pub swim_vertical: f32,
    pub water_distance: Option<f32>,
    pub lava_distance: Option<f32>,
    pub dive_trajectory_clear: Option<bool>,
    pub ground_clearance: Option<f32>,
    pub projected_landing_distance: Option<f32>,
    pub traversal_support: bool,
    pub water_surface_y: Option<f32>,
    /// True when None on either shallow probe means a measured absence.
    pub water_probes: bool,
    pub occupied_shallow_surface: Option<f32>,
    pub nearby_shallow_surface: Option<f32>,
    pub eye_y: Option<f32>,
    pub position_y: Option<f32>,
    pub fluid_depth: Option<f32>,
    pub air_gap: Option<f32>,
    pub freeze_progress: Option<f32>,
    pub biome_temperature: Option<f32>,
    pub biome_id: Option<String>,
    pub burning: bool,
    pub food_level: Option<u8>,
    pub greeting: bool,
    pub alignment: bool,
    pub sleeping: bool,
    pub spectator: bool,
    pub dead: bool,
    pub teleported: bool,
    pub random_seed: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    Eat,
    Drink,
    Other,
}
impl Input {
    fn bounded(&self) -> Self {
        let mut i = self.clone();
        i.yaw = motion::wrap(i.yaw);
        i.pitch = motion::finite(i.pitch, 0.).clamp(-90., 90.);
        i.body_yaw = i.body_yaw.filter(|v| v.is_finite()).map(motion::wrap);
        i.movement = i.movement.map(|v| motion::finite(v, 0.).clamp(-1., 1.));
        i.speed = motion::finite(i.speed, 0.).clamp(0., 64.);
        i.vertical_speed = motion::finite(i.vertical_speed, 0.).clamp(-64., 64.);
        i.fall_distance = motion::finite(i.fall_distance, 0.).clamp(0., 65536.);
        i.velocity = if i.velocity.is_finite() {
            i.velocity.clamp(Vec3::splat(-64.), Vec3::splat(64.))
        } else {
            Vec3::ZERO
        };
        i.vehicle_velocity = if i.vehicle_velocity.is_finite() {
            i.vehicle_velocity
                .clamp(Vec3::splat(-64.), Vec3::splat(64.))
        } else {
            Vec3::ZERO
        };
        i.vehicle_yaw = motion::wrap(i.vehicle_yaw);
        i.swim_vertical = motion::finite(i.swim_vertical, 0.).clamp(-1., 1.);
        i.used = i.used.filter(|h| *h < 2);
        i.using = i.using.filter(|(h, _)| *h < 2);
        i.interact = i.interact.filter(|h| *h < 2);
        i.shield_hit = i.shield_hit.filter(|h| *h < 2);
        i.shield_broken = i.shield_broken.filter(|h| *h < 2);
        i.crossbow_shot = i.crossbow_shot.filter(|h| *h < 2);
        for value in [
            &mut i.water_distance,
            &mut i.lava_distance,
            &mut i.ground_clearance,
            &mut i.projected_landing_distance,
            &mut i.displacement,
            &mut i.fluid_depth,
            &mut i.air_gap,
        ] {
            *value = value.filter(|v| v.is_finite()).map(|v| v.clamp(0., 65536.));
        }
        for value in [
            &mut i.position_y,
            &mut i.eye_y,
            &mut i.water_surface_y,
            &mut i.occupied_shallow_surface,
            &mut i.nearby_shallow_surface,
            &mut i.vertical_displacement,
        ] {
            *value = value
                .filter(|v| v.is_finite())
                .map(|v| v.clamp(-30_000_000., 30_000_000.));
        }
        i.use_speed_multiplier = i.use_speed_multiplier.filter(|v| v.is_finite());
        i.freeze_progress = i
            .freeze_progress
            .filter(|v| v.is_finite())
            .map(motion::unit);
        i.biome_temperature = i.biome_temperature.filter(|v| v.is_finite());
        i.attack_strength = i
            .attack_strength
            .filter(|v| v.is_finite())
            .map(motion::unit);
        i.use_ticks = i.use_ticks.map(|v| v.min(72000));
        i.attack_presses = i.attack_presses.min(16);
        i
    }
}

pub fn number(v: &Value, fallback: f32) -> f32 {
    v.as_f64()
        .map(|x| x as f32)
        .filter(|x| x.is_finite())
        .unwrap_or(fallback)
}
fn text<'a>(v: &'a Value, fallback: &'a str) -> &'a str {
    v.as_str().unwrap_or(fallback)
}
pub(crate) fn matches_id(pattern: &str, id: &str) -> bool {
    if let Some((a, b)) = pattern.split_once('*') {
        id.starts_with(a) && id.ends_with(b)
    } else {
        pattern == id
    }
}
pub(crate) fn opposite_bone(bone: &str) -> String {
    if let Some(suffix) = bone.strip_prefix("right_") {
        format!("left_{suffix}")
    } else if let Some(suffix) = bone.strip_prefix("left_") {
        format!("right_{suffix}")
    } else {
        bone.into()
    }
}
fn quaternion(degrees: Vec3) -> Quat {
    Quat::from_euler(
        EulerRot::ZYX,
        degrees.z.to_radians(),
        degrees.y.to_radians(),
        degrees.x.to_radians(),
    )
}
fn with_rotation(mut pose: Pose, rotation: Quat) -> Pose {
    pose.rotation = rotation.normalize();
    let (z, y, x) = pose.rotation.to_euler(EulerRot::ZYX);
    pose.euler = Vec3::new(x, y, z).map(f32::to_degrees);
    pose
}
fn additive(mut base: Pose, addition: Pose, weight: f32) -> Pose {
    base.position += addition.position * weight;
    base.euler += addition.euler * weight;
    base.rotation = quaternion(base.euler);
    base.scale += (addition.scale - Vec3::ONE) * weight;
    base
}
fn mirrored(mut pose: Pose) -> Pose {
    pose.position.x = -pose.position.x;
    pose.euler *= Vec3::new(1., -1., -1.);
    pose.rotation = quaternion(pose.euler);
    pose
}
pub(crate) fn expr_number(
    value: Option<&Value>,
    context: &BTreeMap<String, f64>,
    fallback: f32,
) -> f32 {
    let result = match value {
        Some(Value::String(source)) => {
            crate::moves_expression::evaluate(source, context, f64::from(fallback)) as f32
        }
        Some(Value::Bool(value)) => {
            if *value {
                1.
            } else {
                0.
            }
        }
        Some(value) => number(value, fallback),
        None => fallback,
    };
    motion::finite(result, fallback)
}
pub(crate) fn condition(
    value: Option<&Value>,
    context: &BTreeMap<String, f64>,
    fallback: bool,
) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(source)) => crate::moves_expression::Expression::compile(source)
            .map_or(fallback, |expression| {
                expression.condition(context, fallback)
            }),
        Some(value) if value.is_number() => number(value, if fallback { 1. } else { 0. }) != 0.,
        _ => fallback,
    }
}

/// A parent clip with same-time, bone-masked overlays. `group` identifies a
/// normalized moving crossfade, not a sequence of independent opacity lerps.
#[derive(Clone, Debug)]
pub struct Layer {
    pub clip: Arc<Clip>,
    pub time: f32,
    pub looping: bool,
    pub hold: bool,
    pub weight: f32,
    pub additive: bool,
    pub bones: Vec<String>,
    pub retain: Value,
    pub group: u16,
    pub previous_time: f32,
    pub rate: f32,
    pub mirror: bool,
    pub(crate) visual_left: bool,
    pub overlays: Vec<Layer>,
    pub exclusive: bool,
    pub amplitude: f32,
    pub leg_weight: f32,
    pub default_retain: f32,
    pub retain_world_space: bool,
    pub exclude: Vec<String>,
    pub base: Option<Arc<Clip>>,
    pub held: BTreeMap<String, Pose>,
    pub previous_fixed: BTreeMap<String, Pose>,
    pub fixed: BTreeMap<String, Pose>,
    /// MovingPoseBlend edges: previous progress, current progress, direction.
    /// Keep these unevaluated until render time, including interrupted fades.
    pub(crate) fades: Vec<[f32; 3]>,
    pub(crate) root_pitch_origin: f32,
}
impl Layer {
    pub fn new(clip: Arc<Clip>) -> Self {
        Self {
            clip,
            time: 0.,
            previous_time: 0.,
            looping: false,
            hold: false,
            weight: 1.,
            additive: false,
            bones: vec![],
            retain: Value::Null,
            group: 0,
            rate: 1.,
            mirror: false,
            visual_left: false,
            overlays: vec![],
            exclusive: false,
            amplitude: 1.,
            leg_weight: 1.,
            default_retain: 0.,
            retain_world_space: false,
            exclude: vec![],
            base: None,
            held: BTreeMap::new(),
            previous_fixed: BTreeMap::new(),
            fixed: BTreeMap::new(),
            fades: vec![],
            root_pitch_origin: 0.,
        }
    }
    fn direct(clip: &Clip, bone: &str) -> bool {
        clip.tracks.contains_key(bone)
            || clip.tracks.contains_key("waist")
                && matches!(bone, "body" | "head" | "right_arm" | "left_arm")
    }
    pub(crate) fn owns(&self, bone: &str) -> bool {
        if self.exclude.iter().any(|b| b == bone)
            || !self.bones.is_empty() && !self.bones.iter().any(|b| b == bone)
        {
            return false;
        }
        let source = if self.mirror {
            opposite_bone(bone)
        } else {
            bone.to_owned()
        };
        self.fixed.contains_key(bone)
            || Self::direct(&self.clip, &source)
            || self.held.contains_key(bone)
            || self.base.as_ref().is_some_and(|c| Self::direct(c, &source))
            || self.overlays.iter().any(|o| o.owns(bone))
    }
    fn pose_at(&self, bone: &str, partial: f32) -> Option<Pose> {
        if let Some(pose) = self.fixed.get(bone) {
            return Some(self.previous_fixed.get(bone).map_or(*pose, |previous| {
                let mixed = previous.blend(*pose, motion::unit(partial));
                with_rotation(mixed, mixed.rotation)
            }));
        }
        if self.exclude.iter().any(|b| b == bone)
            || !self.bones.is_empty() && !self.bones.iter().any(|b| b == bone)
        {
            return None;
        }
        let source = if self.mirror {
            opposite_bone(bone)
        } else {
            bone.to_owned()
        };
        let time = self.previous_time + (self.time - self.previous_time) * motion::unit(partial);
        let sample = |clip: &Clip, time: f32, looped: bool| {
            let pose = if looped {
                clip.sample_model(&source, time.rem_euclid(clip.length.max(0.0001)))
            } else {
                clip.sample_model_once(&source, time.clamp(0., clip.length.max(0.)))
            };
            if self.mirror { mirrored(pose) } else { pose }
        };
        let mut owned = Self::direct(&self.clip, &source);
        let mut pose = if owned {
            sample(&self.clip, time, self.looping && !self.hold)
        } else {
            Pose::default()
        };
        for overlay in &self.overlays {
            if let Some(addition) = overlay.pose_at(bone, partial) {
                owned = true;
                if overlay.additive {
                    pose = additive(pose, addition, overlay.weight);
                } else {
                    pose = pose.blend(addition, overlay.weight);
                }
                if overlay.exclusive {
                    break;
                }
            }
        }
        if let Some(base) = self.base.as_ref().filter(|c| Self::direct(c, &source)) {
            pose = additive(sample(base, base.length, false), pose, 1.);
            owned = true;
        }
        if !owned {
            return self.held.get(bone).copied();
        }
        if bone == "root" && self.root_pitch_origin != 0. {
            pose.euler.x -= self.root_pitch_origin;
            pose.rotation = quaternion(pose.euler);
        }
        if self.amplitude != 1. {
            pose.position *= self.amplitude;
            pose.euler *= self.amplitude;
            pose.rotation = quaternion(pose.euler);
            pose.scale = Vec3::ONE + (pose.scale - Vec3::ONE) * self.amplitude;
        }
        Some(pose)
    }
    fn retention(&self, bone: &str) -> f32 {
        let source = if (self.visual_left || self.mirror) && !self.retain_world_space {
            opposite_bone(bone)
        } else {
            bone.into()
        };
        let value = self.retain.get(&source).or_else(|| {
            matches!(bone, "body" | "head" | "left_arm" | "right_arm")
                .then(|| self.retain.get("waist"))
                .flatten()
        });
        value
            .map_or(self.default_retain, |v| number(v, self.default_retain))
            .clamp(0., 1.)
    }
    pub(crate) fn render_weight(&self, partial: f32) -> f32 {
        self.fades
            .iter()
            .fold(motion::finite(self.weight, 0.).max(0.), |weight, edge| {
                let alpha = motion::smooth(edge[0] + (edge[1] - edge[0]) * motion::unit(partial));
                weight * if edge[2] < 0. { 1. - alpha } else { alpha }
            })
    }
    pub(crate) fn advance(&mut self) {
        self.previous_time = self.time;
        self.time = (self.time + STEP * self.rate).clamp(-86400., 86400.);
        // Keep elapsed time for timeline crossings; sampling itself clamps/wraps.
        for overlay in &mut self.overlays {
            overlay.advance();
        }
    }
    pub(crate) fn restrict_to_hands(&mut self) {
        self.exclude
            .extend(["root", "waist", "body", "head", "left_leg", "right_leg"].map(str::to_owned));
    }
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub layers: Vec<Layer>,
    pub tilt: [f32; 2],
    pub previous_tilt: [f32; 2],
    pub vertical_offset: f32,
    pub body_yaw: Option<f32>,
    pub head_aim: f32,
    pub crouch: f32,
}
impl Frame {
    /// Compose onto the actual native pose. An additive layer is applied once;
    /// unowned native limbs retain their current pose rather than the bind pose.
    pub fn compose(&self, bone: &str, partial: f32, mut base: Pose) -> Option<Pose> {
        let mut groups: BTreeMap<u16, Vec<&Layer>> = BTreeMap::new();
        for layer in &self.layers {
            groups.entry(layer.group).or_default().push(layer);
        }
        let mut changed = false;
        for layers in groups.values() {
            let mut sample = Pose::default();
            let mut total = 0_f32;
            let mut retained = 0_f32;
            let mut addition = true;
            for layer in layers {
                let Some(pose) = layer.pose_at(bone, partial) else {
                    continue;
                };
                let weight = layer.render_weight(partial)
                    * if bone.ends_with("_leg") {
                        layer.leg_weight
                    } else {
                        1.
                    };
                if weight <= 0.00001 {
                    continue;
                }
                let t = weight / (total + weight);
                sample = with_rotation(
                    sample.blend(pose, t),
                    sample.rotation.slerp(pose.rotation, t),
                );
                retained += (layer.retention(bone) - retained) * t;
                total += weight;
                addition &= layer.additive;
            }
            if total <= 0.00001 {
                continue;
            }
            changed = true;
            let weight = total.min(1.);
            if addition {
                base = additive(base, sample, weight);
            } else {
                let target = Pose {
                    position: base.position * retained + sample.position,
                    rotation: quaternion(base.euler * retained) * sample.rotation,
                    scale: (base.scale - Vec3::ONE) * retained + sample.scale,
                    euler: Vec3::ZERO,
                };
                let target = with_rotation(target, target.rotation);
                base = with_rotation(
                    base.blend(target, weight),
                    base.rotation.slerp(target.rotation, weight),
                );
            }
        }
        changed.then_some(base)
    }
    /// Returns a normalized pose and its independent vanilla ownership weight.
    /// Keeping those separate prevents the old double-fade-to-bind-pose bug.
    pub fn bone(&self, bone: &str, partial: f32) -> Option<(Pose, f32)> {
        let mut groups: BTreeMap<u16, Vec<&Layer>> = BTreeMap::new();
        for layer in &self.layers {
            groups.entry(layer.group).or_default().push(layer);
        }
        let mut result = Pose::default();
        let mut ownership = 0_f32;
        for layers in groups.values() {
            let mut sample = Pose::default();
            let mut total = 0_f32;
            let mut retain = 0_f32;
            let mut addition = true;
            for layer in layers {
                let Some(pose) = layer.pose_at(bone, partial) else {
                    continue;
                };
                let weight = layer.render_weight(partial)
                    * if bone.ends_with("_leg") {
                        layer.leg_weight
                    } else {
                        1.
                    };
                if weight < 0.00001 {
                    continue;
                }
                let t = weight / (total + weight);
                sample = with_rotation(
                    sample.blend(pose, t),
                    sample.rotation.slerp(pose.rotation, t),
                );
                retain += (layer.retention(bone) - retain) * t;
                total += weight;
                addition &= layer.additive;
            }
            if total <= 0.00001 {
                continue;
            }
            let weight = total.min(1.);
            if addition {
                result = additive(result, sample, weight);
                ownership = ownership.max(weight);
            } else {
                let target = Pose {
                    position: result.position * retain + sample.position,
                    rotation: quaternion(result.euler * retain) * sample.rotation,
                    scale: (result.scale - Vec3::ONE) * retain + sample.scale,
                    euler: Vec3::ZERO,
                };
                let target = with_rotation(target, target.rotation);
                let combined = ownership * (1. - weight) + weight;
                let t = if combined > 0. { weight / combined } else { 0. };
                result = with_rotation(
                    result.blend(target, t),
                    result.rotation.slerp(target.rotation, t),
                );
                ownership = combined;
            }
        }
        (ownership > 0.00001
            && result.position.is_finite()
            && result.rotation.is_finite()
            && result.scale.is_finite())
        .then_some((result, ownership.clamp(0., 1.)))
    }
    /// Waist inheritance is already evaluated by Clip::sample_model_once.
    /// Only the root is a remaining parent transform.
    pub fn parent_matrix(&self, _bone: &str, partial: f32) -> Mat4 {
        let root = self
            .compose("root", partial, Pose::default())
            .map_or(Mat4::IDENTITY, |p| {
                Mat4::from_scale_rotation_translation(p.scale, p.rotation, p.position)
            });
        let tilt: [f32; 2] = std::array::from_fn(|n| {
            self.previous_tilt[n] + (self.tilt[n] - self.previous_tilt[n]) * motion::unit(partial)
        });
        Mat4::from_translation(Vec3::new(0., self.vertical_offset, 0.))
            * crate::moves_dynamics::tilt_matrix(tilt[0], tilt[1])
            * root
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationEvent {
    pub clip: String,
    pub name: String,
    pub kind: EventKind,
    pub hand: Option<usize>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventKind {
    Timeline,
    Sound,
    Effect,
}

#[derive(Clone, Debug, Default)]
pub struct Controller {
    pub frame: Frame,
    pub camera: Option<Value>,
    pub signals: motion::Signals,
    /// Resource-declared timeline/sound/effect notifications, drained per tick.
    pub events: Vec<AnimationEvent>,
    pub state: State,
    previous: Input,
    initialized: bool,
    ticks: u64,
    gait: motion::Gait,
    fluid: motion::Fluid,
    base: Channel,
    crouch: sequence::Crouch,
    jump: Channel,
    landing: Channel,
    sprint_event: Channel,
    status: Channel,
    biome: Channel,
    biome_event: Channel,
    cold: Channel,
    music: Channel,
    idle_event: Channel,
    emote: Channel,
    emote_name: Option<String>,
    alignment: Channel,
    item_sequence: sequence::ItemSequence,
    held: [Channel; 2],
    item_use: [Channel; 2],
    equip: [Channel; 2],
    action: Channel,
    use_action: Channel,
    hurt: Channel,
    armor: Channel,
    shield: Channel,
    interaction: Channel,
    firework: Channel,
    map_pose: Channel,
    turn: turning::Turning,
    map: crate::moves_dynamics::MapView,
    proximity: crate::moves_dynamics::Proximity,
    lean: crate::moves_dynamics::TurnLean,
    passenger_tilt: crate::moves_dynamics::PassengerTilt,
    direction_lean: motion::DirectionLean,
    variables: BTreeMap<String, f64>,
    fall_peak: f32,
    fall_speed: f32,
    fall_class: u8,
    landing_class: u8,
    throw_kind: u8,
    airborne_ticks: u32,
    jump_hold: f32,
    jump_family: String,
    jump_moving: bool,
    next_hop_right: bool,
    use_ticks: u32,
    attack_combo: usize,
    last_attack: u64,
    pending_attacks: u8,
    pending_age: u8,
    idle_seconds: f32,
    idle_cooldown: f32,
    biome_cooldown: f32,
    last_idle_variant: usize,
    seed: u64,
    cold_latched: bool,
    music_movement: f32,
    music_counts: [u8; 3],
    music_stopping: bool,
    elytra_bank: f32,
    elytra_high: f32,
    fly_pitch: f32,
    fly_roll: f32,
    swim_pitch: f32,
    swim_submerged: bool,
    shallow: motion::ShallowWater,
    swim_playback: f32,
    head_aim: f32,
    flight_peak_y: Option<f32>,
    heroic_landing: bool,
    pose_remaining: f32,
    pose_item: String,
}
impl Controller {
    pub fn stop_emote(&mut self) {
        if self.emote_name.take().is_some() {
            self.emote.stop(0.12);
        }
    }
    pub fn stop_action(&mut self) {
        self.stop_emote();
        self.action.stop(0.1);
        self.use_action.stop(0.1);
        self.pending_attacks = 0;
        self.item_sequence.suspend();
    }
    pub fn is_emote_playing(&self) -> bool {
        self.emote_name.is_some()
    }
    pub fn play_emote(&mut self, pack: &Pack, name: &str) -> bool {
        let entry = pack.config["emotes"]["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|entry| entry["id"] == name || entry["animation"] == name);
        let clip_name = entry.and_then(|e| e["animation"].as_str()).unwrap_or(name);
        let Some(clip) = pack.resolve(clip_name, false) else {
            return false;
        };
        let retrigger =
            number(&pack.config["emotes"]["retrigger_progress"], 50.).clamp(0., 100.) / 100.;
        if self.emote_name.is_some() && self.emote.playback.progress() < retrigger {
            return false;
        }
        if self.initialized
            && (!self.state.ordinary() || !self.previous.grounded || self.gait.moving)
        {
            return false;
        }
        let mut layer = Layer::new(clip);
        // Greeting/emotes use OneShotLayer in the original, including clips
        // whose Bedrock metadata says loop; do not make them uninterruptible.
        layer.looping = false;
        layer.hold = false;
        self.emote.playback.play(vec![layer], 0.125, false);
        self.emote.entry = entry.cloned().unwrap_or(Value::Null);
        self.emote_name = Some(clip_name.into());
        self.music.stop(0.2);
        self.idle_event.stop(0.1);
        true
    }
    fn context(&mut self, pack: &Pack, i: &Input) -> BTreeMap<String, f64> {
        let mut context = self.signals.values.clone();
        let hand = i.using.map_or(0, |(h, _)| h);
        let body = i.body_yaw.unwrap_or(i.yaw);
        let extra = [
            ("head_pitch", i.pitch),
            ("head_yaw", motion::wrap(i.yaw - body)),
            ("body_yaw", body),
            ("fluid_depth", i.fluid_depth.unwrap_or(0.)),
            ("height_above_ground", i.ground_clearance.unwrap_or(0.)),
            ("air_gap", i.air_gap.unwrap_or(0.)),
            ("item_use_ticks", self.use_ticks as f32),
            ("item_charge", motion::unit(self.use_ticks as f32 / 20.)),
            ("is_sneaking", i.crouching as u8 as f32),
            ("is_sprinting", i.sprinting as u8 as f32),
            ("is_on_ground", i.grounded as u8 as f32),
            ("is_swimming", i.swimming as u8 as f32),
            ("is_climbing", i.climbing as u8 as f32),
            ("is_crawling", i.crawling as u8 as f32),
            ("is_burning", i.burning as u8 as f32),
            ("is_riding", i.riding.is_some() as u8 as f32),
            ("is_using_item", i.using.is_some() as u8 as f32),
            (
                "is_visual_hand_left",
                (i.main_left != (hand == 1)) as u8 as f32,
            ),
            ("is_item_loaded", i.charged as u8 as f32),
        ];
        for (name, value) in extra {
            context.insert(name.into(), f64::from(value));
        }
        context.insert(
            "is_blocking".into(),
            context.get("blocking").copied().unwrap_or(0.),
        );
        context.insert("still_seconds".into(), f64::from(self.idle_seconds));
        // Variables can reference the previous tick, but cycles and every
        // dependent of a cycle preserve their previous values.
        let Some(definitions) = pack.config["variables"].as_object() else {
            self.variables.clear();
            return context;
        };
        let mut bindings = BTreeMap::new();
        for (name, binding) in definitions.iter().take(256) {
            let value = binding
                .get("value")
                .or_else(|| binding.get("expression"))
                .unwrap_or(binding);
            bindings.insert(name.clone(), value.clone());
        }
        self.variables.retain(|name, _| bindings.contains_key(name));
        let names: BTreeSet<String> = bindings.keys().cloned().collect();
        let dependencies: BTreeMap<String, BTreeSet<String>> = bindings
            .iter()
            .map(|(name, value)| {
                let source = value.as_str().unwrap_or("");
                let references = source
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
                    .filter(|token| names.contains(*token))
                    .map(str::to_owned)
                    .collect();
                (name.clone(), references)
            })
            .collect();
        let mut pending = names;
        for (name, value) in &self.variables {
            context.insert(name.clone(), *value);
        }
        for _ in 0..256 {
            let ready: Vec<String> = pending
                .iter()
                .filter(|name| dependencies[*name].iter().all(|d| !pending.contains(d)))
                .cloned()
                .collect();
            if ready.is_empty() {
                break;
            }
            for name in ready {
                let old = self.variables.get(&name).copied().unwrap_or(0.) as f32;
                let value = expr_number(bindings.get(&name), &context, old);
                self.variables.insert(name.clone(), f64::from(value));
                context.insert(name.clone(), f64::from(value));
                pending.remove(&name);
            }
        }
        for name in pending {
            let previous = *self.variables.entry(name.clone()).or_insert(0.);
            context.insert(name, previous);
        }
        context
    }
    fn select_state(&mut self, pack: &Pack, i: &Input) -> State {
        if let Some(ride) = &i.riding {
            let passenger = pack.entry("GENERAL", "passenger_state");
            return if passenger["entities"].as_array().is_some_and(|a| {
                a.iter()
                    .any(|p| p.as_str().is_some_and(|p| matches_id(p, ride)))
            }) {
                State::Passenger
            } else {
                State::Sit
            };
        }
        if i.elytra {
            return State::Elytra;
        }
        if i.flying && !i.grounded {
            return State::Fly;
        }
        if i.climbing {
            return State::Climb;
        }
        if i.crawling && !i.in_water && !i.underwater {
            return State::Crawl;
        }
        if let Some(state) = self.fluid.state {
            return state;
        }
        if i.swimming && !i.on_water && i.in_water {
            return State::Swim;
        }
        if i.grounded {
            self.fall_class = 0;
            return State::Ground;
        }
        let falling = pack.entry("GENERAL", "falling");
        let distance = i
            .fall_distance
            .max(i.projected_landing_distance.unwrap_or(0.));
        if !i.in_water
            && !i.in_lava
            && (self.state == State::Falling
                || i.vertical_speed < -0.05
                    && distance > number(&falling["trigger_distance"], 5.).max(5.))
        {
            self.fall_class = self.fall_class.max(motion::weight_class(
                distance,
                -i.vertical_speed,
                &falling,
                true,
            ));
            State::Falling
        } else {
            State::Air
        }
    }
    pub fn tick(&mut self, pack: &Pack, input: &Input) {
        let mut i = input.bounded();
        if i.teleported {
            self.initialized = false;
            self.fluid = Default::default();
            self.turn = Default::default();
            self.fall_peak = 0.;
            self.fall_speed = 0.;
            self.shallow = Default::default();
        }
        if i.water_probes {
            self.shallow.tick(&i);
            i.on_water = self.shallow.active;
        }
        self.events.clear();
        if !self.initialized {
            self.next_hop_right = true;
            self.head_aim = 1.;
        }
        self.ticks = self.ticks.wrapping_add(1);
        self.camera = None;
        if self.seed == 0 {
            self.seed = motion::hash(i.random_seed.wrapping_add(1));
        }
        if i.dead || i.sleeping || i.spectator {
            self.stop_action();
            self.frame = Frame::default();
            self.previous = i;
            self.initialized = false;
            return;
        }
        let previous_state = self.state;
        let water = pack.entry("GENERAL", "water_dive");
        let lava = pack.entry("GENERAL", "lava_dive");
        self.fluid.tick(
            &i,
            &self.previous,
            &water,
            &lava,
            self.state == State::WaterContact && self.base.playback.finished(),
            self.state == State::WaterLoop
                && self.base.playback.age >= self.base.playback.length.max(0.05),
        );
        self.state = self.select_state(pack, &i);
        if self.state != State::Falling {
            self.fall_class = 0;
        }
        let walking = pack.entry("GENERAL", "walking");
        self.gait.tick(
            &i,
            &self.previous,
            self.state,
            &walking,
            &pack.entry("GENERAL", "run"),
        );
        self.signals.capture(&i, self.state, &self.gait);
        let left_ground = self.initialized && self.previous.grounded && !i.grounded;
        let landed =
            self.initialized && !self.previous.grounded && i.grounded && self.airborne_ticks >= 2;
        if i.grounded {
            self.airborne_ticks = 0;
        } else {
            self.airborne_ticks = self.airborne_ticks.saturating_add(1);
            self.fall_peak = self.fall_peak.max(i.fall_distance);
            self.fall_speed = self.fall_speed.max(-i.vertical_speed);
        }
        self.use_ticks = if i.using.is_some() {
            i.use_ticks.unwrap_or(if i.using == self.previous.using {
                self.use_ticks.saturating_add(1)
            } else {
                0
            })
        } else {
            0
        };
        if self.gait.moving {
            self.idle_seconds = 0.;
        } else {
            self.idle_seconds = (self.idle_seconds + STEP).min(86400.);
        }
        let mut context = self.context(pack, &i);
        context.insert("time".into(), self.ticks as f64 * f64::from(STEP));
        if self.emote_name.is_some()
            && (!self.state.ordinary()
                || !i.grounded
                || self.gait.moving
                || i.attack
                || i.mining
                || i.using.is_some()
                || i.used.is_some()
                || i.hurt
                || i.force_equip
                || i.external_attack)
        {
            self.stop_emote();
        }
        if i.alignment && !self.previous.alignment {
            let entry = pack.entry("GENERAL", "alignment");
            self.alignment
                .trigger(pack, &entry, "clip", false, false, &context, 0, true);
        }
        self.alignment.tick(true);
        self.emote.tick(true);
        if self.emote.playback.finished() {
            self.emote_name = None;
        }
        self.item_sequence
            .tick(pack, &i, self.state, &context, self.emote_name.is_some());
        self.tick_base(pack, &i, &context, previous_state);
        self.tick_traversal(pack, &i, &context, left_ground, landed);
        self.tick_items(pack, &i, &context);
        self.tick_ambient(pack, &i, &context, left_ground);
        self.tick_camera(pack, &i);
        self.tick_frame(pack, &i, &context, previous_state);
        self.signals.values.extend([
            ("still_seconds".into(), f64::from(self.idle_seconds)),
            ("fall_weight".into(), f64::from(self.fall_class)),
            ("landing_weight".into(), f64::from(self.landing_class)),
            ("thrown".into(), f64::from(self.throw_kind)),
            ("leaping".into(), f64::from(self.jump_moving)),
            ("elytra_bank".into(), f64::from(self.elytra_bank)),
        ]);
        if self.initialized {
            if i.crouching && !self.previous.crouching {
                self.signals.event("crouched");
            }
            if !i.crouching && self.previous.crouching {
                self.signals.event("stood_up");
            }
            if i.in_water && !self.previous.in_water && i.vertical_speed < -0.4 {
                self.signals.event("splashed");
            }
            if i.swapped {
                self.signals.event("swapped");
            }
        }
        self.previous = i;
        self.initialized = true;
    }
    fn tick_base(&mut self, pack: &Pack, i: &Input, context: &BTreeMap<String, f64>, old: State) {
        let defaults = |key| pack.entry("GENERAL", key);
        if self.state == State::Swim {
            if i.underwater {
                self.swim_submerged = true;
            } else if let (Some(surface), Some(eye)) = (i.water_surface_y, i.eye_y) {
                if surface - eye >= 0.1 {
                    self.swim_submerged = true;
                } else if surface - eye <= -0.05 {
                    self.swim_submerged = false;
                }
            } else {
                self.swim_submerged = false;
            }
        } else {
            self.swim_submerged = false;
            self.swim_playback = 0.;
        }
        let swim_intent = motion::swim_intent(i);
        let (entry, field, looping) = match self.state {
            State::Ground | State::Air | State::Crawl => (defaults("idle"), "clip", true),
            State::Climb => (defaults("climbing"), "clip", true),
            State::Fly => (defaults("creative_fly"), "pose", false),
            State::Elytra => (defaults("elytra"), "normal", true),
            State::Falling => (
                defaults("falling"),
                if self.fall_class >= 2 && defaults("falling")["free_fall"].is_string() {
                    "free_fall"
                } else {
                    "normal"
                },
                true,
            ),
            State::Swim => (
                defaults("swimming"),
                if swim_intent[1] != 0. && defaults("swimming")["vertical"].is_string() {
                    "vertical"
                } else if self.swim_submerged && defaults("swimming")["underwater"].is_string() {
                    "underwater"
                } else if defaults("swimming")["surface"].is_string() {
                    "surface"
                } else if defaults("swimming")["underwater"].is_string() {
                    "underwater"
                } else {
                    "vertical"
                },
                true,
            ),
            State::WaterAir => (defaults("water_dive"), self.fluid.tuck.key(false), false),
            State::WaterContact => (defaults("water_dive"), "contact", false),
            State::WaterLoop => (defaults("water_dive"), self.fluid.tuck.key(true), true),
            State::LavaAir | State::LavaContact => (defaults("lava_dive"), "pose", false),
            State::Passenger => (
                defaults("passenger_state"),
                if i.speed.max(i.vehicle_velocity.length())
                    > number(&defaults("passenger_state")["speed_threshold"], 0.12)
                {
                    "moving"
                } else {
                    "idle"
                },
                true,
            ),
            State::Sit => (defaults("sitting_state"), "pose", false),
        };
        let key = format!("{}:{field}", self.state.key());
        let swim_handoff = old == State::Swim && self.state == State::Swim && self.base.key != key;
        let swim_phase = self.base.playback.current.first().map_or(0., |layer| {
            layer.time.rem_euclid(layer.clip.length.max(0.0001)) / layer.clip.length.max(0.0001)
        });
        self.base
            .set(pack, &key, &entry, field, false, false, looping, context);
        if swim_handoff {
            self.base.playback.duration_override(0.2);
            self.base
                .playback
                .seek(swim_phase * self.base.playback.length);
            for layer in &mut self.base.playback.current {
                layer.previous_time = layer.time;
            }
        }
        if old != self.state {
            self.base
                .playback
                .duration_override(if self.state == State::Falling {
                    1. / number(&entry["blend_in_response"], 1.2).clamp(0.05, 100.)
                } else if self.state == State::Swim
                    && matches!(old, State::WaterContact | State::WaterLoop)
                {
                    number(&defaults("water_dive")["swim_blend_seconds"], 0.6)
                } else if matches!(self.state, State::WaterContact | State::WaterLoop) {
                    number(&defaults("water_dive")["entry_blend_seconds"], 0.45)
                } else {
                    number(&entry["transition_seconds"], 0.16)
                });
        }
        if self.state == State::Climb
            && text(&entry["timer_effect"], "").replace('_', "") == "verticaldelta"
        {
            let delta = i.vertical_displacement.unwrap_or(i.vertical_speed);
            self.base
                .playback
                .rate(delta * number(&entry["timer_scale"], 8.) / STEP);
        }
        if self.state == State::Swim {
            let vertical = field == "vertical" && swim_intent[1] != 0.;
            let moving = vertical || swim_intent[2] > 0.01 || i.speed > 0.015;
            let speed = if vertical {
                i.speed.hypot(i.vertical_speed)
            } else {
                i.speed.max(swim_intent[2] * 0.09)
            };
            let mut rate = if moving {
                (speed / 0.2).clamp(0.45, 1.)
            } else {
                0.
            };
            if !vertical && moving && i.sprinting {
                rate *= number(&entry["sprint_playback_multiplier"], 1.25).clamp(1., 1.25);
            }
            if !vertical && swim_intent[0] < -0.01 {
                rate = -rate;
            }
            rate *= sequence::timer_rate(&entry, context);
            self.swim_playback += (rate - self.swim_playback) * 0.35;
            self.base.playback.rate(self.swim_playback);
        }
        self.base.playback.tick();
    }
    fn trigger_event(
        channel: &mut Channel,
        pack: &Pack,
        action: &str,
        field: &str,
        context: &BTreeMap<String, f64>,
        force: bool,
    ) {
        channel.trigger(
            pack,
            &pack.entry("GENERAL", action),
            field,
            false,
            false,
            context,
            0,
            force,
        );
    }
    fn tick_traversal(
        &mut self,
        pack: &Pack,
        i: &Input,
        context: &BTreeMap<String, f64>,
        left_ground: bool,
        landed: bool,
    ) {
        let ordinary = self.state.ordinary();
        if !ordinary {
            self.jump.stop(0.1);
            self.landing.stop(0.1);
            self.sprint_event.stop(0.12);
        }
        let remote_takeoff = i.remote && self.airborne_ticks <= 4;
        if ordinary
            && (left_ground || remote_takeoff && !self.jump.playback.has_clip())
            && i.vertical_speed > 0.04
        {
            let entry = pack.entry("GENERAL", "jump");
            let family = if self.gait.moving && i.sprinting {
                "running"
            } else if self.gait.moving {
                "moving"
            } else {
                "idle"
            };
            self.jump_moving = self.gait.moving;
            let side = if self.next_hop_right { "right" } else { "left" };
            let families = if family == "running" {
                vec!["running", "moving", "idle"]
            } else {
                vec![family, "idle"]
            };
            for family in families {
                let mut selected = entry[family].clone();
                if selected.is_null() {
                    selected = entry.clone();
                }
                let other = if side == "left" { "right" } else { "left" };
                if selected[side].is_null() {
                    selected[side] = selected[other].clone();
                }
                if self
                    .jump
                    .trigger(pack, &selected, side, false, false, context, 0, true)
                {
                    self.jump_family = family.into();
                    let contact = selected["landing"]
                        .as_str()
                        .or_else(|| entry["landing"].as_str())
                        .and_then(|n| pack.resolve(n, false));
                    if let Some(layer) = self.jump.playback.current.first() {
                        self.jump_hold = motion::jump_hold(
                            &layer.clip,
                            contact.as_deref(),
                            number(&entry["landing_before_seconds"], 0.7),
                        );
                    }
                    self.next_hop_right = !self.next_hop_right;
                    break;
                }
            }
            self.signals.event("jumped");
            self.landing.stop(0.05);
        }
        if i.flying && !i.elytra {
            if let Some(y) = i.position_y {
                self.flight_peak_y = Some(self.flight_peak_y.unwrap_or(y).max(y));
            }
        } else if !i.grounded && self.flight_peak_y.is_none() && self.fall_peak > 4. {
            self.flight_peak_y = i.position_y.map(|y| y + self.fall_peak);
        }
        if landed
            && !matches!(
                self.state,
                State::Passenger | State::Sit | State::Climb | State::Crawl
            )
        {
            let entry = pack.entry("GENERAL", "landing");
            let class = motion::weight_class(self.fall_peak, self.fall_speed, &entry, false);
            self.landing_class = class;
            self.signals.event("landed");
            self.signals
                .values
                .insert("landing_weight".into(), class as f64);
            let jump_entry = pack.entry("GENERAL", "jump");
            let contact_name = jump_entry[&self.jump_family]["landing"]
                .as_str()
                .or_else(|| jump_entry["landing"].as_str());
            let early = self.jump.playback.has_clip()
                && !self.jump.playback.finished()
                && self.jump.playback.age < number(&jump_entry["landing_before_seconds"], 0.7)
                && contact_name.is_some();
            let heroic = pack.entry("GENERAL", "heroic_landing");
            let heroic_distance = number(&heroic["minimum_fall_distance"], 4.).max(0.);
            let heroic_contact = self
                .flight_peak_y
                .is_some_and(|y| i.position_y.is_some_and(|p| y - p >= heroic_distance))
                && !i.in_water
                && !i.in_lava
                && heroic["long_fall"] != false;
            self.heroic_landing = heroic_contact && heroic["clip"].is_string();
            if self.heroic_landing {
                self.landing
                    .trigger(pack, &heroic, "clip", false, false, context, 0, true);
            } else if early {
                let selected = serde_json::json!({"clip":contact_name,"additive":true});
                self.landing
                    .trigger(pack, &selected, "clip", false, false, context, 0, true);
            } else {
                let field = if class >= 3 {
                    "heavy"
                } else if class >= 2 {
                    "medium"
                } else {
                    "light"
                };
                let field = if entry[field].is_string() {
                    field
                } else if entry["medium"].is_string() {
                    "medium"
                } else if entry["light"].is_string() {
                    "light"
                } else {
                    "clip"
                };
                self.landing
                    .trigger(pack, &entry, field, false, false, context, 0, true);
                if entry["additive"] == true {
                    let strength = motion::landing_amplitude(
                        self.fall_speed,
                        number(&entry["full_strength_distance"], 5.),
                    );
                    for layer in &mut self.landing.playback.current {
                        layer.amplitude = strength;
                    }
                }
            }
            if self.gait.moving && i.sprinting {
                let walking = pack.entry("GENERAL", "walking");
                let name = walking["directions"]["sprint"]["forward"]["clip"]
                    .as_str()
                    .or_else(|| walking["directions"]["forward"]["clip"].as_str());
                if let Some(clip) = name.and_then(|n| pack.resolve(n, false)) {
                    let previous = &self.frame;
                    self.gait.phase = motion::match_phase(self.gait.phase, |phase| {
                        ["left_leg", "right_leg", "root"]
                            .iter()
                            .map(|bone| {
                                previous.bone(bone, 1.).map_or(0., |(pose, _)| {
                                    motion::pose_cost(
                                        pose,
                                        clip.sample_model(bone, phase * clip.length),
                                    )
                                })
                            })
                            .sum()
                    });
                    self.gait.previous_phase = self.gait.phase;
                }
            }
            self.jump.stop(0.08);
            self.fall_peak = 0.;
            self.fall_speed = 0.;
            self.flight_peak_y = None;
        }
        if ordinary && i.grounded && i.sprinting && !self.previous.sprinting && self.gait.moving {
            self.signals.event("run_started");
            Self::trigger_event(
                &mut self.sprint_event,
                pack,
                "run_start",
                "clip",
                context,
                true,
            );
        } else if ordinary
            && i.grounded
            && self.previous.grounded
            && self.previous.sprinting
            && (!i.sprinting || !self.gait.moving)
            && self.previous.speed
                > number(&pack.entry("GENERAL", "run_stop")["minimum_speed"], 0.08)
        {
            self.signals.event("run_stopped");
            Self::trigger_event(
                &mut self.sprint_event,
                pack,
                "run_stop",
                if self.gait.moving {
                    "to_walk"
                } else {
                    "to_idle"
                },
                context,
                true,
            );
        }
        if ordinary && i.crouching != self.previous.crouching {
            self.signals
                .event(if i.crouching { "crouched" } else { "stood_up" });
        }
        self.crouch.tick(pack, i, ordinary, context);
        self.jump.tick(false);
        if !i.grounded && self.jump.playback.has_clip() {
            self.jump.playback.hold_at(self.jump_hold);
        }
        if i.grounded {
            self.jump.stop(0.1);
        }
        if self.heroic_landing
            && (self.gait.moving || i.attack || i.using.is_some() || i.crouching || !i.grounded)
        {
            self.landing.stop(0.15);
            self.heroic_landing = false;
        }
        self.landing.tick(true);
        self.sprint_event.tick(true);
    }
    fn tick_items(&mut self, pack: &Pack, i: &Input, context: &BTreeMap<String, f64>) {
        let ordinary = self.state.ordinary();
        let hands_only = self.state.hands_only();
        let riding = i.riding.is_some();
        let active_allowed = ordinary || hands_only || riding;
        let passive_allowed = ordinary && !i.fence && !i.on_water && self.emote_name.is_none();
        let sequence_owns = !sequence::ItemSequence::behavior(pack, &i.items[0]).is_empty();
        let consumed = i
            .using
            .is_some_and(|(_, using)| matches!(using, Use::Eat | Use::Drink));
        let action_busy = self.action.playback.has_clip()
            || self.use_action.playback.has_clip()
            || self.interaction.playback.has_clip()
            || i.external_attack;
        for hand in 0..2 {
            let id = &i.items[hand];
            let left = i.main_left != (hand == 1);
            let one = !i.items[1 - hand].is_empty();
            let behavior = sequence::ItemSequence::behavior(pack, id);
            let declared_use = sequence::declared(pack, id, "use_item") && behavior.is_empty();
            let using = i.using.is_some_and(|(h, _)| h == hand);
            let changed = self.initialized
                && (id != &self.previous.items[hand]
                    || hand == 0 && i.selected_slot != self.previous.selected_slot);
            if ordinary && (changed || i.force_equip || i.swapped) && behavior.is_empty() {
                let action = if i.swapped {
                    "item_change"
                } else if id.is_empty() {
                    "item_switch"
                } else if self.previous.items[hand].is_empty() {
                    "hand_in"
                } else {
                    "item_change"
                };
                let mut entry = sequence::item_entry(pack, id, action);
                if entry.is_null() {
                    entry = sequence::item_entry(pack, id, "hand_in");
                }
                self.equip[hand].trigger(pack, &entry, "clip", left, one, context, 0, true);
                self.signals.event("equipped");
                self.pose_item = id.clone();
                self.pose_remaining = number(
                    &sequence::item_entry(pack, id, "pose")["duration_seconds"],
                    -1.,
                );
            }
            if self.pose_item == *id
                && self.pose_remaining > 0.
                && !self.equip[hand].playback.has_clip()
            {
                self.pose_remaining = (self.pose_remaining - STEP).max(0.);
            }
            if passive_allowed
                && behavior.is_empty()
                && !using
                && !action_busy
                && !declared_use
                && !(sequence_owns && self.item_sequence.hand == hand)
                && !(self.pose_item == *id && self.pose_remaining == 0.)
            {
                let action = if sequence::declared(pack, id, "idle") {
                    "idle"
                } else {
                    "pose"
                };
                if sequence::declared(pack, id, action) {
                    let entry = sequence::item_entry(pack, id, action);
                    self.held[hand].set(
                        pack,
                        &format!("{id}:{action}:{left}:{one}"),
                        &entry,
                        "clip",
                        left,
                        one,
                        true,
                        context,
                    );
                } else {
                    self.held[hand].stop(0.12);
                }
            } else {
                self.held[hand].stop(0.12);
            }
            if declared_use
                && active_allowed
                && !consumed
                && (!action_busy || using)
                && self.emote_name.is_none()
            {
                let action = if using { "use_item" } else { "pose" };
                let entry = sequence::item_entry(pack, id, action);
                if using || passive_allowed {
                    self.item_use[hand].set(
                        pack,
                        &format!("{id}:{action}:{left}:{one}"),
                        &entry,
                        "clip",
                        left,
                        one,
                        true,
                        context,
                    );
                } else {
                    self.item_use[hand].stop(0.12);
                }
            } else if using && active_allowed && behavior.is_empty() {
                let using_type = i.using.map(|(_, u)| u).unwrap_or(Use::Other);
                let group = crate::moves_pack::item_group(id);
                let (action, field, looped) = match using_type {
                    Use::Eat => ("eating", "looping", true),
                    Use::Drink => ("drinking", "looping", true),
                    Use::Other => match group {
                        "CROSSBOW" => (
                            "crossbow",
                            if self.use_ticks as f32 * STEP
                                >= self.item_use[hand].playback.length.max(0.05)
                                && self.item_use[hand].key.contains("charge")
                                || self.item_use[hand].key.contains("charge_idle")
                            {
                                "charge_idle"
                            } else {
                                "charge"
                            },
                            false,
                        ),
                        "SPEAR" => ("spear", "charge", false),
                        "TRIDENT" => ("trident", "charge", false),
                        "SPYGLASS" => ("spyglass_use", "clip", false),
                        _ => ("use_item", "clip", true),
                    },
                };
                let entry = if matches!(using_type, Use::Eat | Use::Drink) {
                    pack.entry("CONSUMABLE", action)
                } else {
                    sequence::item_entry(pack, id, action)
                };
                self.item_use[hand].set(
                    pack,
                    &format!("{id}:{action}:{field}:{left}:{one}"),
                    &entry,
                    field,
                    left,
                    one,
                    looped,
                    context,
                );
            } else if passive_allowed
                && behavior.is_empty()
                && crate::moves_pack::item_group(id) == "CROSSBOW"
            {
                let entry = sequence::item_entry(pack, id, "crossbow");
                let field = if i.charged { "charged" } else { "idle" };
                self.item_use[hand].set(
                    pack,
                    &format!("{id}:crossbow:{field}:{left}"),
                    &entry,
                    field,
                    left,
                    one,
                    true,
                    context,
                );
            } else {
                self.item_use[hand].stop(0.12);
            }
            if hands_only {
                for layer in &mut self.item_use[hand].playback.current {
                    layer.restrict_to_hands();
                }
            }
            self.held[hand].tick(false);
            self.item_use[hand].tick(false);
            self.equip[hand].tick(true);
        }
        if i.attack || i.attack_presses > 0 {
            // The reference queues one primary request, not an auto-play list
            // of every click made while an attack was waiting to retrigger.
            self.pending_attacks = 1;
            self.pending_age = 0;
        } else {
            self.pending_age = self.pending_age.saturating_add(1);
        }
        if self.pending_age > 24 {
            self.pending_attacks = 0;
        }
        if sequence_owns || i.external_attack || i.force_equip || i.swapped {
            self.pending_attacks = 0;
        }
        if active_allowed
            && !consumed
            && !i.external_attack
            && !sequence_owns
            && self.pending_attacks > 0
            && !i.mining
        {
            let group = crate::moves_pack::item_group(&i.items[0]);
            let spear = group == "SPEAR";
            let mut entry =
                sequence::item_entry(pack, &i.items[0], if spear { "spear" } else { "attack" });
            if spear && entry["attack"].is_null() {
                entry = sequence::item_entry(pack, &i.items[0], "attack");
            }
            let charge = i.attack_strength.unwrap_or(1.);
            let required = number(
                &entry["minimum_attack_strength"],
                if spear {
                    number(&entry["attack_ready_threshold"], 0.9)
                } else {
                    0.
                },
            );
            if charge >= required {
                if self.ticks.saturating_sub(self.last_attack) > 20 {
                    self.attack_combo = 0;
                }
                if self.action.trigger(
                    pack,
                    &entry,
                    if spear { "attack" } else { "clip" },
                    i.main_left,
                    !i.items[1].is_empty(),
                    context,
                    self.attack_combo,
                    false,
                ) {
                    self.attack_combo = self.attack_combo.wrapping_add(1);
                    self.last_attack = self.ticks;
                    self.pending_attacks = self.pending_attacks.saturating_sub(1);
                    if spear {
                        self.signals.event("lunged");
                    }
                }
            } else {
                self.pending_attacks = self.pending_attacks.saturating_sub(1);
            }
        }
        if i.mining && active_allowed && !consumed && !i.external_attack && !sequence_owns {
            let entry = sequence::item_entry(pack, &i.items[0], "mining");
            self.action.set(
                pack,
                &format!("mining:{}:{}", i.items[0], i.main_left),
                &entry,
                "clip",
                i.main_left,
                !i.items[1].is_empty(),
                true,
                context,
            );
            // Mining is an explicit continuous loop even if clip metadata is once.
            self.action.playback.looping = true;
            for layer in &mut self.action.playback.current {
                layer.looping = true;
            }
        } else if self.action.key.starts_with("mining:") {
            self.action.stop(0.12);
        }
        if let Some(hand) = i.used.filter(|_| active_allowed) {
            let id = i.accepted_item.as_deref().unwrap_or(&i.items[hand]);
            if sequence::ItemSequence::behavior(pack, id).is_empty() {
                let entry = sequence::item_entry(pack, id, "use");
                let entry = if entry.is_null() {
                    sequence::item_entry(pack, id, "use_item")
                } else {
                    entry
                };
                self.use_action.trigger(
                    pack,
                    &entry,
                    "clip",
                    i.main_left != (hand == 1),
                    !i.items[1 - hand].is_empty(),
                    context,
                    0,
                    true,
                );
                self.signals.event("interacted");
            }
            match crate::moves_pack::item_group(id) {
                "THROWABLE" => {
                    self.throw_kind = match id {
                        "minecraft:ender_pearl" => 2,
                        "minecraft:splash_potion" | "minecraft:lingering_potion" => 3,
                        _ => 1,
                    };
                    self.signals.event("threw");
                }
                "FLINT" => self.signals.event("ignited"),
                "SHEARS" => self.signals.event("sheared"),
                "FISHING_ROD" => self.signals.event(if self.previous.fishing {
                    "reeled"
                } else {
                    "cast"
                }),
                _ => {}
            }
        }
        if let Some((hand, _)) = self
            .previous
            .using
            .filter(|_| i.using.is_none() && i.use_released && active_allowed)
        {
            let id = &self.previous.items[hand];
            if sequence::ItemSequence::behavior(pack, id).is_empty() && (i.use_released || i.remote)
            {
                self.use_action.trigger(
                    pack,
                    &sequence::item_entry(pack, id, "release"),
                    "clip",
                    i.main_left != (hand == 1),
                    false,
                    context,
                    0,
                    true,
                );
            }
            if id == "minecraft:trident" {
                self.signals.event("hurled");
            } else if id == "minecraft:bow" {
                self.signals.event("released");
            }
        }
        if let Some(hand) = i.crossbow_shot {
            self.signals.event("released");
            let entry = sequence::item_entry(pack, &i.items[hand], "crossbow");
            self.use_action.trigger(
                pack,
                &entry,
                "shoot",
                i.main_left != (hand == 1),
                !i.items[1 - hand].is_empty(),
                context,
                0,
                true,
            );
        }
        if let Some(hand) = i.shield_broken.or(i.shield_hit) {
            let entry = sequence::item_entry(
                pack,
                &i.items[hand],
                if i.shield_broken.is_some() {
                    "shield_break"
                } else {
                    "shield_hit"
                },
            );
            self.shield.trigger(
                pack,
                &entry,
                "clip",
                i.main_left != (hand == 1),
                true,
                context,
                0,
                true,
            );
        }
        if let Some(hand) = i.interact.filter(|_| active_allowed) {
            let entry = sequence::item_entry(pack, &i.items[hand], "interact");
            self.interaction.trigger(
                pack,
                &entry,
                "clip",
                i.main_left != (hand == 1),
                true,
                context,
                0,
                true,
            );
            for layer in &mut self.interaction.playback.current {
                layer
                    .exclude
                    .extend(["root", "left_leg", "right_leg"].map(str::to_owned));
            }
            self.signals.event("interacted");
        }
        if i.hurt && ordinary {
            let (action, opposite) = if i.hurt_from_back {
                ("hurt_forward", "hurt_backward")
            } else {
                ("hurt_backward", "hurt_forward")
            };
            Self::trigger_event(&mut self.hurt, pack, action, "clip", context, true);
            if !self.hurt.playback.has_clip() {
                Self::trigger_event(&mut self.hurt, pack, opposite, "clip", context, true);
            }
            self.signals.event("hurt");
        }
        if let Some(helmet) = i.armor_changed.filter(|_| ordinary) {
            Self::trigger_event(
                &mut self.armor,
                pack,
                if helmet { "armor_helmet" } else { "armor" },
                "clip",
                context,
                true,
            );
        }
        if i.firework_boost && self.state == State::Elytra {
            Self::trigger_event(
                &mut self.firework,
                pack,
                "elytra",
                "firework_use",
                context,
                true,
            );
            self.signals.event("boosted");
        }
        if !active_allowed {
            self.action.stop(0.1);
            self.use_action.stop(0.1);
            self.interaction.stop(0.08);
        }
        if !ordinary {
            self.hurt.stop(0.1);
            self.armor.stop(0.1);
        }
        self.action.tick(!i.mining);
        self.use_action.tick(true);
        self.hurt.tick(true);
        self.armor.tick(true);
        self.shield.tick(true);
        self.interaction.tick(true);
        self.firework.tick(true);
        let map = sequence::item_entry(pack, &i.items[0], "map_view");
        let map_active = self.map.update(
            passive_allowed && i.items[0].ends_with(":filled_map"),
            i.pitch,
            number(&map["look_down_enter"], 42.),
            number(&map["look_down_exit"], 32.),
        );
        if map_active {
            self.map_pose
                .set(pack, "map", &map, "clip", i.main_left, false, true, context);
        } else {
            self.map_pose.stop(0.2);
        }
        self.map_pose.tick(false);
        self.signals
            .values
            .insert("map_view".into(), map_active as u8 as f64);
        self.head_aim +=
            ((if consumed { 0. } else { 1. }) - self.head_aim).clamp(-0.3333333, 0.125);
    }
    fn tick_ambient(
        &mut self,
        pack: &Pack,
        i: &Input,
        context: &BTreeMap<String, f64>,
        jumped: bool,
    ) {
        let ordinary = self.state.ordinary() && i.grounded && !i.fence && !i.on_water;
        let stationary = ordinary
            && !self.gait.moving
            && !i.attack
            && !i.mining
            && i.using.is_none()
            && !i.external_attack;
        let free = !self.action.playback.has_clip()
            && !self.use_action.playback.has_clip()
            && !self.item_sequence.channel.playback.has_clip();
        let status = if stationary && i.nausea {
            Some("nausea")
        } else if stationary
            && (i.hungry
                || i.food_level.is_some_and(|level| {
                    f32::from(level)
                        <= number(
                            &pack.entry("GENERAL", "low_hunger")["food_level_at_most"],
                            6.,
                        )
                }))
        {
            Some("low_hunger")
        } else {
            None
        };
        if let Some(action) = status {
            self.status.set(
                pack,
                action,
                &pack.entry("GENERAL", action),
                "clip",
                false,
                false,
                true,
                context,
            );
        } else {
            self.status.stop(0.2);
        }
        self.status.tick(false);
        let cold = pack.entry("GENERAL", "cold");
        let progress = i.freeze_progress.unwrap_or(if i.cold { 1. } else { 0. });
        if !self.cold_latched && progress >= number(&cold["trigger_progress"], 0.05).max(0.001) {
            if ordinary {
                self.cold
                    .trigger(pack, &cold, "clip", false, false, context, 0, true);
            }
            self.cold_latched = true;
        } else if progress <= number(&cold["reset_progress"], 0.01) {
            self.cold_latched = false;
        }
        self.cold.tick(true);
        let mut biome = pack.entry("GENERAL", "biome");
        if let Some(rules) = biome["rules"].as_array()
            && let Some(rule) = rules.iter().find(|rule| {
                rule["biomes"].as_array().is_none_or(|ids| {
                    i.biome_id.as_deref().is_some_and(|id| {
                        ids.iter()
                            .any(|v| v.as_str().is_some_and(|p| matches_id(p, id)))
                    })
                }) && i.biome_temperature.map_or(i.cold, |t| {
                    t <= number(&rule["temperature_max"], f32::INFINITY)
                        && t >= number(&rule["temperature_min"], f32::NEG_INFINITY)
                })
            })
        {
            let rule = rule.clone();
            crate::moves_pack::merge(&mut biome, &rule);
        }
        let biome_active = ordinary
            && i.biome_temperature.map_or(i.cold, |t| {
                t <= number(&biome["temperature_max"], 0.15)
                    && t >= number(&biome["temperature_min"], -100.)
            });
        if biome_active {
            self.biome.set(
                pack,
                &format!("biome:{}", text(&biome["pose"], "")),
                &biome,
                "pose",
                false,
                false,
                true,
                context,
            );
            if self.biome_cooldown <= 0. {
                self.biome_cooldown = motion::interval(&biome, &mut self.seed, [12., 28.]);
            }
            if (biome["event_while_moving"] == true || !self.gait.moving)
                && !self.cold.playback.has_clip()
                && !self.biome_event.playback.has_clip()
            {
                self.biome_cooldown -= STEP;
                if self.biome_cooldown <= 0.
                    && motion::random(&mut self.seed) <= number(&biome["chance"], 1.).clamp(0., 1.)
                {
                    self.biome_event
                        .trigger(pack, &biome, "event", false, false, context, 0, true);
                    self.biome_cooldown = motion::interval(&biome, &mut self.seed, [12., 28.]);
                }
            }
        } else {
            self.biome.stop(0.3);
            self.biome_cooldown = 0.;
        }
        self.biome.tick(false);
        self.biome_event.tick(true);
        if i.greeting && stationary {
            let entry = pack.entry("GENERAL", "greeting");
            self.idle_event.trigger(
                pack,
                &entry,
                "clip",
                false,
                false,
                context,
                motion::hash(self.ticks ^ self.seed) as usize,
                true,
            );
        }
        let music = pack.entry("GENERAL", "music_listening");
        let was_music = self.music.playback.has_clip();
        if was_music {
            self.music_movement = if self.gait.moving {
                self.music_movement + STEP
            } else {
                0.
            };
            if jumped {
                self.music_counts[0] = self.music_counts[0].saturating_add(1);
            }
            if i.attack {
                self.music_counts[1] = self.music_counts[1].saturating_add(1);
            }
            if i.mining && !self.previous.mining {
                self.music_counts[2] = self.music_counts[2].saturating_add(1);
            }
            self.music_stopping |= !i.music
                || self.music_movement >= number(&music["movement_stop_delay_seconds"], 1.).max(0.)
                || self.music_counts.iter().any(|n| *n >= 2);
        }
        if !ordinary || self.emote_name.is_some() {
            self.music_stopping = true;
        }
        let start = !was_music
            && stationary
            && free
            && self.idle_seconds >= number(&music["start_delay_seconds"], 1.).max(0.);
        if i.music && !self.music_stopping && (was_music || start) && self.emote_name.is_none() {
            let left = i.main_left != (music["item_hand"] == "off_hand");
            self.music
                .set(pack, "music", &music, "loop", left, false, true, context);
        } else {
            self.music.stop(0.5);
        }
        self.music.tick(false);
        if !self.music.playback.active() {
            self.music_stopping = false;
            self.music_counts = [0; 3];
            self.music_movement = 0.;
        }
        if stationary && free && !self.music.playback.active() && self.emote_name.is_none() {
            let entry = pack.entry("GENERAL", "idle_events");
            if self.idle_cooldown <= 0. {
                self.idle_cooldown = motion::interval(&entry, &mut self.seed, [20., 45.]);
            }
            if !self.idle_event.playback.has_clip() {
                self.idle_cooldown -= STEP;
                if self.idle_cooldown <= 0. {
                    if motion::random(&mut self.seed) <= number(&entry["chance"], 1.).clamp(0., 1.)
                    {
                        let count = entry["variants"].as_array().map_or(1, Vec::len).max(1);
                        let mut variant = (motion::random(&mut self.seed)
                            * count.saturating_sub(1).max(1) as f32)
                            as usize;
                        if count > 1 && variant >= self.last_idle_variant {
                            variant += 1;
                        }
                        self.last_idle_variant = variant % count;
                        self.idle_event.trigger(
                            pack,
                            &entry,
                            "clip",
                            false,
                            false,
                            context,
                            self.last_idle_variant,
                            true,
                        );
                        self.signals.event("idle_event");
                    }
                    self.idle_cooldown = motion::interval(&entry, &mut self.seed, [20., 45.]);
                }
            }
        } else {
            self.idle_event.stop(0.12);
            self.idle_cooldown = 0.;
        }
        self.idle_event.tick(true);
    }
    fn tick_camera(&mut self, pack: &Pack, i: &Input) {
        let base = &self.base.entry;
        if !base["camera"].is_null() {
            self.camera = Some(base["camera"].clone());
        }
        if self.state.ordinary() && i.fence {
            let entry = pack.entry("GENERAL", "fence");
            if !entry["camera"].is_null() {
                self.camera = Some(entry["camera"].clone());
            }
        }
        if self.map.active && !self.map_pose.entry["camera"].is_null() {
            self.camera = Some(self.map_pose.entry["camera"].clone());
        }
        let active_hand = i.using.map_or(0, |(h, _)| h);
        let definition = pack.definition(&i.items[active_hand]);
        if !definition["camera"].is_null() {
            self.camera = Some(definition["camera"].clone());
        }
        let group = crate::moves_pack::item_group(&i.items[active_hand]);
        if i.using.is_some() && matches!(group, "BOW" | "CROSSBOW" | "TRIDENT") {
            let camera = &pack.config["cameras"]["aiming"]["camera"];
            if !camera.is_null() {
                self.camera = Some(camera.clone());
            }
        }
        for channel in [&self.emote, &self.alignment] {
            if channel.playback.has_clip() && !channel.entry["camera"].is_null() {
                self.camera = Some(channel.entry["camera"].clone());
            }
        }
        let proximity = &pack.config["cameras"]["proximity"];
        let blend = self.proximity.update(
            i.block_distance,
            i.mining,
            i.sprinting,
            STEP,
            number(&proximity["instant_distance"], 1.),
            number(&proximity["enter_distance"], 2.2),
            number(&proximity["mining_hold_seconds"], 2.),
            number(&proximity["blend_in_response"], 5.),
            number(&proximity["blend_out_response"], 3.2),
        );
        if blend > 0.0001 && !proximity["camera"].is_null() && self.camera.is_none() {
            let mut camera = proximity["camera"].clone();
            if camera.is_object() {
                camera["_blend"] = serde_json::json!(blend);
                self.camera = Some(camera);
            }
        }
    }
    fn ground_layers(&self, pack: &Pack, i: &Input, context: &BTreeMap<String, f64>) -> Vec<Layer> {
        let crawl = self.state == State::Crawl;
        let settings = pack.entry("GENERAL", if crawl { "crawling" } else { "walking" });
        let mut out = Vec::new();
        let mut weights = if crawl {
            self.gait.crawl_directions
        } else {
            self.gait.directions
        };
        let total: f32 = weights.iter().sum();
        if total < 0.0001 && crawl {
            weights[0] = 1.;
        }
        let total = weights.iter().sum::<f32>().max(0.0001);
        let motion = if crawl { 1. } else { self.gait.motion };
        if !crawl {
            let idle = pack.entry("GENERAL", "idle");
            let mut layers = sequence::resolve(pack, &idle, "clip", false, false, true, context, 0);
            for layer in &mut layers {
                layer.group = 0;
                layer.weight *= 1. - motion;
                layer.time = self.ticks as f32 * STEP;
                layer.previous_time = layer.time - STEP;
            }
            out.extend(layers);
        }
        let sprint_overlay = settings["sprint_overlay"].is_string();
        if !crawl && sprint_overlay {
            weights[0] += (weights[1] + weights[2] + weights[3]) * self.gait.sprint;
            for weight in &mut weights[1..] {
                *weight *= 1. - self.gait.sprint;
            }
        }
        for (index, direction) in ["forward", "backward", "left", "right"].iter().enumerate() {
            let weight = weights[index] / total * motion;
            if weight < 0.00001 {
                continue;
            }
            let normal = settings["directions"][direction].clone();
            let sprint = settings["directions"]["sprint"][direction].clone();
            let mut entries = vec![(normal.clone(), weight)];
            if !crawl && index == 0 && !sprint.is_null() {
                entries = vec![
                    (normal, weight * (1. - self.gait.sprint)),
                    (sprint, weight * self.gait.sprint),
                ];
            }
            for (entry, weight) in entries {
                let candidates: Vec<&Value> = entry
                    .as_array()
                    .map(|a| a.iter().take(32).collect())
                    .unwrap_or_else(|| vec![&entry]);
                for candidate in candidates {
                    let mut local = context.clone();
                    local.insert("direction_weight".into(), weight as f64);
                    let mut layers =
                        sequence::resolve(pack, candidate, "clip", false, false, true, &local, 0);
                    for layer in &mut layers {
                        let reverse = candidate["reverse"] == true
                            || index == 1 && settings["reverse_backward"] == true;
                        let mut phase = if crawl {
                            self.gait.crawl_phase
                        } else {
                            self.gait.phase
                        };
                        if reverse && phase != 0. {
                            phase = 1. - phase;
                        }
                        layer.group = 0;
                        if crawl {
                            layer.root_pitch_origin =
                                layer.clip.sample_model_once("root", 0.).euler.x;
                        }
                        layer.weight *= weight;
                        layer.time = phase * layer.clip.length;
                        let delta = if crawl {
                            STEP * (i.speed * 10.).clamp(0.35, 1.45)
                        } else {
                            (self.gait.phase - self.gait.previous_phase).rem_euclid(1.)
                        };
                        layer.previous_time =
                            layer.time - delta * layer.clip.length * if reverse { -1. } else { 1. };
                        layer.amplitude = if crawl {
                            1.
                        } else {
                            self.gait.amplitude * self.gait.use_stride_scale
                        };
                        for overlay in &mut layer.overlays {
                            overlay.time = layer.time;
                            overlay.previous_time = layer.previous_time;
                        }
                    }
                    if let Some(name) = candidate["add"].as_str() {
                        let addition = serde_json::json!({"clip":name,"additive":true});
                        let mut additions = sequence::resolve(
                            pack, &addition, "clip", false, false, true, &local, 0,
                        );
                        for layer in &mut additions {
                            layer.group = 1;
                            layer.weight *= weight;
                            layer.amplitude = self.gait.use_stride_scale;
                            let reverse = candidate["reverse"] == true
                                || index == 1 && settings["reverse_backward"] == true;
                            let phase = if reverse && self.gait.phase != 0. {
                                1. - self.gait.phase
                            } else {
                                self.gait.phase
                            };
                            let delta = (self.gait.phase - self.gait.previous_phase).rem_euclid(1.);
                            layer.time = phase * layer.clip.length;
                            layer.previous_time = layer.time
                                - delta * layer.clip.length * if reverse { -1. } else { 1. };
                        }
                        out.extend(additions);
                    }
                    out.extend(layers);
                }
            }
        }
        if !crawl && sprint_overlay {
            let mut layers = sequence::resolve(
                pack,
                &settings,
                "sprint_overlay",
                false,
                false,
                true,
                context,
                0,
            );
            for layer in &mut layers {
                layer.group = 2;
                layer.additive = true;
                layer.weight *= self.gait.sprint * self.gait.motion;
                layer.amplitude = self.gait.use_stride_scale;
                layer.time = self.gait.phase * layer.clip.length;
                layer.previous_time = layer.time
                    - (self.gait.phase - self.gait.previous_phase).rem_euclid(1.)
                        * layer.clip.length;
            }
            out.extend(layers);
        }
        out
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit animation and world inputs keep the pure solver independent of the game"
    )]
    fn extra_base(
        &self,
        pack: &Pack,
        action: &str,
        field: &str,
        group: u16,
        context: &BTreeMap<String, f64>,
        additive: bool,
        weight: f32,
    ) -> Vec<Layer> {
        let entry = pack.entry("GENERAL", action);
        let mut layers = sequence::resolve(pack, &entry, field, false, false, true, context, 0);
        for layer in &mut layers {
            layer.group = group;
            layer.additive = additive;
            layer.weight *= weight;
            layer.time = self.base.playback.age;
            layer.previous_time = (layer.time - STEP * layer.rate).max(0.);
            for overlay in &mut layer.overlays {
                overlay.time = layer.time;
                overlay.previous_time = layer.previous_time;
            }
        }
        layers
    }
    fn tick_frame(
        &mut self,
        pack: &Pack,
        i: &Input,
        context: &BTreeMap<String, f64>,
        previous_state: State,
    ) {
        let mut layers = if self.state.ordinary() || self.state == State::Crawl {
            self.ground_layers(pack, i, context)
        } else {
            self.base.playback.emit(0)
        };
        if self.state == State::Falling && self.item_sequence.replaces_falling(i) {
            layers.clear();
        }
        if self.state == State::Falling && self.fall_class >= 3 {
            layers.extend(self.extra_base(pack, "falling", "effect", 3, context, true, 1.));
        }
        if self.state == State::WaterAir {
            layers.extend(self.extra_base(
                pack,
                "water_dive",
                self.fluid.tuck.key(true),
                3,
                context,
                true,
                motion::smooth(self.base.playback.age / self.base.playback.length.max(0.001)),
            ));
        }
        if matches!(self.state, State::LavaAir | State::LavaContact) {
            layers.extend(self.extra_base(
                pack,
                "lava_dive",
                if self.state == State::LavaAir {
                    "air_effect"
                } else {
                    "contact_effect"
                },
                3,
                context,
                true,
                1.,
            ));
        }
        if self.state == State::Swim {
            let settings = pack.entry("GENERAL", "swimming");
            motion::approach(
                &mut self.swim_pitch,
                -motion::swim_intent(i)[1]
                    * number(&settings["vertical_tilt_degrees"], 30.8).clamp(0., 90.),
                5.,
            );
        } else {
            motion::approach(&mut self.swim_pitch, 0., 5.);
        }
        if self.state == State::Elytra {
            let settings = pack.entry("GENERAL", "elytra");
            let threshold = number(&settings["high_speed_threshold"], 0.9).max(0.05);
            motion::approach(
                &mut self.elytra_high,
                motion::unit((i.speed - threshold * 0.8) / (threshold * 0.4)),
                4.5,
            );
            for layer in &mut layers {
                layer.weight *= 1. - self.elytra_high;
            }
            layers.extend(self.extra_base(
                pack,
                "elytra",
                "high_speed",
                0,
                context,
                false,
                self.elytra_high,
            ));
            let delta = if previous_state == State::Elytra {
                motion::wrap(i.yaw - self.previous.yaw)
            } else {
                0.
            };
            let bank = number(&settings["bank_degrees"], 30.).clamp(0., 90.);
            let target = (-delta * number(&settings["bank_sensitivity"], 5.)).clamp(-bank, bank);
            motion::approach(
                &mut self.elytra_bank,
                target,
                number(&settings["bank_response"], 5.),
            );
            layers.extend(self.extra_base(
                pack,
                "elytra",
                "rotate",
                3,
                context,
                true,
                motion::unit(self.elytra_bank.abs() / bank.max(0.001)),
            ));
            if i.items[0].ends_with(":firework_rocket") {
                let mut hold =
                    self.extra_base(pack, "elytra", "firework_hold", 30, context, false, 1.);
                for layer in &mut hold {
                    layer.mirror = i.main_left;
                    layer.restrict_to_hands();
                }
                layers.extend(hold);
            }
        } else {
            motion::approach(&mut self.elytra_bank, 0., 5.);
        }
        if matches!(self.state, State::Fly | State::Sit | State::Passenger) {
            layers.extend(self.extra_base(
                pack,
                if self.state == State::Fly {
                    "creative_fly"
                } else if self.state == State::Sit {
                    "sitting_state"
                } else {
                    "passenger_state"
                },
                "idle",
                3,
                context,
                true,
                1.,
            ));
        }
        if self.state.ordinary() {
            if i.fence {
                layers.extend(self.extra_base(pack, "fence", "pose", 4, context, false, 1.));
                layers.extend(self.extra_base(pack, "fence", "idle", 5, context, true, 1.));
            } else if i.on_water {
                layers.extend(self.extra_base(
                    pack,
                    "walk_on_water",
                    "pose",
                    4,
                    context,
                    false,
                    1.,
                ));
                layers.extend(self.extra_base(pack, "walk_on_water", "idle", 5, context, true, 1.));
            }
        }
        layers.extend(self.status.emit(6, i, 0., false));
        let mut biome = self.biome.emit(7, i, 1., false);
        if text(&self.biome.entry["pose_layer"], "full_body").replace('_', "") == "upperbody" {
            for layer in &mut biome {
                layer
                    .exclude
                    .extend(["root", "left_leg", "right_leg"].map(str::to_owned));
            }
        }
        layers.extend(biome);
        layers.extend(self.crouch.channel.playback.emit(10));
        layers.extend(self.jump.emit(11, i, 0., false));
        layers.extend(
            self.landing
                .emit(12, i, if self.gait.moving { 1. } else { 0. }, false),
        );
        layers.extend(self.sprint_event.emit(13, i, 1., false));
        self.turn.tick(
            pack,
            i,
            self.state.ordinary()
                && i.grounded
                && !i.fence
                && !i.on_water
                && !i.crouching
                && (!self.status.playback.active() || self.status.entry["additive"] == true),
            self.gait.moving || i.sprinting,
            i.attack
                || i.mining
                || i.external_attack
                || i.using.is_some()
                || self.action.playback.has_clip()
                || self.use_action.playback.has_clip()
                || self.hurt.playback.has_clip()
                || self.armor.playback.has_clip()
                || self.item_sequence.channel.playback.active()
                    && !matches!(
                        self.item_sequence.phase.as_str(),
                        "idle" | "idle_one_handed"
                    ),
            context,
        );
        layers.extend(self.turn.layers());
        layers.extend(self.music.emit(19, i, 0., false));
        layers.extend(self.idle_event.emit(20, i, 0., false));
        layers.extend(self.biome_event.emit(21, i, 0., false));
        layers.extend(self.cold.emit(22, i, 1., false));
        for hand in (0..2).rev() {
            layers.extend(self.held[hand].emit(30 + hand as u16, i, 0.65, true));
            layers.extend(self.equip[hand].emit(32 + hand as u16, i, 1., true));
        }
        layers.extend(self.map_pose.emit(35, i, 1., true));
        let using_hand = i.using.map_or(0, |(h, _)| h);
        for hand in [1 - using_hand, using_hand] {
            let mut channel =
                self.item_use[hand].emit(40 + (hand == using_hand) as u16, i, 0., true);
            if self.state.hands_only() || i.riding.is_some() {
                for layer in &mut channel {
                    layer.restrict_to_hands();
                }
            }
            layers.extend(channel);
        }
        layers.extend(self.item_sequence.emit(45, i, self.state));
        let mut action = self.action.emit(50, i, 1., true);
        if self.state.hands_only() || i.riding.is_some() {
            for layer in &mut action {
                layer.restrict_to_hands();
            }
        }
        layers.extend(action);
        let mut action = self.use_action.emit(51, i, 1., true);
        if self.state.hands_only() || i.riding.is_some() {
            for layer in &mut action {
                layer.restrict_to_hands();
            }
        }
        layers.extend(action);
        layers.extend(self.interaction.emit(52, i, 0., true));
        layers.extend(self.shield.emit(53, i, 0., true));
        layers.extend(self.firework.emit(54, i, 0., false));
        layers.extend(self.hurt.emit(60, i, 0., true));
        layers.extend(self.armor.emit(61, i, 0., true));
        layers.extend(self.emote.playback.emit(70));
        layers.extend(self.alignment.playback.emit(71));
        layers.retain(|layer| layer.render_weight(0.).max(layer.render_weight(1.)) > 0.00001);
        layers.truncate(128);
        self.capture_events(&layers);
        let previous_tilt = self.frame.tilt;
        self.lean.tick(
            i.body_yaw.unwrap_or(i.yaw),
            i.speed,
            i.movement[1],
            i.movement[0],
            self.state.ordinary() && i.grounded,
            !i.grounded && self.previous.grounded,
            !i.grounded && self.state == State::Air,
        );
        self.direction_lean
            .tick(i, self.state.ordinary() && i.grounded);
        let mut tilt = [
            self.direction_lean.pitch,
            self.lean.sample(1.) + self.direction_lean.roll,
        ];
        let mut vertical_offset = 0.;
        match self.state {
            State::Fly => {
                let [forward, left] = motion::flight_direction(i);
                motion::approach(
                    &mut self.fly_pitch,
                    forward
                        * if forward < 0. {
                            14.
                        } else if i.sprinting {
                            35.
                        } else {
                            18.
                        },
                    5.5,
                );
                motion::approach(&mut self.fly_roll, left * 16., 5.5);
                tilt = [self.fly_pitch, self.fly_roll];
            }
            State::Elytra => {
                tilt = [0., self.elytra_bank];
            }
            State::Swim => {
                tilt = [self.swim_pitch, 0.];
            }
            State::Passenger => {
                let entry = pack.entry("GENERAL", "passenger_state");
                let dynamics = &entry["dynamics"];
                self.passenger_tilt.tick(
                    i.vehicle_yaw,
                    i.vehicle_velocity,
                    number(&entry["speed_threshold"], 0.12),
                    number(&dynamics["bank_degrees"], 28.),
                    number(&dynamics["bank_sensitivity"], 1.5),
                    number(&dynamics["vertical_tilt_degrees"], 22.),
                    number(&dynamics["vertical_sensitivity"], 4.),
                    number(&dynamics["response"], 6.),
                );
                tilt = self.passenger_tilt.sample(1.);
                vertical_offset = -number(&dynamics["rider_vertical_offset"], 0.).clamp(-64., 64.);
            }
            _ => {}
        }
        self.frame = Frame {
            layers,
            tilt,
            previous_tilt,
            vertical_offset,
            body_yaw: self.turn.body_yaw(),
            head_aim: self.head_aim,
            crouch: motion::smooth(self.gait.crouch),
        };
    }
    fn capture_events(&mut self, layers: &[Layer]) {
        let mut seen = BTreeSet::new();
        for layer in layers
            .iter()
            .flat_map(|layer| std::iter::once(layer).chain(layer.overlays.iter()))
        {
            if layer.render_weight(1.) < 0.01
                || layer.time <= layer.previous_time
                || layer.clip.length <= 0.
            {
                continue;
            }
            let clip = &layer.clip;
            for (kind, entries) in [
                (EventKind::Timeline, &clip.timeline),
                (EventKind::Sound, &clip.sounds),
            ] {
                for (time, names) in entries.iter().take(256) {
                    let crossed = if layer.looping {
                        let length = clip.length;
                        ((layer.time - time) / length).floor()
                            > ((layer.previous_time - time) / length).floor()
                            || layer.previous_time == 0. && *time == 0.
                    } else {
                        *time > layer.previous_time && *time <= layer.time
                            || layer.previous_time == 0. && *time == 0.
                    };
                    if !crossed {
                        continue;
                    }
                    for name in names.iter().take(32) {
                        if seen.insert((clip.name.clone(), name.clone(), kind)) {
                            self.events.push(AnimationEvent {
                                clip: clip.name.clone(),
                                name: name.clone(),
                                kind,
                                hand: None,
                            });
                        }
                    }
                }
            }
            if layer.previous_time == 0. && layer.time > 0. {
                for name in clip.effects.iter().take(32) {
                    if seen.insert((clip.name.clone(), name.clone(), EventKind::Effect)) {
                        self.events.push(AnimationEvent {
                            clip: clip.name.clone(),
                            name: name.clone(),
                            kind: EventKind::Effect,
                            hand: None,
                        });
                    }
                }
            }
        }
        self.events.truncate(256);
    }
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    fn pack() -> Pack {
        let mut pack = Pack::default();
        pack.clips = crate::load_model_clips(br#"{"animations":{"idle":{"loop":true,"animation_length":1,"bones":{"body":{"rotation":[0,0,0]}}},"walk":{"loop":true,"animation_length":1,"bones":{"right_leg":{"rotation":[30,0,0]}}},"wave":{"animation_length":0.2,"bones":{"right_arm":{"rotation":[90,0,0]}}},"hurt":{"animation_length":0.3,"bones":{"body":{"rotation":[10,0,0]}}},"attack":{"animation_length":0.5,"bones":{"right_arm":{"rotation":[45,0,0]}}}}}"#)
            .unwrap().into_iter().map(|(n, clip)| (n, Arc::new(clip))).collect();
        pack.config = serde_json::json!({"defaults":{"animations":{"idle":{"clip":"idle"},"walking":{"directions":{"forward":{"clip":"walk"}}},"attack":{"clip":"attack"},"hurt_forward":{"clip":"hurt"}}}});
        pack
    }
    #[test]
    fn locomotion_and_emote_cancellation_are_data_driven() {
        let pack = pack();
        let mut controller = Controller::default();
        let mut i = Input {
            grounded: true,
            movement: [0., 1.],
            speed: 0.1,
            ..Default::default()
        };
        controller.tick(&pack, &i);
        assert!(
            controller
                .frame
                .layers
                .iter()
                .any(|l| l.clip.name == "walk")
        );
        assert!(!controller.play_emote(&pack, "wave"));
        i.movement = [0., 0.];
        i.speed = 0.;
        controller.tick(&pack, &i);
        assert!(controller.play_emote(&pack, "wave"));
        controller.tick(&pack, &i);
        assert!(controller.is_emote_playing());
        i.attack = true;
        controller.tick(&pack, &i);
        assert!(!controller.is_emote_playing());
    }
    #[test]
    fn independent_hurt_and_attack_do_not_replace_each_other() {
        let pack = pack();
        let mut c = Controller::default();
        let i = Input {
            grounded: true,
            attack: true,
            hurt: true,
            ..Default::default()
        };
        c.tick(&pack, &i);
        assert!(c.action.playback.has_clip());
        assert!(c.hurt.playback.has_clip());
        assert!(c.frame.layers.iter().any(|l| l.clip.name == "attack"));
        assert!(c.frame.layers.iter().any(|l| l.clip.name == "hurt"));
    }
    #[test]
    fn consumed_throwable_keeps_its_face_event_kind_after_the_stack_is_gone() {
        let pack = pack();
        let mut controller = Controller::default();
        let mut input = Input {
            grounded: true,
            used: Some(0),
            accepted_item: Some("minecraft:ender_pearl".into()),
            ..Default::default()
        };
        controller.tick(&pack, &input);
        assert_eq!(controller.signals.since("threw"), 0);
        assert_eq!(controller.signals.values["thrown"], 2.);
        input.used = None;
        input.accepted_item = None;
        controller.tick(&pack, &input);
        assert_eq!(controller.signals.since("threw"), 1);
        assert_eq!(controller.signals.values["thrown"], 2.);
    }
    #[test]
    fn cancelled_item_use_is_not_a_ranged_release_reaction() {
        let pack = pack();
        let mut controller = Controller::default();
        let mut input = Input {
            grounded: true,
            items: ["minecraft:trident".into(), String::new()],
            using: Some((0, Use::Other)),
            ..Default::default()
        };
        controller.tick(&pack, &input);
        input.using = None;
        input.items[0] = String::new();
        controller.tick(&pack, &input);
        assert_eq!(controller.signals.since("hurled"), u64::MAX);
        assert_eq!(controller.signals.since("released"), u64::MAX);
        input.items[0] = "minecraft:trident".into();
        input.using = Some((0, Use::Other));
        controller.tick(&pack, &input);
        input.using = None;
        input.use_released = true;
        controller.tick(&pack, &input);
        assert_eq!(controller.signals.since("hurled"), 0);
        assert_eq!(controller.signals.since("released"), u64::MAX);
    }
    #[test]
    fn mirrored_fallback_swaps_tracks_and_retention() {
        let pack = pack();
        let mut layer = Layer::new(pack.resolve("attack", false).unwrap());
        layer.mirror = true;
        layer.retain = serde_json::json!({"right_arm":0.25});
        assert!(layer.pose_at("left_arm", 1.).is_some());
        assert!(layer.pose_at("right_arm", 1.).is_none());
        assert_eq!(layer.retention("left_arm"), 0.25);
    }
    #[test]
    fn additive_scale_and_retained_base_follow_sequence_composition() {
        let pack = pack();
        let mut base = Layer::new(pack.resolve("attack", false).unwrap());
        base.group = 0;
        let mut action = Layer::new(pack.resolve("wave", false).unwrap());
        action.group = 1;
        action.retain = serde_json::json!({"right_arm":1.});
        let frame = Frame {
            layers: vec![base, action],
            ..Default::default()
        };
        let (pose, weight) = frame.bone("right_arm", 1.).unwrap();
        assert!((pose.euler.x - 135.).abs() < 0.01);
        assert_eq!(weight, 1.);
    }
    #[test]
    fn invalid_observations_never_escape_into_pose_matrices() {
        let pack = pack();
        let mut c = Controller::default();
        let i = Input {
            yaw: f32::NAN,
            pitch: f32::INFINITY,
            speed: f32::NAN,
            vertical_speed: f32::NEG_INFINITY,
            used: Some(99),
            using: Some((99, Use::Other)),
            velocity: Vec3::splat(f32::NAN),
            grounded: true,
            ..Default::default()
        };
        c.tick(&pack, &i);
        for bone in MODEL_BONES {
            assert!(c.frame.parent_matrix(bone, f32::NAN).is_finite());
        }
    }
    #[test]
    fn variables_keep_cycles_but_evaluate_acyclic_dependencies() {
        let mut pack = pack();
        pack.config["variables"] =
            serde_json::json!({"a":"b+1","b":"a+1","c":"horizontal_speed * 2","d":"c+1"});
        let mut c = Controller::default();
        let i = Input {
            grounded: true,
            speed: 0.2,
            ..Default::default()
        };
        c.tick(&pack, &i);
        assert_eq!(c.variables["a"], 0.);
        assert_eq!(c.variables["b"], 0.);
        assert!((c.variables["d"] - 1.4).abs() < 0.0001);
    }
    #[test]
    fn swimming_freezes_without_intent_and_changes_clips_without_resetting_phase() {
        let mut p = pack();
        p.config["defaults"]["animations"]["swimming"] = serde_json::json!({
            "surface":"idle","underwater":"walk","vertical":"idle"
        });
        let mut c = Controller::default();
        let mut i = Input {
            swimming: true,
            in_water: true,
            ..Default::default()
        };
        for _ in 0..10 {
            c.tick(&p, &i);
        }
        assert_eq!(c.state, State::Swim);
        assert_eq!(c.base.playback.current[0].time, 0.);
        i.speed = 0.1;
        i.movement = [0., 1.];
        for _ in 0..10 {
            c.tick(&p, &i);
        }
        let phase = c.base.playback.current[0].time;
        i.underwater = true;
        c.tick(&p, &i);
        assert_eq!(c.base.key, "swimming:underwater");
        assert!(c.base.playback.current[0].time > phase);
        assert!(c.base.playback.current[0].time < phase + STEP);
        i.swim_vertical = -1.;
        c.tick(&p, &i);
        assert_eq!(c.base.key, "swimming:vertical");
        i.swim_vertical = 0.;
        i.movement = [0., -1.];
        for _ in 0..10 {
            c.tick(&p, &i);
        }
        assert!(c.base.playback.current[0].rate < 0.);
    }
    #[test]
    fn creative_flight_uses_analog_diagonals_and_backward_tilt() {
        let p = pack();
        let mut c = Controller::default();
        let mut i = Input {
            flying: true,
            movement: [1., 1.],
            ..Default::default()
        };
        for _ in 0..30 {
            c.tick(&p, &i);
        }
        assert!(c.frame.tilt[0] > 12. && c.frame.tilt[0] < 13.);
        assert!(c.frame.tilt[1] > 11. && c.frame.tilt[1] < 12.);
        i.movement = [0., -1.];
        for _ in 0..30 {
            c.tick(&p, &i);
        }
        assert!((c.frame.tilt[0] + 14.).abs() < 0.1);
    }
}
