//! Native model-space renderer mathematics. Model poses are pixels, y down;
//! attachment matrices and hull geometry are blocks. No renderer is replaced.
use glam::{DVec3, EulerRot, Mat4, Quat, Vec2, Vec3};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

pub type Parts = [[f32; 9]; 6];

pub fn rotation(p: &[f32; 9]) -> Quat {
    Quat::from_euler(EulerRot::ZYX, p[5], p[4], p[3])
}
pub fn matrix(p: &[f32; 9]) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::new(p[6], p[7], p[8]),
        rotation(p),
        Vec3::new(p[0], p[1], p[2]),
    )
}
pub fn write_matrix(p: &mut [f32; 9], m: Mat4) -> bool {
    let (s, q, t) = m.to_scale_rotation_translation();
    if !s.is_finite() || !q.is_finite() || !t.is_finite() {
        return false;
    }
    let (z, y, x) = q.normalize().to_euler(EulerRot::ZYX);
    *p = [t.x, t.y, t.z, x, y, z, s.x, s.y, s.z];
    true
}
pub fn blend(from: &[f32; 9], to: &[f32; 9], weight: f32) -> [f32; 9] {
    let w = weight.clamp(0., 1.);
    let mut out = *from;
    let q = rotation(from).slerp(rotation(to), w);
    let (z, y, x) = q.to_euler(EulerRot::ZYX);
    for i in [0, 1, 2, 6, 7, 8] {
        out[i] = from[i] + (to[i] - from[i]) * w;
    }
    out[3..6].copy_from_slice(&[x, y, z]);
    out
}

/// The item attachment has different axes from the humanoid model. The
/// animation's raw Euler channel is intentionally used, not its body quaternion.
pub fn item_matrix(position: Vec3, euler_degrees: Vec3, scale: Vec3) -> Mat4 {
    let r = euler_degrees * (PI / 180.);
    Mat4::from_scale_rotation_translation(
        scale.max(Vec3::splat(0.001)),
        Quat::from_euler(EulerRot::ZYX, -r.y, -r.z, -r.x),
        Vec3::new(-position.x, -position.z, position.y) / 16.,
    )
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (10. + t * (-15. + 6. * t))
}
fn near(angle: f32, reference: f32) -> f32 {
    reference + (angle - reference + PI).rem_euclid(TAU) - PI
}

#[derive(Clone, Debug)]
struct Paddle {
    side: usize,
    direction: f32,
    phase: f32,
    offset: Vec2,
    velocity: Vec2,
    old_time: f32,
    time: f32,
}
impl Paddle {
    fn new(side: usize, phase: f32, direction: f32) -> Self {
        Self {
            side,
            phase,
            direction,
            offset: Vec2::ZERO,
            velocity: Vec2::ZERO,
            old_time: 0.3,
            time: 0.3,
        }
    }
    fn target(&self, partial: f32) -> Vec2 {
        paddle_pose(
            self.side,
            if self.direction == 0. {
                0.
            } else {
                self.phase - self.direction * PI / 8. * (1. - partial)
            },
        )
    }
    fn speed(&self, partial: f32) -> Vec2 {
        let phase = self.phase - self.direction * PI / 8. * (1. - partial);
        let speed = self.direction * PI / 8. / 0.05;
        Vec2::new(
            -PI / 8. * (-phase).cos(),
            -PI / 4. * (-phase + 1.).cos() * if self.side == 1 { -1. } else { 1. },
        ) * speed
    }
    fn duration(&self) -> f32 {
        if self.direction == 0. { 0.3 } else { 0.15 }
    }
    fn progress(&self, partial: f32) -> f32 {
        ((self.old_time + (self.time - self.old_time) * partial) / self.duration()).clamp(0., 1.)
    }
    fn sample(&self, partial: f32) -> Vec2 {
        let t = self.progress(partial);
        self.target(partial)
            + self.offset * (1. - smooth(t))
            + self.velocity * self.duration() * (t - t.powi(3) * (6. + t * (-8. + 3. * t)))
    }
    fn sample_speed(&self) -> Vec2 {
        let t = self.progress(1.);
        self.speed(1.)
            + self.offset * (-30. * t * t * (1. - t).powi(2) / self.duration())
            + self.velocity * (1. - t * t * (18. + t * (-32. + 15. * t)))
    }
    fn tick(&mut self, direction: f32, phase: f32) {
        if direction != self.direction {
            let current = self.sample(1.);
            let speed = self.sample_speed();
            self.direction = direction;
            self.phase = phase;
            self.offset = current - self.target(0.);
            self.velocity = speed - self.speed(0.);
            self.old_time = 0.;
            self.time = 0.05;
        } else {
            self.old_time = self.time;
            self.time = (self.time + 0.05).min(0.3);
        }
        self.phase = phase;
    }
}
pub fn paddle_pose(side: usize, phase: f32) -> Vec2 {
    let pitch = -PI / 3. + ((-phase).sin() + 1.) * PI / 8.;
    let yaw = (-phase + 1.).sin() * PI / 4.;
    Vec2::new(pitch, if side == 1 { PI - yaw } else { yaw })
}

#[derive(Clone, Debug, Default)]
struct Response {
    time: Option<f64>,
    value: f32,
    velocity: f32,
}
impl Response {
    fn update(&mut self, target: f32, time: f64, rate: f32) -> f32 {
        if !target.is_finite() || !time.is_finite() {
            return self.value;
        }
        let dt = self.time.map_or(f64::INFINITY, |last| time - last);
        if !(0. ..=0.5).contains(&dt) {
            self.value = target;
            self.velocity = 0.;
        } else if dt > 0. {
            let dt = dt as f32;
            let delta = self.value - target;
            let advance = (self.velocity + rate * delta) * dt;
            let decay = (-rate * dt).exp();
            self.value = target + (delta + advance) * decay;
            self.velocity = (self.velocity - rate * advance) * decay;
        }
        self.time = Some(time);
        self.value
    }
}

/// Kept per boat/controlling rider. Resets on discontinuous clocks, not once per
/// render pass (armor and item sampling must see exactly the same solution).
#[derive(Clone, Debug)]
pub struct Rowing {
    tick: i32,
    paddles: [Paddle; 2],
    phases: [f32; 2],
    weight: [f32; 2],
    torso: Response,
    corrections: [Response; 4],
}
impl Rowing {
    pub fn new(tick: i32, phase: [f32; 2], direction: [f32; 2]) -> Self {
        Self {
            tick,
            paddles: [
                Paddle::new(0, phase[0], direction[0]),
                Paddle::new(1, phase[1], direction[1]),
            ],
            phases: phase,
            weight: [0.; 2],
            torso: Response::default(),
            corrections: std::array::from_fn(|_| Response::default()),
        }
    }
    pub fn tick(&mut self, tick: i32, directions: [f32; 2], phases: [f32; 2]) {
        let elapsed = tick.saturating_sub(self.tick);
        if tick < self.tick || elapsed > 40 {
            *self = Self::new(tick, phases, directions);
            return;
        }
        for _ in 0..elapsed {
            self.weight[0] = self.weight[1];
            self.weight[1] = (self.weight[1] + 0.2).min(1.);
            for (side, direction) in directions.iter().copied().enumerate() {
                self.phases[side] = (self.phases[side] + direction * PI / 8.).rem_euclid(TAU);
                self.paddles[side].tick(direction, self.phases[side]);
            }
        }
        self.tick = tick;
    }
    pub fn weight(&self, partial: f32) -> f32 {
        smooth(self.weight[0] + (self.weight[1] - self.weight[0]) * partial.clamp(0., 1.))
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit animation and world inputs keep the pure solver independent of the game"
    )]
    pub fn solve(
        &mut self,
        body: &Parts,
        hull_in_model: Mat4,
        raft: bool,
        chest: bool,
        slim: bool,
        partial: f32,
        action: [f32; 2],
    ) -> RowFrame {
        let weight = self.weight(partial);
        let mut paddles = self
            .paddles
            .each_ref()
            .map(|p| p.sample(partial.clamp(0., 1.)));
        if chest {
            paddles = [
                Vec2::new(paddles[1].x, PI - paddles[1].y),
                Vec2::new(paddles[0].x, PI - paddles[0].y),
            ];
        }
        let sides = if chest { [1, 0] } else { [0, 1] };
        let hand_paddles = if chest {
            [paddles[1], paddles[0]]
        } else {
            paddles
        };
        let mut grips = std::array::from_fn(|hand| {
            paddle_grip(hull_in_model, raft, sides[hand], hand_paddles[hand])
        });
        let hips = rotation(&body[1]) * Vec3::new(0., 12. * body[1][7], 0.)
            + Vec3::from_slice(&body[1][0..3]);
        let shoulders = [
            shoulder(&body[3], true, slim),
            shoulder(&body[2], false, slim),
        ];
        let mut best = (f32::INFINITY, 0_f32, 0_f32);
        // Coarse-to-fine constrained fitting keeps chest-boat hands on the
        // fixed oar handles; ordinary boats can also adjust the oars.
        let mut centre = Vec2::ZERO;
        for step in [5_f32, 1., 0.2] {
            let radius = if step == 5. { 8 } else { 5 };
            for p in -radius..=radius {
                for y in -radius..=radius {
                    if !chest && y != 0 {
                        continue;
                    }
                    let pitch = (centre.x + p as f32 * step).clamp(-45., 35.).to_radians();
                    let yaw = (centre.y + y as f32 * step).clamp(-40., 40.).to_radians();
                    let turn = Quat::from_rotation_x(pitch) * Quat::from_rotation_y(yaw);
                    let mut errors = [0.; 2];
                    for hand in 0..2 {
                        let d =
                            (hips + turn * (shoulders[hand] - hips)).distance(grips[hand] * 16.);
                        let e = if chest {
                            d - d.clamp(11.04, 12.6)
                        } else {
                            (d - 12.) / 12.
                        };
                        errors[hand] = e * e * (1. - action[hand].clamp(0., 1.));
                    }
                    let cost = if chest {
                        1e6 * (errors[0] + errors[1]) + pitch * pitch + 1.5 * yaw * yaw
                    } else {
                        errors[0].max(errors[1])
                            + 0.25 * (errors[0] + errors[1])
                            + 0.002 * pitch * pitch
                    };
                    if cost < best.0 {
                        best = (cost, pitch, yaw);
                    }
                }
            }
            centre = Vec2::new(best.1.to_degrees(), best.2.to_degrees());
        }
        let time = (self.tick as f64 + f64::from(partial)) / 20.;
        let pitch = self
            .torso
            .update(best.1, time, 16.)
            .clamp(-FRAC_PI_4, 35_f32.to_radians());
        let yaw = if chest { best.2 } else { 0. };
        if !chest {
            let turn = Quat::from_rotation_x(pitch);
            for hand in 0..2 {
                let side = sides[hand];
                let shoulder = (hips + turn * (shoulders[hand] - hips)) / 16.;
                let base = hand_paddles[hand];
                let exact = reachable_paddle(hull_in_model, raft, side, base, shoulder);
                let delta = exact - base;
                let limit = (1.2 / delta.length().max(1e-6)).min(1.);
                let delta = delta * limit * weight * (1. - action[hand].clamp(0., 1.));
                let correction = Vec2::new(
                    self.corrections[side * 2].update(delta.x, time, 20.),
                    self.corrections[side * 2 + 1].update(delta.y, time, 20.),
                );
                let mut adjusted = base + correction;
                // A smoothed oar may temporarily leave the reachable annulus.
                let distance = paddle_grip(hull_in_model, raft, side, adjusted).distance(shoulder);
                if !(0.69..=0.7875).contains(&distance) {
                    let mut lo = 0.;
                    let mut hi = 1.;
                    for _ in 0..12 {
                        let t = (lo + hi) * 0.5;
                        let d =
                            paddle_grip(hull_in_model, raft, side, adjusted.lerp(base + delta, t))
                                .distance(shoulder);
                        if (0.69..=0.7875).contains(&d) {
                            hi = t;
                        } else {
                            lo = t;
                        }
                    }
                    adjusted = adjusted.lerp(base + delta, hi);
                }
                paddles[side] = adjusted;
                grips[hand] = paddle_grip(hull_in_model, raft, side, adjusted);
            }
        }
        RowFrame {
            paddles,
            grips,
            pitch,
            yaw,
            weight,
            action,
        }
    }
}

pub fn hull_matrix(relative: Vec3, yaw: f32, hurt: f32, bubble: f32) -> Mat4 {
    Mat4::from_translation(relative + Vec3::new(0., 0.375, 0.))
        * Mat4::from_rotation_y((180. - yaw).to_radians())
        * Mat4::from_rotation_x(hurt.to_radians())
        * Mat4::from_axis_angle(Vec3::new(1., 0., 1.).normalize(), bubble.to_radians())
        * Mat4::from_scale(Vec3::new(-1., -1., 1.))
        * Mat4::from_rotation_y(FRAC_PI_2)
}
pub fn seat_offset(yaw: f32, chest: bool, passengers: usize) -> Vec3 {
    let distance = if chest {
        0.8
    } else if passengers > 1 {
        0.25
    } else {
        0.3125
    };
    Quat::from_rotation_y(-yaw.to_radians()) * Vec3::new(0., 0., -distance)
}
pub fn paddle_grip(hull: Mat4, raft: bool, side: usize, pose: Vec2) -> Vec3 {
    let pivot = Vec3::new(
        3.,
        if raft { -4. } else { -5. },
        if side == 0 { 9. } else { -9. },
    ) / 16.;
    (hull
        * Mat4::from_translation(pivot)
        * Mat4::from_quat(Quat::from_euler(EulerRot::ZYX, PI / 16., pose.y, pose.x)))
    .transform_point3(Vec3::new(0., 1., -5.) / 16.)
}
fn reachable_paddle(hull: Mat4, raft: bool, side: usize, base: Vec2, shoulder: Vec3) -> Vec2 {
    if hull.determinant().abs() < 1e-9 {
        return base;
    }
    let pivot_local = Vec3::new(
        3.,
        if raft { -4. } else { -5. },
        if side == 0 { 9. } else { -9. },
    ) / 16.;
    let pivot = hull.transform_point3(pivot_local);
    let original = paddle_grip(hull, raft, side, base);
    let radius = original.distance(pivot);
    let axis = shoulder - pivot;
    let distance = axis.length();
    if distance < 1e-6 || distance > radius + 0.75 || distance < (radius - 0.75).abs() {
        return base;
    }
    let axis = axis / distance;
    let along = (radius * radius - 0.75_f32.powi(2) + distance * distance) / (2. * distance);
    let radial = original - pivot - axis * (original - pivot).dot(axis);
    if radial.length_squared() < 1e-9 {
        return base;
    }
    let point = pivot
        + axis * along
        + radial.normalize() * (radius * radius - along * along).max(0.).sqrt();
    let q = Quat::from_rotation_z(-PI / 16.)
        * ((hull.inverse().transform_point3(point) - pivot_local) * 16.);
    let angle = (q.y / q.length().max(1e-6)).clamp(-1., 1.).acos();
    let offset = (-5_f32).atan2(1.);
    let a = near(angle - offset, base.x);
    let b = near(-angle - offset, base.x);
    let pitch = if (a - base.x).abs() <= (b - base.x).abs() {
        a
    } else {
        b
    };
    let out = pitch.sin() - 5. * pitch.cos();
    let yaw = if out.abs() < 1e-6 {
        base.y
    } else {
        near((q.x / out).atan2(q.z / out), base.y)
    };
    Vec2::new(pitch, yaw)
}

#[derive(Clone, Debug)]
pub struct RowFrame {
    pub paddles: [Vec2; 2],
    pub grips: [Vec3; 2],
    pub pitch: f32,
    pub yaw: f32,
    pub weight: f32,
    /// Left/right action ownership: a waving/using arm must not be seized by IK.
    pub action: [f32; 2],
}
fn shoulder(arm: &[f32; 9], left: bool, slim: bool) -> Vec3 {
    let palm = if slim { 0.5 } else { 1. } * if left { 1. } else { -1. };
    Vec3::from_slice(&arm[..3]) + rotation(arm) * Vec3::new(palm * arm[6], -2. * arm[7], 0.)
}
pub fn constrain_arm(arm: &mut [f32; 9], target_pixels: Vec3, left: bool, slim: bool) {
    let anchor = shoulder(arm, left, slim);
    let delta = target_pixels - anchor;
    if !delta.is_finite() || delta.length_squared() < 1e-4 {
        return;
    }
    let stretch = (delta.length() / 12.).clamp(0.92, 1.05);
    let q = Quat::from_rotation_arc(Vec3::Y, delta.normalize());
    let palm = if slim { 0.5 } else { 1. } * if left { 1. } else { -1. };
    let position = anchor - q * Vec3::new(palm * arm[6], -2. * stretch, 0.);
    let (z, y, x) = q.to_euler(EulerRot::ZYX);
    arm[..6].copy_from_slice(&[position.x, position.y, position.z, x, y, z]);
    arm[7] = stretch;
}
impl RowFrame {
    pub fn apply(&self, parts: &mut Parts, slim: bool) {
        let original = *parts;
        let hips = Vec3::from_slice(&parts[1][..3])
            + rotation(&parts[1]) * Vec3::new(0., 12. * parts[1][7], 0.);
        let turn = Quat::from_rotation_x(self.pitch) * Quat::from_rotation_y(self.yaw);
        for index in [0, 1, 2, 3] {
            let p = &mut parts[index];
            let position = hips + turn * (Vec3::from_slice(&p[..3]) - hips);
            let q = if index == 0 {
                rotation(p)
            } else {
                turn * rotation(p)
            };
            let (z, y, x) = q.to_euler(EulerRot::ZYX);
            p[..6].copy_from_slice(&[position.x, position.y, position.z, x, y, z]);
        }
        for (hand, index) in [(0, 3), (1, 2)] {
            constrain_arm(&mut parts[index], self.grips[hand] * 16., hand == 0, slim);
            parts[index] = blend(
                &original[index],
                &parts[index],
                self.weight * (1. - self.action[hand].clamp(0., 1.)),
            );
        }
        for index in [0, 1] {
            parts[index] = blend(&original[index], &parts[index], self.weight);
        }
    }
}

/// Fixed-step damped, inextensible hanging line. Coordinates remain doubles so
/// rod motion far from spawn does not acquire floating-point jitter.
#[derive(Clone, Debug, Default)]
pub struct LineSwing {
    bob: DVec3,
    previous: DVec3,
    pivot: DVec3,
    time: Option<f64>,
    carried: f64,
    pub turn: Quat,
}
impl LineSwing {
    pub fn direction(&mut self, pivot: DVec3, length: f64, time: f64) -> DVec3 {
        const STEP: f64 = 1. / 120.;
        if !pivot.is_finite() || !length.is_finite() || !time.is_finite() {
            return -DVec3::Y;
        }
        let dt = self.time.map_or(f64::INFINITY, |t| time - t);
        if !(0. ..=0.25).contains(&dt) || length <= 1e-6 {
            self.bob = pivot - DVec3::Y * length.max(0.);
            self.previous = self.bob;
            self.pivot = pivot;
            self.carried = 0.;
        } else {
            let elapsed = dt + self.carried;
            let steps = (elapsed / STEP).floor() as usize;
            self.carried = elapsed - steps as f64 * STEP;
            let keep = (-1.6 * STEP).exp();
            for i in 0..steps {
                let at = self.pivot.lerp(pivot, (i + 1) as f64 / steps as f64);
                let velocity = (self.bob - self.previous) * keep;
                self.previous = self.bob;
                self.bob += velocity - DVec3::Y * (16. * STEP * STEP);
                let delta = self.bob - at;
                let reach = delta.length();
                if reach > length {
                    self.bob = at + delta * (length / reach);
                }
            }
            if steps > 0 {
                self.pivot = pivot;
            }
        }
        self.time = Some(time);
        (self.bob - pivot).try_normalize().unwrap_or(-DVec3::Y)
    }
    pub fn matrix(
        &mut self,
        model_to_world: Mat4,
        origin: DVec3,
        pivot: Vec3,
        end: Vec3,
        time: f64,
    ) -> Mat4 {
        let at = model_to_world.transform_point3(pivot);
        let rest = model_to_world.transform_vector3(end - pivot);
        let length = rest.length();
        if model_to_world.determinant().abs() < 1e-9 || length < 1e-5 {
            return Mat4::IDENTITY;
        }
        let hang = self
            .direction(origin + at.as_dvec3(), f64::from(length), time)
            .as_vec3();
        let (_, frame, _) = model_to_world.to_scale_rotation_translation();
        let world = Quat::from_rotation_arc(rest / length, hang);
        self.turn = frame.inverse() * world * frame;
        Mat4::from_translation(pivot) * Mat4::from_quat(self.turn) * Mat4::from_translation(-pivot)
    }
}

/// A handoff journal, deliberately independent of entity identity/ownership.
/// Callers keep one per model and never replace somebody else's BodyPose.
#[derive(Clone, Debug, Default)]
pub struct OwnershipBlend {
    last: Option<Parts>,
    source: Option<Parts>,
    time: Option<f32>,
    external: bool,
    started: f32,
}
impl OwnershipBlend {
    pub fn apply(&mut self, parts: &mut Parts, external: bool, time: f32, enabled: bool) {
        let continuous = self
            .time
            .is_some_and(|last| time >= last && time - last <= 5.);
        if !enabled || !continuous || !time.is_finite() {
            self.source = None;
        } else if external != self.external {
            self.source = self.last;
            self.started = time;
        }
        let duration = if external { 3. } else { 4. };
        let t = ((time - self.started) / duration).clamp(0., 1.);
        if let Some(source) = &self.source {
            let weight = t * t * (3. - 2. * t);
            for i in 0..6 {
                parts[i] = blend(&source[i], &parts[i], weight);
            }
            if t >= 1. {
                self.source = None;
            }
        }
        self.last = Some(*parts);
        self.external = external;
        self.time = Some(time);
    }
}
