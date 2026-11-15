//! Native equivalents of the inspected Moves 1.0.9 camera and motion helpers.
//! Inputs are plain numbers so engine adapters do not leak Minecraft classes.
use crate::moves::TurnInPlace;
use glam::{Quat, Vec3};

pub fn smooth_step(low: f32, high: f32, value: f32) -> f32 {
    if high <= low {
        return if value >= high { 1. } else { 0. };
    }
    let t = ((value - low) / (high - low)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
pub fn response(rate: f32, seconds: f32) -> f32 {
    1. - (-rate.max(0.1) * seconds.clamp(0., 0.1)).exp()
}
#[derive(Clone, Copy, Debug)]
pub struct CollisionRecovery {
    fraction: f64,
}
impl Default for CollisionRecovery {
    fn default() -> Self {
        Self { fraction: 1. }
    }
}
impl CollisionRecovery {
    pub fn reset(&mut self) {
        self.fraction = 1.;
    }

    pub fn fraction(&self) -> f64 {
        self.fraction
    }

    pub fn update(&mut self, safe: f64, seconds: f32) -> f64 {
        let target = if safe.is_finite() {
            safe.clamp(0., 1.)
        } else {
            0.
        };
        if target <= self.fraction {
            self.fraction = target;
        } else {
            self.fraction +=
                (target - self.fraction) * (1. - (-7. * f64::from(seconds.clamp(0., 0.1))).exp());
            if target - self.fraction < 1e-4 {
                self.fraction = target;
            }
        }
        self.fraction
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Proximity {
    pub blend: f32,
    hold: f32,
}
impl Proximity {
    /// Disabling proximity must cancel the mining latch immediately, while its
    /// framing blend is still allowed to ease out.
    pub fn clear_hold(&mut self) {
        self.hold = 0.;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn hold_seconds(&self) -> f32 {
        self.hold
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit animation and world inputs keep the pure solver independent of the game"
    )]
    pub fn update(
        &mut self,
        distance: f32,
        mining: bool,
        sprinting: bool,
        seconds: f32,
        instant: f32,
        enter: f32,
        hold: f32,
        in_rate: f32,
        out_rate: f32,
    ) -> f32 {
        self.update_precise(
            f64::from(distance),
            mining,
            sprinting,
            seconds,
            instant,
            enter,
            hold,
            in_rate,
            out_rate,
        )
    }

    /// Double-precision world-query entry point; the original f32 API remains
    /// available for animation/runtime callers.
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit animation and world inputs keep the pure solver independent of the game"
    )]
    pub fn update_precise(
        &mut self,
        distance: f64,
        mining: bool,
        sprinting: bool,
        seconds: f32,
        instant: f32,
        enter: f32,
        hold: f32,
        in_rate: f32,
        out_rate: f32,
    ) -> f32 {
        let instant = instant.max(0.);
        let enter = enter.max(instant);
        if mining && distance.is_finite() && distance <= f64::from(enter) {
            self.hold = hold.max(0.);
        } else {
            self.hold = (self.hold - seconds.max(0.)).max(0.);
        }
        let active = (!sprinting && distance.is_finite() && distance <= f64::from(instant))
            || self.hold > 0.0001;
        let target = if active { 1. } else { 0. };
        self.blend +=
            (target - self.blend) * response(if active { in_rate } else { out_rate }, seconds);
        if (self.blend - target).abs() < 0.0001 {
            self.blend = target;
        }
        smooth_step(0., 1., self.blend)
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct MapView {
    pub active: bool,
}
impl MapView {
    pub fn update(&mut self, allowed: bool, pitch: f32, enter: f32, exit: f32) -> bool {
        let enter = enter.clamp(-90., 90.);
        let exit = exit.clamp(-90., enter);
        self.active = allowed && pitch >= if self.active { exit } else { enter };
        self.active
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct TurnLean {
    yaw: Option<f32>,
    previous: f32,
    current: f32,
    holding_jump: bool,
    visible: bool,
}
impl TurnLean {
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit animation and world inputs keep the pure solver independent of the game"
    )]
    pub fn tick(
        &mut self,
        yaw: f32,
        speed: f32,
        forward: f32,
        left: f32,
        allowed: bool,
        takeoff: bool,
        airborne_compatible: bool,
    ) {
        let Some(old) = self.yaw.replace(yaw) else {
            return;
        };
        self.previous = self.current;
        self.holding_jump = airborne_compatible && (self.holding_jump || takeoff && self.visible);
        self.visible = allowed || self.holding_jump;
        if self.holding_jump {
            return;
        }
        let delta = TurnInPlace::wrap_degrees(yaw - old);
        let target = if allowed && [speed, forward, left, delta].iter().all(|v| v.is_finite()) {
            let movement = ((speed - 0.01) / 0.205).clamp(0., 1.25);
            let curve = if delta.abs() <= 60. {
                -delta * 1.4 * forward.clamp(-1., 1.)
            } else {
                0.
            };
            ((curve + 6. * left.clamp(-1., 1.)) * movement).clamp(-14., 14.)
        } else {
            0.
        };
        self.current += (target - self.current) * if target == 0. { 0.3 } else { 0.38 };
        if self.current.abs() < 0.001 {
            self.current = 0.;
        }
    }
    pub fn sample(&self, partial: f32) -> f32 {
        if self.visible {
            self.previous + (self.current - self.previous) * partial.clamp(0., 1.)
        } else {
            0.
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct PassengerTilt {
    yaw: Option<f32>,
    previous: [f32; 2],
    current: [f32; 2],
    turn_rate: f32,
}
impl PassengerTilt {
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit animation and world inputs keep the pure solver independent of the game"
    )]
    pub fn tick(
        &mut self,
        yaw: f32,
        velocity: Vec3,
        speed_threshold: f32,
        bank_degrees: f32,
        bank_sensitivity: f32,
        vertical_degrees: f32,
        vertical_sensitivity: f32,
        rate: f32,
    ) {
        self.previous = self.current;
        let speed = Vec3::new(velocity.x, 0., velocity.z).length();
        let threshold = (speed_threshold * 0.65).max(0.02);
        let horizontal_weight =
            smooth_step(threshold, speed_threshold.max(threshold + 0.001), speed);
        let vertical_weight =
            smooth_step(0.015, (speed_threshold * 0.5).max(0.04), velocity.y.abs());
        let pitch = (-velocity.y * vertical_sensitivity * vertical_degrees)
            .clamp(-vertical_degrees, vertical_degrees)
            * vertical_weight;
        let delta = self
            .yaw
            .replace(yaw)
            .map_or(0., |old| TurnInPlace::wrap_degrees(yaw - old));
        let turn = if delta.abs() >= 0.2 {
            delta * horizontal_weight
        } else {
            0.
        };
        self.turn_rate += (turn - self.turn_rate) * if turn == 0. { 0.45 } else { 0.28 };
        if self.turn_rate.abs() < 0.05 {
            self.turn_rate = 0.;
        }
        let bank = (-self.turn_rate * bank_sensitivity).clamp(-bank_degrees, bank_degrees);
        let alpha = response(rate, 0.05);
        self.current[0] += (pitch - self.current[0]) * alpha;
        self.current[1] += (bank - self.current[1]) * alpha;
    }
    pub fn sample(&self, partial: f32) -> [f32; 2] {
        std::array::from_fn(|i| {
            self.previous[i] + (self.current[i] - self.previous[i]) * partial.clamp(0., 1.)
        })
    }
}

/// Root tilts are about the feet, not the world origin.
pub fn tilt_matrix(pitch: f32, roll: f32) -> glam::Mat4 {
    let rotation = Quat::from_euler(
        glam::EulerRot::ZYX,
        roll.to_radians(),
        0.,
        pitch.to_radians(),
    );
    let pivot = Vec3::new(0., 24., 0.);
    glam::Mat4::from_rotation_translation(rotation, pivot - rotation * pivot)
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    #[test]
    fn recovery_never_smooths_towards_an_obstacle() {
        let mut c = CollisionRecovery::default();
        assert_eq!(c.update(0.2, 0.05), 0.2);
        let r = c.update(1., 0.05);
        assert!(r > 0.2 && r < 1.);
        assert_eq!(c.update(f64::NAN, 0.05), 0.);
    }
    #[test]
    fn map_uses_distinct_enter_exit_thresholds() {
        let mut m = MapView::default();
        assert!(!m.update(true, 54., 55., 18.));
        assert!(m.update(true, 56., 55., 18.));
        assert!(m.update(true, 20., 55., 18.));
        assert!(!m.update(true, 17., 55., 18.));
        assert!(!m.update(false, 80., 55., 18.));
    }
    #[test]
    fn mining_holds_proximity_framing_over_crosshair_edge_changes() {
        let mut p = Proximity::default();
        let first = p.update(2., true, false, 0.05, 1., 2.2, 2., 5., 3.2);
        let next = p.update(f32::INFINITY, false, false, 0.05, 1., 2.2, 2., 5., 3.2);
        assert!(next > first);
    }
}
