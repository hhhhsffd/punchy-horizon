//! Recovered TurnInPlaceController, CameraTurnInPlace and KeyframedTurnInPlace.
//! Thresholds, root-driven step progress, gesture collection, reversal and the
//! two-step correction are kept separate; they are not exponential yaw easing.
use super::{Input, Layer, motion, number};
use crate::{Clip, Pose, moves_pack::Pack};
use glam::Vec3;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, OnceLock},
};
const BONES: [&str; 6] = [
    "left_leg",
    "right_leg",
    "left_arm",
    "right_arm",
    "body",
    "head",
];
fn wrap(v: f32) -> f32 {
    let mut v = v % 360.;
    if v >= 180. {
        v -= 360.;
    }
    if v < -180. {
        v += 360.;
    }
    v
}
fn ease(v: f32) -> f32 {
    let v = v.clamp(0., 1.);
    v * v * (3. - 2. * v)
}
fn weighted(mut pose: Pose, w: f32) -> Pose {
    pose.position *= w;
    pose.euler *= w;
    pose.scale = Vec3::ONE + (pose.scale - Vec3::ONE) * w;
    super::with_rotation(pose, super::quaternion(pose.euler))
}
fn authored(clip: &Clip, addition: Option<&Clip>, bone: &str, time: f32) -> Pose {
    let mut base = clip.sample_model_once(bone, time);
    if let Some(addition) = addition {
        let mut pose = addition.sample_model_once(bone, time);
        if time > addition.length && clip.length > addition.length {
            pose = weighted(
                pose,
                ease((clip.length - time) / (clip.length - addition.length)),
            );
        }
        base.position += pose.position;
        base.euler += pose.euler;
        base.scale += pose.scale - Vec3::ONE;
        base = super::with_rotation(base, super::quaternion(base.euler));
    }
    base
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Steps,
    Scaled,
    Camera,
    Dynamic,
}
struct Settings {
    enabled: bool,
    mode: Mode,
    threshold: f32,
    delay: f32,
    fade: f32,
    reference: f32,
    double_turn: f32,
    template: Option<Arc<Clip>>,
    left: Option<Arc<Clip>>,
    right: Option<Arc<Clip>>,
    left_add: Option<Arc<Clip>>,
    right_add: Option<Arc<Clip>>,
}
impl Settings {
    fn read(pack: &Pack) -> Self {
        let e = pack.entry("GENERAL", "turn_in_place");
        let mode = match e["mode"].as_str() {
            Some("scaled") => Mode::Scaled,
            Some("camera") => Mode::Camera,
            Some("dynamic" | "procedural") => Mode::Dynamic,
            _ => Mode::Steps,
        };
        let clip =
            |field: &str, default: &str| pack.resolve(e[field].as_str().unwrap_or(default), false);
        let dynamic = mode == Mode::Dynamic;
        let template = clip("clip", "");
        let left = clip("left", if dynamic { "step_left" } else { "" });
        let right = clip("right", if dynamic { "step_right" } else { "" });
        let valid = |c: Option<&Arc<Clip>>| {
            c.is_some_and(|c| {
                c.length.is_finite()
                    && c.length > 0.
                    && c.tracks.contains_key("left_leg")
                    && c.tracks.contains_key("right_leg")
            })
        };
        let valid = if mode == Mode::Scaled {
            valid(template.as_ref())
        } else {
            valid(left.as_ref()) && valid(right.as_ref())
        };
        Self {
            enabled: valid
                && e["enabled"]
                    .as_bool()
                    .unwrap_or_else(|| number(&e["enabled"], 0.) > 0.5),
            mode,
            threshold: number(&e["minimum_turn_degrees"], if dynamic { 2. } else { 45. })
                .clamp(2., 90.),
            delay: if dynamic {
                number(&e["turn_delay_seconds"], 0.12).clamp(0., 0.3)
            } else {
                number(
                    &e["look_idle_seconds"],
                    if mode == Mode::Camera { 0. } else { 2. },
                )
                .clamp(0., 10.)
            },
            fade: number(&e["fade_out_seconds"], 0.12).clamp(0.05, 0.4),
            reference: number(&e["reference_degrees"], 45.).clamp(1., 180.),
            double_turn: number(&e["double_turn_degrees"], 75.).clamp(30., 180.),
            template,
            left,
            right,
            left_add: clip("left_add", e["left_overlay"].as_str().unwrap_or("")),
            right_add: clip("right_add", e["right_overlay"].as_str().unwrap_or("")),
        }
    }
    fn clips(&self, negative: bool) -> (Option<Arc<Clip>>, Option<Arc<Clip>>) {
        if negative {
            (self.left.clone(), self.left_add.clone())
        } else {
            (self.right.clone(), self.right_add.clone())
        }
    }
}
#[derive(Clone, Debug, Default)]
struct CameraTurn {
    state: u8,
    previous_look: f32,
    target: f32,
    body: f32,
    start: f32,
    time: f32,
    strength: f32,
    retarget_time: f32,
    last_motion: f32,
    retargeting: bool,
    clip: Option<Arc<Clip>>,
    addition: Option<Arc<Clip>>,
}
impl CameraTurn {
    fn reset(&mut self, yaw: f32, look: f32, now: f32) {
        *self = Self {
            previous_look: look,
            target: yaw + wrap(look - yaw),
            body: yaw,
            start: yaw,
            last_motion: now,
            ..Self::default()
        };
    }
    fn tracking(&mut self, interpolate: bool) {
        self.state = 1;
        self.start = self.body;
        self.target = self.body + wrap(self.target - self.body);
        self.time = 0.;
        self.strength = 0.;
        self.retarget_time = 0.;
        self.retargeting = interpolate;
    }
    fn update(&mut self, look: f32, now: f32, dt: f32, s: &Settings) -> f32 {
        let delta = wrap(look - self.previous_look);
        self.previous_look = look;
        self.target += delta;
        let moved = delta.abs() > 1e-6;
        if moved {
            self.last_motion = now;
        }
        if self.state == 2 {
            self.time = (self.time + dt).min(self.clip.as_ref().unwrap().length);
            if self.time < self.clip.as_ref().unwrap().length {
                return self.body;
            }
            self.clip = None;
            self.strength = 0.;
            self.state = 0;
            if wrap(self.target - self.body).abs() > 0.001 {
                self.tracking(true);
            }
            return self.body;
        }
        if self.state == 0 {
            if !moved {
                return self.body;
            }
            self.tracking(false);
        }
        if self.retargeting {
            self.retarget_time = (self.retarget_time + dt).min(0.08);
            self.body =
                self.start + wrap(self.target - self.start) * ease(self.retarget_time / 0.08);
            self.retargeting = self.retarget_time < 0.08;
        } else {
            self.body = self.target;
        }
        let displacement = self.body - self.start;
        self.strength = (displacement.abs() / s.reference).clamp(0., 1.);
        (self.clip, self.addition) = s.clips(displacement < 0.);
        self.time = 0.;
        if !self.retargeting && !moved && now - self.last_motion >= 0.0499999 {
            if self.strength > 0. {
                self.state = 2;
            } else {
                self.state = 0;
                self.clip = None;
            }
        }
        self.body
    }
    fn sample(&self, bone: &str) -> Pose {
        self.clip.as_ref().map_or_else(Pose::default, |clip| {
            weighted(
                authored(clip, self.addition.as_deref(), bone, self.time),
                self.strength,
            )
        })
    }
}
#[derive(Clone, Debug, Default)]
struct DynamicTurn {
    from: BTreeMap<String, Pose>,
    now: f32,
    gesture_time: f32,
    last_motion: f32,
    reverse: f32,
    restart_since: f32,
    large_since: Option<f32>,
    body: f32,
    previous_look: f32,
    look_target: f32,
    origin: f32,
    direction: f32,
    start: f32,
    target: f32,
    time: f32,
    strength: f32,
    rate: f32,
    body_duration: f32,
    entry_duration: f32,
    turns: u8,
    gesture: bool,
    stopped: bool,
    two_turns: bool,
    restart_pending: bool,
    clip: Option<Arc<Clip>>,
    addition: Option<Arc<Clip>>,
}
impl DynamicTurn {
    fn reset(&mut self, yaw: f32, look: f32) {
        *self = Self {
            body: yaw,
            origin: yaw,
            start: yaw,
            target: yaw,
            previous_look: look,
            look_target: yaw + wrap(look - yaw),
            rate: 1.,
            body_duration: 0.36,
            entry_duration: 0.1,
            last_motion: f32::NEG_INFINITY,
            ..Self::default()
        };
    }
    fn gesture(&mut self, direction: f32, began: f32) {
        self.gesture = true;
        self.stopped = false;
        self.two_turns = false;
        self.gesture_time = began;
        self.origin = self.body;
        self.direction = direction;
        self.reverse = 0.;
        self.large_since = None;
        self.turns = 0;
        self.restart_pending = false;
    }
    fn strength(degrees: f32, reference: f32) -> f32 {
        (f64::from(degrees) / f64::from(reference)).clamp(0., 1.33333333333) as f32
    }
    fn begin(&mut self, target: f32, correction: bool, s: &Settings) {
        let snapshot = BONES
            .into_iter()
            .map(|b| (b.to_owned(), self.sample(b)))
            .collect();
        self.start = self.body;
        self.target = target;
        self.strength = Self::strength((target - self.start).abs(), s.reference);
        self.rate = if correction { 1.4 } else { 1. };
        self.body_duration = if correction { 0.3 } else { 0.36 };
        self.entry_duration = if correction { 0.06 } else { 0.1 };
        (self.clip, self.addition) = s.clips(target < self.body);
        self.time = 0.;
        self.from = snapshot;
    }
    fn update(&mut self, look: f32, dt: f32, action: bool, s: &Settings) -> f32 {
        self.now += dt;
        let input = wrap(look - self.previous_look);
        self.previous_look = look;
        self.look_target += input;
        if input.abs() > 1e-6 {
            let completed =
                self.clip.is_none() && (self.turns == 2 || self.turns == 1 && !self.two_turns);
            if !self.gesture || completed || self.clip.is_none() && self.turns == 0 && self.stopped
            {
                self.gesture(input.signum(), self.now);
            }
            if self.gesture && input * self.direction < 0. {
                self.reverse += input;
            } else {
                self.reverse = 0.;
            }
            if self.reverse.abs() >= 3.
                || self.clip.is_some() && (self.turns == 2 || self.turns == 1 && !self.two_turns)
            {
                if self.clip.is_none() {
                    self.gesture(input.signum(), self.now);
                } else if !self.restart_pending {
                    self.restart_pending = true;
                    self.restart_since = self.now;
                }
            }
            self.last_motion = self.now;
        } else if action && !self.gesture && (self.look_target - self.body).abs() > 1. {
            self.gesture((self.look_target - self.body).signum(), self.now);
            self.last_motion = self.now - 0.12;
        }
        self.stopped = self.gesture && self.now - self.last_motion >= 0.119999;
        if let Some(clip) = &self.clip {
            let mut forward = (self.look_target - self.start) * (self.target - self.start).signum();
            if self.turns == 1 && self.two_turns {
                forward *= 0.48;
            }
            let wanted = Self::strength((self.target - self.start).abs().max(forward), s.reference);
            self.strength += (wanted - self.strength).max(0.) * (-(-dt / 0.04).exp_m1());
            self.time = (self.time + dt * self.rate).min(clip.length);
            self.body = self.start
                + (self.target - self.start)
                    * ease(self.time / self.body_duration.min(clip.length));
            if self.time >= self.entry_duration {
                self.from.clear();
            }
            if self.time >= clip.length - 1e-4 {
                self.body = self.target;
                self.clip = None;
                self.addition = None;
                self.strength = 0.;
                self.from.clear();
                if self.restart_pending {
                    self.gesture((self.look_target - self.body).signum(), self.restart_since);
                    self.stopped = self.now - self.last_motion >= 0.119999;
                }
            }
        }
        let mut total = self.look_target
            - if self.turns == 0 {
                self.body
            } else {
                self.origin
            };
        let large = total.abs() >= s.double_turn - 0.001;
        if !self.gesture || self.turns != 0 || !large {
            self.large_since = None;
        } else if self.large_since.is_none() {
            self.large_since = Some(self.now);
        }
        let captured = self.stopped
            || large
                && (f64::from(total.abs() * 0.48) >= f64::from(s.reference) * 1.33333333333
                    || self.large_since.is_some_and(|at| self.now - at >= 0.299999));
        if self.gesture
            && self.clip.is_none()
            && self.turns == 0
            && (action || self.now - self.gesture_time >= s.delay - 1e-6)
            && captured
            && total.abs() >= if action { 1. } else { s.threshold }
        {
            self.origin = self.body;
            total = self.look_target - self.origin;
            self.two_turns = total.abs() >= s.double_turn - 0.001;
            let amount = if self.two_turns { total * 0.48 } else { total };
            self.begin(self.body + amount, false, s);
            self.turns = 1;
        } else if self.gesture
            && self.turns == 1
            && self.two_turns
            && self.clip.is_none()
            && self.stopped
        {
            if (self.look_target - self.body).abs() >= if action { 1. } else { s.threshold } {
                self.begin(self.look_target, true, s);
                self.turns = 2;
            } else {
                self.two_turns = false;
            }
        }
        self.body
    }
    fn sample(&self, bone: &str) -> Pose {
        let Some(clip) = &self.clip else {
            return Pose::default();
        };
        let pose = weighted(
            authored(clip, self.addition.as_deref(), bone, self.time),
            self.strength,
        );
        if self.time >= self.entry_duration {
            pose
        } else {
            let from = self.from.get(bone).copied().unwrap_or_default();
            let mixed = from.blend(pose, ease(self.time / self.entry_duration));
            super::with_rotation(mixed, mixed.rotation)
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Turning {
    initialized: bool,
    managed: bool,
    releasing: bool,
    now: f32,
    last_look: f32,
    quiet_yaw: f32,
    quiet_pitch: f32,
    body: f32,
    start: f32,
    target: f32,
    progress: f32,
    weight: f32,
    time: f32,
    release_time: f32,
    release_yaw: f32,
    strength: f32,
    clip: Option<Arc<Clip>>,
    addition: Option<Arc<Clip>>,
    mirrored: bool,
    mode: Option<Mode>,
    camera: CameraTurn,
    dynamic: DynamicTurn,
    previous: BTreeMap<String, Pose>,
}
impl Turning {
    pub fn body_yaw(&self) -> Option<f32> {
        self.managed.then_some(self.body)
    }
    fn reset_continuous(&mut self, look: f32) {
        self.camera.reset(self.body, look, self.now);
        self.dynamic.reset(self.body, look);
    }
    fn reset(&mut self, yaw: f32, look: f32, pitch: f32) {
        self.initialized = true;
        self.body = yaw;
        self.quiet_yaw = look;
        self.quiet_pitch = pitch;
        self.last_look = self.now;
        self.managed = false;
        self.releasing = false;
        self.weight = 0.;
        self.time = 0.;
        self.progress = 0.;
        self.clip = None;
        self.addition = None;
        self.mirrored = false;
        self.mode = None;
        self.reset_continuous(look);
    }
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        pack: &Pack,
        i: &Input,
        compatible: bool,
        moving: bool,
        action: bool,
        _context: &BTreeMap<String, f64>,
    ) {
        self.previous = BONES
            .into_iter()
            .map(|b| (b.to_owned(), self.sample(b)))
            .collect();
        self.now += motion::STEP;
        let native = i.body_yaw.unwrap_or(i.yaw);
        let settings = Settings::read(pack);
        if !self.initialized {
            self.reset(native, i.yaw, i.pitch);
            return;
        }
        if !compatible || !settings.enabled {
            self.reset(native, i.yaw, i.pitch);
            return;
        }
        let dt = motion::STEP;
        if wrap(i.yaw - self.quiet_yaw).abs() > 0.15 || (i.pitch - self.quiet_pitch).abs() > 0.15 {
            self.quiet_yaw = i.yaw;
            self.quiet_pitch = i.pitch;
            self.last_look = self.now;
        }
        if moving {
            self.mode = None;
            self.clip = None;
            self.weight = 0.;
            if !self.managed {
                self.body = native;
                self.reset_continuous(i.yaw);
                return;
            }
            if !self.releasing {
                self.releasing = true;
                self.release_yaw = self.body;
                self.release_time = 0.;
            }
            self.release_time += dt;
            let blend = ease(self.release_time / settings.fade);
            self.body = self.release_yaw + wrap(native - self.release_yaw) * blend;
            if blend >= 1. {
                self.managed = false;
                self.releasing = false;
                self.body = native;
            }
            self.reset_continuous(i.yaw);
            self.last_look = self.now;
            return;
        }
        self.managed = true;
        self.releasing = false;
        self.mode = Some(settings.mode);
        if settings.mode == Mode::Dynamic {
            self.body = self.dynamic.update(i.yaw, dt, action, &settings);
            return;
        }
        if settings.mode == Mode::Camera {
            self.body = self.camera.update(i.yaw, self.now, dt, &settings);
            return;
        }
        if let Some(clip) = &self.clip {
            self.time = (self.time + dt).min(clip.length);
            let first = clip.sample_model_once("root", 0.).euler.y;
            let last = clip.sample_model_once("root", clip.length).euler.y;
            let progress = if (last - first).abs() < 0.001 {
                ease(self.time / clip.length)
            } else {
                ((clip.sample_model_once("root", self.time).euler.y - first) / (last - first))
                    .clamp(0., 1.)
            };
            self.progress = self.progress.max(progress);
            self.body = self.start + wrap(self.target - self.start) * self.progress;
            self.weight = self.strength
                * ease(self.time / 0.08)
                * ease((clip.length - self.time) / settings.fade.min(clip.length * 0.2));
            if self.time >= clip.length {
                self.body = self.target;
                self.clip = None;
                self.weight = 0.;
            }
            return;
        }
        let offset = wrap(i.yaw - self.body);
        if offset.abs() > 1.
            && (action
                || offset.abs() >= settings.threshold
                || self.now - self.last_look >= settings.delay)
        {
            self.start = self.body;
            self.target = self.body + offset;
            self.mirrored = settings.mode == Mode::Scaled && offset < 0.;
            (self.clip, self.addition) = if settings.mode == Mode::Scaled {
                (settings.template, None)
            } else {
                settings.clips(offset < 0.)
            };
            self.strength = (offset.abs() / settings.reference).clamp(0., 1.);
            self.progress = 0.;
            self.time = 0.;
            self.weight = 0.;
        }
    }
    fn sample(&self, bone: &str) -> Pose {
        match self.mode {
            Some(Mode::Dynamic) => return self.dynamic.sample(bone),
            Some(Mode::Camera) => return self.camera.sample(bone),
            _ => {}
        }
        let Some(clip) = &self.clip else {
            return Pose::default();
        };
        let source = if self.mirrored {
            super::opposite_bone(bone)
        } else {
            bone.to_owned()
        };
        let pose = authored(clip, self.addition.as_deref(), &source, self.time);
        weighted(
            if self.mirrored {
                super::mirrored(pose)
            } else {
                pose
            },
            self.weight,
        )
    }
    pub fn layers(&self) -> Vec<Layer> {
        let active = match self.mode {
            Some(Mode::Dynamic) => self.dynamic.clip.is_some(),
            Some(Mode::Camera) => self.camera.clip.is_some() && self.camera.strength > 0.,
            _ => self.clip.is_some() && self.weight > 0.,
        };
        if !active {
            return vec![];
        }
        static CLIP: OnceLock<Arc<Clip>> = OnceLock::new();
        let clip = CLIP
            .get_or_init(|| {
                Arc::new(Clip {
                    name: "stationary_turn".into(),
                    length: 1.,
                    looped: false,
                    hold_on_last_frame: false,
                    length_defined: true,
                    loop_defined: true,
                    tracks: BTreeMap::new(),
                    effects: BTreeSet::new(),
                    timeline: vec![],
                    sounds: vec![],
                })
            })
            .clone();
        let mut layer = Layer::new(clip);
        layer.group = 18;
        layer.additive = true;
        layer.rate = 0.;
        layer.previous_fixed = self.previous.clone();
        layer.fixed = BONES
            .into_iter()
            .map(|bone| (bone.to_owned(), self.sample(bone)))
            .collect();
        vec![layer]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_turn_moves_body_and_releases_when_walking() {
        let mut pack = Pack {
            config: serde_json::json!({"defaults":{"animations":{"turn_in_place":{
                "enabled":true,"left":"step","right":"step","minimum_turn_degrees":45
            }}}}),
            ..Default::default()
        };
        pack.clips = crate::load_model_clips(br#"{"animations":{"step":{"animation_length":0.3,"bones":{"left_leg":{"rotation":[0,20,0]},"right_leg":{"rotation":[0,-20,0]}}}}}"#)
            .unwrap().into_iter().map(|(n,c)|(n,std::sync::Arc::new(c))).collect();
        let mut turn = Turning::default();
        let mut input = Input {
            yaw: 90.,
            body_yaw: Some(0.),
            grounded: true,
            ..Default::default()
        };
        for _ in 0..8 {
            turn.tick(&pack, &input, true, false, false, &BTreeMap::new());
        }
        assert!((turn.body_yaw().unwrap() - 90.).abs() < 0.01);
        input.body_yaw = Some(20.);
        for _ in 0..20 {
            turn.tick(&pack, &input, true, true, false, &BTreeMap::new());
        }
        assert!(turn.body_yaw().is_none());
        assert!(turn.layers().is_empty());
        turn.tick(&pack, &input, false, false, false, &BTreeMap::new());
        assert!(turn.body_yaw().is_none());
    }
}
