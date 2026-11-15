//! Native Moves 1.0.9 camera state machine and shoulder-camera solver.
//!
//! The adapter supplies interpolated player geometry (including the animated
//! swimming eye height), pack profiles and world queries. World positions use
//! doubles so shoulder offsets remain stable far from the world origin.
//! `update` is called once per displayed frame, not once per simulation tick.
use crate::moves::{CameraParameter, ShoulderCamera};
use crate::moves_dynamics::{CollisionRecovery, MapView, Proximity, response, smooth_step};
use glam::{DVec3, Mat4, Vec3};

#[path = "moves_camera_targeting.rs"]
pub mod targeting;
pub use targeting::{
    AimRay, BlockHit, CameraTargeting, CameraWorld, ClipKind, EntityHit, FrameIdentity, PickInput,
    TargetHit,
};

fn finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

fn seconds(value: f32) -> f32 {
    finite(value, 0.).clamp(0., 0.1)
}

/// Camera-relative translation and absolute configured FOV in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraValues {
    pub horizontal: f32,
    pub vertical: f32,
    pub depth: f32,
    pub fov: f32,
}

impl Default for CameraValues {
    fn default() -> Self {
        Self {
            horizontal: 1.17,
            vertical: -0.11,
            depth: 2.5,
            fov: 70.,
        }
    }
}

impl CameraValues {
    fn lerp(self, other: Self, weight: f32) -> Self {
        Self {
            horizontal: self.horizontal + (other.horizontal - self.horizontal) * weight,
            vertical: self.vertical + (other.vertical - self.vertical) * weight,
            depth: self.depth + (other.depth - self.depth) * weight,
            fov: self.fov + (other.fov - self.fov) * weight,
        }
    }

    pub fn desired(self, pivot: DVec3, forward: DVec3, right: DVec3, up: DVec3) -> DVec3 {
        pivot - forward * f64::from(self.depth)
            + right * f64::from(self.horizontal)
            + up * f64::from(self.vertical)
    }
}

/// No bundled pack data: incomplete/missing profiles disable their transition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraProfile {
    pub horizontal: f32,
    pub vertical: f32,
    pub depth: f32,
    pub fov: f32,
    pub response: f32,
}

impl CameraProfile {
    pub const MAP_DEFAULT: Self = Self {
        horizontal: 0.,
        vertical: 0.68,
        depth: 1.,
        fov: 55.,
        response: 4.5,
    };

    pub fn is_complete(self) -> bool {
        self.has_values() && self.response.is_finite() && self.response > 0.
    }

    pub fn has_values(self) -> bool {
        [self.horizontal, self.vertical, self.depth, self.fov]
            .iter()
            .all(|v| v.is_finite())
    }

    pub fn values(self) -> CameraValues {
        CameraValues {
            horizontal: self.horizontal,
            vertical: self.vertical,
            depth: self.depth,
            fov: self.fov,
        }
    }
}

impl Default for CameraProfile {
    fn default() -> Self {
        Self {
            horizontal: f32::NAN,
            vertical: f32::NAN,
            depth: f32::NAN,
            fov: f32::NAN,
            response: f32::NAN,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CameraProfiles {
    /// Pack action `fence`.
    pub fence: Option<CameraProfile>,
    /// Pack action `proximity_camera`.
    pub proximity: Option<CameraProfile>,
    /// Pack action `elytra`.
    pub elytra: Option<CameraProfile>,
    /// Pack action `climbing`.
    pub climbing: Option<CameraProfile>,
    /// Pack action `bow_and_crossbow_camera_fix`.
    pub ranged: Option<CameraProfile>,
    /// Timed item-pose profile takes priority over held-item profile.
    pub timed_throwable: Option<CameraProfile>,
    pub held_throwable: Option<CameraProfile>,
    /// Item-specific map_view settings, or the original numeric fallbacks.
    pub map: Option<CameraProfile>,
}

/// All persistent camera settings. Legacy ShoulderCamera's fields/API stay intact.
#[derive(Clone, Copy, Debug)]
pub struct CameraSettings {
    pub enabled: bool,
    pub values: CameraValues,
    pub rotation_yaw: f32,
    pub rotation_pitch: f32,
    pub fence_camera: bool,
    pub ladder_camera: bool,
    pub proximity_camera: bool,
    pub elytra_camera: bool,
    pub map_camera: bool,
    pub bow_and_crossbow_camera_fix: bool,
    pub spear_and_trident_camera_fix: bool,
    /// Forward this flag to the animation/render adapter for ladder body facing.
    pub ladder_body_lock: bool,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            values: CameraValues::default(),
            rotation_yaw: 0.,
            rotation_pitch: 0.,
            fence_camera: true,
            ladder_camera: true,
            proximity_camera: true,
            elytra_camera: true,
            map_camera: true,
            bow_and_crossbow_camera_fix: true,
            spear_and_trident_camera_fix: true,
            ladder_body_lock: true,
        }
    }
}

impl CameraSettings {
    pub fn from_shoulder(camera: &ShoulderCamera) -> Self {
        let mut settings = Self {
            enabled: camera.enabled,
            values: CameraValues {
                horizontal: camera.horizontal,
                vertical: camera.vertical,
                depth: camera.depth,
                fov: camera.fov,
            },
            rotation_yaw: camera.rotation_yaw,
            rotation_pitch: camera.rotation_pitch,
            ..Self::default()
        };
        settings.sanitize();
        settings
    }

    pub fn apply_to_shoulder(&self, camera: &mut ShoulderCamera) {
        camera.enabled = self.enabled;
        camera.horizontal = self.values.horizontal;
        camera.vertical = self.values.vertical;
        camera.depth = self.values.depth;
        camera.fov = self.values.fov;
        camera.rotation_yaw = self.rotation_yaw;
        camera.rotation_pitch = self.rotation_pitch;
    }

    pub fn sanitize(&mut self) {
        self.values.horizontal = CameraParameter::Horizontal.clamp(finite(
            self.values.horizontal,
            CameraParameter::Horizontal.default_value(),
        ));
        self.values.vertical = CameraParameter::Vertical.clamp(finite(
            self.values.vertical,
            CameraParameter::Vertical.default_value(),
        ));
        self.values.depth = CameraParameter::Depth.clamp(finite(
            self.values.depth,
            CameraParameter::Depth.default_value(),
        ));
        self.values.fov = CameraParameter::Fov.clamp(finite(
            self.values.fov,
            CameraParameter::Fov.default_value(),
        ));
        self.rotation_yaw = finite(self.rotation_yaw, 0.).clamp(-30., 30.);
        self.rotation_pitch = finite(self.rotation_pitch, 0.).clamp(-30., 30.);
    }

    pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        let mut settings = Self::from_shoulder(&ShoulderCamera::read(&get));
        for (key, flag) in [
            ("moves_camera_fence", &mut settings.fence_camera),
            ("moves_camera_ladder", &mut settings.ladder_camera),
            ("moves_camera_proximity", &mut settings.proximity_camera),
            ("moves_camera_elytra", &mut settings.elytra_camera),
            ("moves_camera_map", &mut settings.map_camera),
            (
                "moves_camera_bow_and_crossbow_fix",
                &mut settings.bow_and_crossbow_camera_fix,
            ),
            (
                "moves_camera_spear_and_trident_fix",
                &mut settings.spear_and_trident_camera_fix,
            ),
            (
                "moves_camera_ladder_body_lock",
                &mut settings.ladder_body_lock,
            ),
        ] {
            if let Some(value) = get(key).and_then(|v| v.parse::<bool>().ok()) {
                *flag = value;
            }
        }
        settings
    }

    /// Persistence is owned by the game; this emits the complete native setting set.
    pub fn write(&self, mut set: impl FnMut(&str, String)) {
        for (key, value) in [
            ("moves_camera_horizontal", self.values.horizontal),
            ("moves_camera_vertical", self.values.vertical),
            ("moves_camera_depth", self.values.depth),
            ("moves_camera_fov", self.values.fov),
            ("moves_camera_rot_yaw", self.rotation_yaw),
            ("moves_camera_rot_pitch", self.rotation_pitch),
        ] {
            set(key, value.to_string());
        }
        for (key, value) in [
            ("moves_camera_enabled", self.enabled),
            ("moves_camera_fence", self.fence_camera),
            ("moves_camera_ladder", self.ladder_camera),
            ("moves_camera_proximity", self.proximity_camera),
            ("moves_camera_elytra", self.elytra_camera),
            ("moves_camera_map", self.map_camera),
            (
                "moves_camera_bow_and_crossbow_fix",
                self.bow_and_crossbow_camera_fix,
            ),
            (
                "moves_camera_spear_and_trident_fix",
                self.spear_and_trident_camera_fix,
            ),
            ("moves_camera_ladder_body_lock", self.ladder_body_lock),
        ] {
            set(key, value.to_string());
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Perspective {
    #[default]
    FirstPerson,
    ThirdPersonBack,
    ThirdPersonFront,
}

#[derive(Clone, Copy, Debug)]
pub enum CameraCommand {
    Toggle,
    SetEnabled(bool),
}

#[derive(Clone, Copy, Debug)]
pub struct ProximitySettings {
    pub instant_distance: f32,
    pub enter_distance: f32,
    pub mining_hold_seconds: f32,
    pub blend_in_response: f32,
    pub blend_out_response: f32,
}

impl Default for ProximitySettings {
    fn default() -> Self {
        Self {
            instant_distance: 1.,
            enter_distance: 2.2,
            mining_hold_seconds: 2.,
            blend_in_response: 5.,
            blend_out_response: 3.2,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CameraInput {
    pub elapsed_seconds: f32,
    /// Current vanilla perspective; apply `requested_perspective` before next frame.
    pub perspective: Perspective,
    pub player_present: bool,
    pub focused_entity_present: bool,
    pub detached: bool,
    pub local_player_view: bool,
    /// Spatial UI cameras yield unless this is their target-computation pass.
    pub external_camera_active: bool,
    pub computing_external_target: bool,
    /// Must be set on the actual vanilla perspective hotkey, even if it cycles
    /// back to rear third person between frames. Does not disable saved settings.
    pub vanilla_perspective_changed: bool,
    pub command: Option<CameraCommand>,
    pub toggle_orbit: bool,
    /// Raw mouse delta before vanilla's 0.15 degree multiplier.
    pub mouse_delta: Option<[f32; 2]>,
    /// Interpolated focused-entity view rotations used for camera rendering.
    pub yaw: f32,
    pub pitch: f32,
    /// Optional current (uninterpolated) local-player look, used when entering
    /// orbit and evaluating map hysteresis. Defaults to yaw/pitch.
    pub player_look: Option<LookAngles>,
    /// Interpolated entity origin + animation-adjusted camera eye height.
    pub pivot: DVec3,
    /// Interpolated actual entity eye, used for stable reach and visibility.
    pub eye: DVec3,
    /// Interpolated world-space player bounds (not last tick's bounds).
    pub bounds_min: DVec3,
    pub bounds_max: DVec3,
    pub entity_width: f64,
    pub entity_height: f64,
    pub eye_height: f64,
    pub block_reach: f64,
    /// Vanilla base/options FOV, before sprint/zoom effects (not calculated FOV).
    pub vanilla_fov: f32,
    pub using_spyglass: bool,
    pub grounded: bool,
    pub supported_by_fence: bool,
    /// True only for the local camera, no GUI, attack held, block under crosshair.
    pub mining_target: bool,
    pub sprinting: bool,
    pub fall_flying: bool,
    pub climbing: bool,
    pub swimming: bool,
    /// Riding also suppresses map view when no passenger camera profile exists.
    pub riding: bool,
    pub passenger: Option<CameraProfile>,
    pub holding_filled_map: bool,
    /// Share this result with map animation when it already evaluated the latch.
    pub map_view_override: Option<bool>,
    pub map_enter_pitch: f32,
    pub map_exit_pitch: f32,
    pub using_bow: bool,
    pub holding_charged_crossbow: bool,
    pub timed_throwable_pose: bool,
    pub holding_throwable: bool,
    pub charging_spear_or_trident: bool,
    pub spear_attack_camera: bool,
    pub profiles: CameraProfiles,
    pub proximity: ProximitySettings,
}

impl Default for CameraInput {
    fn default() -> Self {
        Self {
            elapsed_seconds: 0.,
            perspective: Perspective::FirstPerson,
            player_present: false,
            focused_entity_present: false,
            detached: true,
            local_player_view: true,
            external_camera_active: false,
            computing_external_target: false,
            vanilla_perspective_changed: false,
            command: None,
            toggle_orbit: false,
            mouse_delta: None,
            yaw: 0.,
            pitch: 0.,
            player_look: None,
            pivot: DVec3::ZERO,
            eye: DVec3::ZERO,
            bounds_min: DVec3::new(-0.3, -1.62, -0.3),
            bounds_max: DVec3::new(0.3, 0.18, 0.3),
            entity_width: 0.6,
            entity_height: 1.8,
            eye_height: 1.62,
            block_reach: 4.5,
            vanilla_fov: 70.,
            using_spyglass: false,
            grounded: false,
            supported_by_fence: false,
            mining_target: false,
            sprinting: false,
            fall_flying: false,
            climbing: false,
            swimming: false,
            riding: false,
            passenger: None,
            holding_filled_map: false,
            map_view_override: None,
            map_enter_pitch: 42.,
            map_exit_pitch: 32.,
            using_bow: false,
            holding_charged_crossbow: false,
            timed_throwable_pose: false,
            holding_throwable: false,
            charging_spear_or_trident: false,
            spear_attack_camera: false,
            profiles: CameraProfiles::default(),
            proximity: ProximitySettings::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookAngles {
    pub yaw: f32,
    pub pitch: f32,
}

impl LookAngles {
    /// Minecraft conventions: yaw zero looks +Z, positive pitch looks down.
    pub fn toward(eye: DVec3, target: DVec3) -> Option<Self> {
        let delta = target - eye;
        if !delta.is_finite() || delta.length_squared() < 1e-12 {
            return None;
        }
        Some(Self {
            yaw: (-delta.x).atan2(delta.z).to_degrees() as f32,
            pitch: (-delta.y).atan2(delta.x.hypot(delta.z)).to_degrees() as f32,
        })
    }

    pub fn basis(self) -> (DVec3, DVec3, DVec3) {
        let (sin_y, cos_y) = f64::from(self.yaw).to_radians().sin_cos();
        let (sin_p, cos_p) = f64::from(self.pitch).to_radians().sin_cos();
        let forward = DVec3::new(-sin_y * cos_p, -sin_p, cos_y * cos_p);
        let right = DVec3::new(cos_y, 0., sin_y);
        (forward, right, forward.cross(right))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvatarVisibility {
    pub alpha: f32,
    pub skip_avatar: bool,
}

impl Default for AvatarVisibility {
    fn default() -> Self {
        Self {
            alpha: 1.,
            skip_avatar: false,
        }
    }
}

impl AvatarVisibility {
    /// Local avatar draws must use their translucent material below this cutoff,
    /// including armor, held items and other avatar render layers.
    pub fn is_translucent(self) -> bool {
        self.alpha < 0.999
    }

    /// Apply only inside local-avatar render submission, never to world/UI tint.
    pub fn apply_argb(self, tint: u32) -> u32 {
        let alpha = (((tint >> 24) & 0xff) as f32 * self.alpha.clamp(0., 1.)).round() as u32;
        (tint & 0x00ff_ffff) | (alpha << 24)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit animation and world inputs keep the pure solver independent of the game"
    )]
    pub fn sample(
        desired_offset: DVec3,
        camera_offset: DVec3,
        right: DVec3,
        up: DVec3,
        width: f64,
        height: f64,
        eye_height: f64,
        first_person_transition: bool,
    ) -> Self {
        let visible = Self::default();
        if !first_person_transition && desired_offset.distance_squared(camera_offset) <= 1e-8 {
            return visible;
        }
        let width = width.max(0.001);
        let distance = camera_offset.length();
        let inner_radius = width * 0.75;
        if distance < inner_radius {
            return Self {
                alpha: 1.,
                skip_avatar: true,
            };
        }
        let fade_radius = width * 2.5;
        if distance >= fade_radius {
            return visible;
        }
        let lateral = camera_offset.dot(right).abs();
        let vertical = camera_offset.dot(up);
        let half_width = width * 0.5;
        let eye_height = eye_height.max(0.001);
        let head_room = (height - eye_height).max(0.001);
        if lateral >= half_width || vertical <= -eye_height || vertical >= head_room {
            return visible;
        }
        let x = (lateral / half_width).clamp(0., 1.);
        let y = if vertical >= 0. {
            (vertical / head_room).clamp(0., 1.)
        } else {
            (-vertical / eye_height).clamp(0., 1.)
        };
        let aligned_alpha = x.hypot(y).clamp(0.15, 1.) as f32;
        let weight = smooth_step(inner_radius as f32, fade_radius as f32, distance as f32);
        Self {
            alpha: aligned_alpha + (1. - aligned_alpha) * weight,
            skip_avatar: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CameraOutput {
    /// Apply position/rotation when true, including the final exit frame.
    pub solved: bool,
    /// Controller remains active after finishing this frame.
    pub active: bool,
    pub requested_perspective: Option<Perspective>,
    /// Apply both current AND previous player yaw/pitch when leaving orbit.
    pub apply_player_look: Option<LookAngles>,
    pub consume_mouse_delta: bool,
    pub settings_changed: bool,
    pub orbit_toggle_rejected: bool,
    pub position: DVec3,
    pub desired_position: DVec3,
    pub rotation: LookAngles,
    pub values: CameraValues,
    /// Multiply vanilla's calculated FOV by this to retain sprint/zoom effects.
    pub fov_multiplier: f32,
    pub activation_transition: bool,
    pub activation_blend: f32,
    pub orbit_active: bool,
    pub map_view_active: bool,
    pub proximity_distance: f64,
    pub collision_fraction: f64,
    pub avatar: AvatarVisibility,
    pub hide_local_player: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Transition {
    blend: f32,
    profile: Option<CameraProfile>,
}

impl Transition {
    fn update(&mut self, active: bool, requested: Option<CameraProfile>, elapsed: f32) {
        let requested = requested.filter(|p| p.is_complete());
        if let Some(profile) = requested {
            self.profile = Some(profile);
        }
        self.blend = blend_toward(
            self.blend,
            active && requested.is_some(),
            self.profile.map_or(4., |p| p.response),
            elapsed,
        );
    }

    fn apply(self, value: CameraValues) -> CameraValues {
        self.profile.map_or(value, |profile| {
            value.lerp(profile.values(), smooth_step(0., 1., self.blend))
        })
    }
}

fn blend_toward(blend: f32, active: bool, rate: f32, elapsed: f32) -> f32 {
    let target = if active { 1. } else { 0. };
    let alpha = 1. - (-finite(rate, 4.).max(0.) * seconds(elapsed)).exp();
    let result = blend + (target - blend) * alpha;
    if (result - target).abs() < 1e-4 {
        target
    } else {
        result
    }
}

#[derive(Clone, Debug, Default)]
struct DynamicCamera {
    fence: Transition,
    fence_latched: bool,
    proximity: Proximity,
    proximity_profile: Option<CameraProfile>,
    elytra: Transition,
    climbing: Transition,
    passenger: Transition,
    map: Transition,
    ranged: Transition,
}

impl DynamicCamera {
    fn contextual(&self, base: CameraValues, proximity: bool) -> CameraValues {
        // Original precedence is significant: ranged is last, elytra beats ladder.
        let mut value = self.fence.apply(base);
        if proximity && let Some(profile) = self.proximity_profile {
            value = value.lerp(profile.values(), smooth_step(0., 1., self.proximity.blend));
        }
        value = self.climbing.apply(value);
        value = self.elytra.apply(value);
        value = self.passenger.apply(value);
        value = self.map.apply(value);
        self.ranged.apply(value)
    }
}

/// Retain one controller per actual camera. No global/static player state.
#[derive(Clone, Debug, Default)]
pub struct CameraController {
    activated: bool,
    activation_requested: bool,
    activation_blend: f32,
    vanilla_override: bool,
    spyglass_takeover: bool,
    transition: CameraValues,
    dynamic: DynamicCamera,
    orbit: Option<LookAngles>,
    map_view: MapView,
    initialized: bool,
    smoothed: CameraValues,
    collision: CollisionRecovery,
    first_person_grace: f32,
}

impl CameraController {
    pub fn activation_blend(&self) -> f32 {
        self.activation_blend
    }

    pub fn activation_requested(&self) -> bool {
        self.activation_requested
    }

    pub fn is_activation_transition_running(&self) -> bool {
        self.activated && (!self.activation_requested || self.activation_blend < 1.)
    }

    pub fn is_active(&self, input: &CameraInput) -> bool {
        self.has_custom_camera(input)
            && (!input.external_camera_active || input.computing_external_target)
    }

    pub fn has_custom_camera(&self, input: &CameraInput) -> bool {
        self.activated && input.player_present && input.perspective == Perspective::ThirdPersonBack
    }

    pub fn orbit_angles(&self) -> Option<LookAngles> {
        self.orbit
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn reset_dynamic_transitions(&mut self) {
        self.dynamic = DynamicCamera::default();
        self.initialized = false;
    }

    /// Call on vanilla F5 rather than treating saved enabled=true as authority
    /// to override the user's vanilla perspective choice.
    pub fn on_vanilla_perspective_change(&mut self) -> Option<LookAngles> {
        self.vanilla_override = true;
        self.activated = false;
        self.activation_requested = false;
        self.activation_blend = 0.;
        self.spyglass_takeover = false;
        self.first_person_grace = 0.;
        self.reset_dynamic_transitions();
        self.orbit.take()
    }

    pub fn effective_values(&self, settings: &CameraSettings, vanilla_fov: f32) -> CameraValues {
        let contextual = self.dynamic.contextual(settings.values, true);
        if !self.activated || self.activation_blend >= 1. {
            return contextual;
        }
        let blend = self.activation_blend;
        CameraValues {
            horizontal: self.transition.horizontal * smooth_step(0.18, 1., blend),
            vertical: self.transition.vertical * smooth_step(0.12, 1., blend),
            depth: self.transition.depth * smooth_step(0., 1., blend),
            fov: finite(vanilla_fov, 70.)
                + (self.transition.fov - finite(vanilla_fov, 70.)) * smooth_step(0.3, 1., blend),
        }
    }

    /// Framing with proximity omitted prevents positive feedback from its own
    /// shoulder displacement changing the next frame's measured target.
    pub fn proximity_neutral_values(&self, settings: &CameraSettings) -> CameraValues {
        self.dynamic.contextual(settings.values, false)
    }

    fn set_enabled(
        &mut self,
        enabled: bool,
        settings: &mut CameraSettings,
        perspective: &mut Perspective,
    ) -> Option<LookAngles> {
        self.vanilla_override = false;
        self.first_person_grace = 0.;
        let look = if enabled { None } else { self.orbit.take() };
        if enabled {
            if !self.activated {
                self.activation_blend = 0.;
            }
            self.activated = true;
            self.transition = settings.values;
            *perspective = Perspective::ThirdPersonBack;
        } else if self.activated {
            self.transition = self.dynamic.contextual(settings.values, true);
        } else {
            *perspective = Perspective::FirstPerson;
        }
        self.activation_requested = enabled;
        settings.enabled = enabled;
        look
    }

    pub fn turn_free_camera(
        &mut self,
        input: &CameraInput,
        yaw_delta: f32,
        pitch_delta: f32,
    ) -> bool {
        if !self.is_active(input) {
            self.orbit = None;
            return false;
        }
        let Some(angles) = self.orbit.as_mut() else {
            return false;
        };
        angles.yaw += (f64::from(finite(yaw_delta, 0.)) * 0.15) as f32;
        angles.pitch =
            (angles.pitch + (f64::from(finite(pitch_delta, 0.)) * 0.15) as f32).clamp(-90., 90.);
        true
    }

    /// Complete update/solve. A single world adapter can serve both this camera
    /// solver and CameraTargeting. No collision query may include entity bounds.
    pub fn update(
        &mut self,
        input: &CameraInput,
        settings: &mut CameraSettings,
        world: &mut impl CameraWorld,
    ) -> CameraOutput {
        settings.sanitize();
        let elapsed = seconds(input.elapsed_seconds);
        self.first_person_grace =
            (self.first_person_grace - finite(input.elapsed_seconds, 0.).max(0.)).max(0.);
        let mut current = *input;
        let mut requested_perspective = None;
        let mut apply_player_look = None;
        let mut settings_changed = false;
        if input.vanilla_perspective_changed {
            apply_player_look = self.on_vanilla_perspective_change();
        }
        if let Some(command) = input.command.filter(|_| input.player_present) {
            let enabled = match command {
                CameraCommand::Toggle => !self.activation_requested,
                CameraCommand::SetEnabled(enabled) => enabled,
            };
            if let Some(look) = self.set_enabled(enabled, settings, &mut current.perspective) {
                apply_player_look = Some(look);
            }
            requested_perspective = Some(current.perspective);
            settings_changed = true;
        }

        let external = current.external_camera_active && !current.computing_external_target;
        if !external {
            // Spyglass is evaluated BEFORE activation advancement, matching CameraMixin.
            if current.local_player_view && current.using_spyglass {
                if !self.spyglass_takeover
                    && current.perspective == Perspective::ThirdPersonBack
                    && self.activated
                    && self.activation_requested
                {
                    self.spyglass_takeover = true;
                    apply_player_look = self.orbit.take().or(apply_player_look);
                    self.transition = self.dynamic.contextual(settings.values, true);
                    self.activation_requested = false;
                }
            } else if self.spyglass_takeover {
                self.spyglass_takeover = false;
                if current.player_present {
                    self.activated = true;
                    self.activation_requested = true;
                    current.perspective = Perspective::ThirdPersonBack;
                    requested_perspective = Some(current.perspective);
                    self.transition = settings.values;
                }
            }
            if self.activated && current.perspective != Perspective::ThirdPersonBack {
                apply_player_look = self.on_vanilla_perspective_change().or(apply_player_look);
            }
            if current.player_present
                && !self.vanilla_override
                && !self.activated
                && !self.spyglass_takeover
                && settings.enabled
                && current.perspective == Perspective::ThirdPersonBack
            {
                self.activation_blend = 0.;
                self.activated = true;
                self.activation_requested = true;
                self.transition = settings.values;
            }
            if self.activated {
                let duration = if self.activation_requested { 0.9 } else { 0.72 };
                let direction = if self.activation_requested { 1. } else { -1. };
                self.activation_blend =
                    (self.activation_blend + direction * elapsed / duration).clamp(0., 1.);
            }
        }

        let active = self.is_active(&current);
        let mut orbit_toggle_rejected = false;
        if current.toggle_orbit && current.player_present {
            if active {
                if self.orbit.is_some() {
                    apply_player_look = self.orbit.take().or(apply_player_look);
                } else {
                    let look = current.player_look.unwrap_or(LookAngles {
                        yaw: current.yaw,
                        pitch: current.pitch,
                    });
                    self.orbit = Some(LookAngles {
                        yaw: finite(look.yaw, 0.),
                        pitch: finite(look.pitch, 0.).clamp(-90., 90.),
                    });
                }
            } else {
                orbit_toggle_rejected = true;
            }
        }
        // Orbit release must affect this frame's camera as well as next tick's player.
        if let Some(look) = apply_player_look {
            current.yaw = look.yaw;
            current.pitch = look.pitch;
            current.player_look = Some(look);
        }
        let consume_mouse_delta = current
            .mouse_delta
            .is_some_and(|[yaw, pitch]| self.turn_free_camera(&current, yaw, pitch));
        let base_rotation = self.orbit.filter(|_| active).unwrap_or(LookAngles {
            yaw: finite(current.yaw, 0.),
            pitch: finite(current.pitch, 0.).clamp(-90., 90.),
        });
        let rotation = LookAngles {
            yaw: base_rotation.yaw + settings.rotation_yaw,
            pitch: (base_rotation.pitch + settings.rotation_pitch).clamp(-90., 90.),
        };
        let map_allowed = settings.map_camera
            && current.focused_entity_present
            && current.holding_filled_map
            && !current.fall_flying
            && !current.swimming
            && !current.climbing
            && !current.riding
            && current.passenger.is_none();
        let map_view_active = match current.map_view_override {
            Some(active) => {
                self.map_view.active = map_allowed && active;
                self.map_view.active
            }
            None => self.map_view.update(
                map_allowed,
                if active && self.orbit.is_some() {
                    base_rotation.pitch
                } else {
                    finite(
                        current.player_look.map_or(current.pitch, |look| look.pitch),
                        0.,
                    )
                },
                finite(current.map_enter_pitch, 42.),
                finite(current.map_exit_pitch, 32.),
            ),
        };
        let mut output = CameraOutput {
            solved: false,
            active,
            requested_perspective,
            apply_player_look,
            consume_mouse_delta,
            settings_changed,
            orbit_toggle_rejected,
            position: current.pivot,
            desired_position: current.pivot,
            rotation,
            values: self.effective_values(settings, current.vanilla_fov),
            fov_multiplier: 1.,
            activation_transition: self.is_activation_transition_running(),
            activation_blend: self.activation_blend,
            orbit_active: active && self.orbit.is_some(),
            map_view_active,
            proximity_distance: f64::INFINITY,
            collision_fraction: 1.,
            avatar: AvatarVisibility::default(),
            hide_local_player: false,
        };
        if active {
            if current.vanilla_fov.is_finite() && current.vanilla_fov > 0. {
                output.fov_multiplier = output.values.fov / current.vanilla_fov;
            }
            output.hide_local_player = current.local_player_view
                && (self.first_person_grace > 0.
                    || output.activation_transition && output.values.depth < 0.58);
        }
        if !active
            || !current.detached
            || !current.focused_entity_present
            || !current.pivot.is_finite()
        {
            self.reset_dynamic_transitions();
            return output;
        }

        let (forward, right, up) = rotation.basis();
        if !output.activation_transition {
            self.update_dynamic(
                &current,
                settings,
                elapsed,
                map_view_active,
                forward,
                right,
                up,
                world,
                &mut output,
            );
        }
        output.values = self.effective_values(settings, current.vanilla_fov);
        if !self.initialized {
            self.initialized = true;
            self.collision.reset();
            self.smoothed = output.values;
        } else if output.activation_transition {
            self.smoothed = output.values;
        } else {
            self.smoothed = self.smoothed.lerp(output.values, response(12., elapsed));
        }
        let desired = self.smoothed.desired(current.pivot, forward, right, up);
        let safe = safe_travel_fraction(world, current.pivot, desired, right, up);
        let fraction = self.collision.update(safe, elapsed);
        let position = current.pivot + (desired - current.pivot) * fraction;
        output.solved = true;
        output.position = position;
        output.desired_position = desired;
        output.collision_fraction = fraction;
        if current.vanilla_fov.is_finite() && current.vanilla_fov > 0. {
            output.fov_multiplier = output.values.fov / current.vanilla_fov;
        }
        if current.local_player_view {
            output.avatar = AvatarVisibility::sample(
                desired - current.pivot,
                position - current.pivot,
                right,
                up,
                current.entity_width,
                current.entity_height,
                current.eye_height,
                output.activation_transition,
            );
            output.hide_local_player = self.first_person_grace > 0.
                || output.activation_transition && output.values.depth < 0.58
                || output.avatar.skip_avatar;
        }
        if self.activated && !self.activation_requested && self.activation_blend <= 0. {
            self.first_person_grace = 0.2;
            self.activated = false;
            output.active = false;
            output.orbit_active = false;
            output.requested_perspective = Some(Perspective::FirstPerson);
            self.reset_dynamic_transitions();
        }
        output
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit animation and world inputs keep the pure solver independent of the game"
    )]
    fn update_dynamic(
        &mut self,
        input: &CameraInput,
        settings: &CameraSettings,
        elapsed: f32,
        map_active: bool,
        forward: DVec3,
        right: DVec3,
        up: DVec3,
        world: &mut impl CameraWorld,
        output: &mut CameraOutput,
    ) {
        // Update order also matters: proximity's neutral framing uses this frame's
        // fence latch but last frame's other dynamic transitions.
        let fence_available = input.profiles.fence.is_some_and(|p| p.is_complete());
        if !settings.fence_camera || !fence_available {
            self.dynamic.fence_latched = false;
        } else if input.supported_by_fence {
            self.dynamic.fence_latched = true;
        } else if input.grounded {
            self.dynamic.fence_latched = false;
        }
        self.dynamic
            .fence
            .update(self.dynamic.fence_latched, input.profiles.fence, elapsed);
        let distance = proximity_sight_distance(
            world,
            input,
            self.proximity_neutral_values(settings),
            forward,
            right,
            up,
        );
        output.proximity_distance = distance;
        let proximity_profile = input.profiles.proximity.filter(|p| p.is_complete());
        if let Some(profile) = proximity_profile {
            self.dynamic.proximity_profile = Some(profile);
        }
        let enabled = settings.proximity_camera && proximity_profile.is_some();
        if !enabled {
            self.dynamic.proximity.clear_hold();
        }
        let config = input.proximity;
        self.dynamic.proximity.update_precise(
            if enabled { distance } else { f64::INFINITY },
            enabled && input.local_player_view && input.mining_target,
            input.local_player_view && input.sprinting,
            elapsed,
            finite(config.instant_distance, 1.),
            finite(config.enter_distance, 2.2),
            finite(config.mining_hold_seconds, 2.),
            finite(config.blend_in_response, 5.),
            finite(config.blend_out_response, 3.2),
        );
        self.dynamic.elytra.update(
            settings.elytra_camera && input.fall_flying,
            input.profiles.elytra,
            elapsed,
        );
        self.dynamic.climbing.update(
            settings.ladder_camera && input.climbing,
            input.profiles.climbing,
            elapsed,
        );
        let passenger = input
            .passenger
            .filter(|p| p.has_values())
            .map(|mut profile| {
                profile.response = finite(profile.response, 4.).max(0.1);
                profile
            });
        self.dynamic.passenger.update(true, passenger, elapsed);
        if self.dynamic.passenger.blend == 0. && input.passenger.is_none() {
            self.dynamic.passenger.profile = None;
        }
        let mut map = input
            .profiles
            .map
            .filter(|p| p.has_values())
            .unwrap_or(CameraProfile::MAP_DEFAULT);
        map.response = finite(map.response, 4.5).max(0.1);
        self.dynamic.map.update(map_active, Some(map), elapsed);
        let bow = settings.bow_and_crossbow_camera_fix
            && (input.using_bow || input.holding_charged_crossbow);
        let timed = settings.bow_and_crossbow_camera_fix && input.timed_throwable_pose;
        let held = settings.bow_and_crossbow_camera_fix && input.holding_throwable;
        let spear = settings.spear_and_trident_camera_fix
            && (input.charging_spear_or_trident || input.spear_attack_camera);
        let throwable = if timed {
            input.profiles.timed_throwable
        } else {
            input.profiles.held_throwable
        };
        let profile = if (timed || held) && !bow && throwable.is_some_and(|p| p.is_complete()) {
            throwable
        } else {
            input.profiles.ranged
        };
        self.dynamic
            .ranged
            .update(bow || timed || held || spear, profile, elapsed);
    }
}

/// Five collider rays, no fluid collision, with the original distance-scaled
/// wall margin. Inward motion is immediate; CollisionRecovery eases only outward.
pub fn safe_travel_fraction(
    world: &mut impl CameraWorld,
    pivot: DVec3,
    desired: DVec3,
    right: DVec3,
    up: DVec3,
) -> f64 {
    let travel = desired - pivot;
    let distance_squared = travel.length_squared();
    if !distance_squared.is_finite() || distance_squared < 1e-8 {
        return 0.;
    }
    let mut allowed: f64 = 1.;
    for offset in [
        DVec3::ZERO,
        right * 0.12,
        -right * 0.12,
        up * 0.12,
        -up * 0.12,
    ] {
        let from = pivot + offset;
        if let Some(hit) = world.clip(from, desired + offset, ClipKind::Collider) {
            let fraction = (hit.position - from).dot(travel) / distance_squared;
            if !fraction.is_finite() {
                return 0.;
            }
            allowed = allowed.min(fraction);
        }
    }
    if allowed >= 1. {
        1.
    } else {
        (allowed - (0.08 / distance_squared.sqrt()).min(0.08)).clamp(0., 1.)
    }
}

/// Stable, collision-clipped neutral-camera ray, reach measured from the actual
/// eye, distance measured to the interpolated body AABB rather than the camera.
pub fn proximity_sight_distance(
    world: &mut impl CameraWorld,
    input: &CameraInput,
    neutral: CameraValues,
    forward: DVec3,
    right: DVec3,
    up: DVec3,
) -> f64 {
    if !input.eye.is_finite() || !input.block_reach.is_finite() || input.block_reach < 0. {
        return f64::INFINITY;
    }
    let desired = neutral.desired(input.pivot, forward, right, up);
    let fraction = safe_travel_fraction(world, input.pivot, desired, right, up);
    let origin = input.pivot + (desired - input.pivot) * fraction;
    let end = origin + forward * (input.block_reach + origin.distance(input.eye) + 1.);
    let Some(hit) = world.clip(origin, end, ClipKind::Outline) else {
        return f64::INFINITY;
    };
    if !hit.position.is_finite()
        || hit.position.distance_squared(input.eye) > input.block_reach * input.block_reach
        || !input.bounds_min.is_finite()
        || !input.bounds_max.is_finite()
    {
        return f64::INFINITY;
    }
    let nearest = hit.position.clamp(
        input.bounds_min.min(input.bounds_max),
        input.bounds_min.max(input.bounds_max),
    );
    hit.position.distance(nearest)
}

/// Recover the exact optical ray from displayed, camera-relative matrices.
/// The projection must already contain hurt bob, view bob and portal/nausea.
pub fn center_aim_ray(camera_position: DVec3, view: Mat4, projection: Mat4) -> Option<AimRay> {
    let (origin, direction) = crate::moves::unproject_center_ray(view, projection)?;
    AimRay::new(camera_position + origin.as_dvec3(), direction.as_dvec3())
}

/// Reproduce CameraAimFrameMixin's projection composition. `view_effects` is the
/// hurt-bob matrix followed by optional view bob from the displayed frame.
pub fn projection_with_effects(
    projection: Mat4,
    view_effects: Mat4,
    portal_intensity: f32,
    nausea_intensity: f32,
    screen_effect_scale: f32,
    spinning_effect_angle: f32,
) -> Mat4 {
    let mut projection = projection * view_effects;
    let scale = finite(screen_effect_scale, 0.);
    let intensity = finite(portal_intensity, 0.).max(finite(nausea_intensity, 0.)) * scale * scale;
    if intensity > 0. {
        let stretch = 5. / (intensity * intensity + 5.) - 0.04 * intensity;
        let axis = Vec3::new(
            0.,
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2,
        );
        let angle = finite(spinning_effect_angle, 0.).to_radians();
        projection = projection
            * Mat4::from_axis_angle(axis, angle)
            * Mat4::from_scale(Vec3::new(1. / (stretch * stretch), 1., 1.))
            * Mat4::from_axis_angle(axis, -angle);
    }
    projection
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    use glam::IVec3;

    #[derive(Default)]
    struct EmptyWorld;
    impl CameraWorld for EmptyWorld {
        fn clip(&mut self, _: DVec3, _: DVec3, _: ClipKind) -> Option<BlockHit> {
            None
        }
    }

    fn input() -> CameraInput {
        CameraInput {
            player_present: true,
            focused_entity_present: true,
            perspective: Perspective::ThirdPersonBack,
            elapsed_seconds: 0.1,
            ..CameraInput::default()
        }
    }

    fn settle(
        controller: &mut CameraController,
        input: &CameraInput,
        settings: &mut CameraSettings,
    ) {
        for _ in 0..30 {
            controller.update(input, settings, &mut EmptyWorld);
        }
    }

    fn profile(horizontal: f32) -> CameraProfile {
        CameraProfile {
            horizontal,
            vertical: 0.,
            depth: 2.,
            fov: 70.,
            response: 4.,
        }
    }

    #[test]
    fn saved_camera_never_forces_a_vanilla_perspective() {
        let mut controller = CameraController::default();
        let mut settings = CameraSettings::default();
        let mut input = input();
        input.perspective = Perspective::FirstPerson;
        assert!(
            !controller
                .update(&input, &mut settings, &mut EmptyWorld)
                .active
        );
        input.perspective = Perspective::ThirdPersonBack;
        assert!(
            controller
                .update(&input, &mut settings, &mut EmptyWorld)
                .active
        );
        input.vanilla_perspective_changed = true;
        assert!(
            !controller
                .update(&input, &mut settings, &mut EmptyWorld)
                .active
        );
        input.vanilla_perspective_changed = false;
        assert!(
            !controller
                .update(&input, &mut settings, &mut EmptyWorld)
                .active
        );
        assert!(settings.enabled);
        input.command = Some(CameraCommand::Toggle);
        assert!(
            controller
                .update(&input, &mut settings, &mut EmptyWorld)
                .active
        );
    }

    #[test]
    fn activation_stages_offsets_and_returns_to_first_person_after_exit() {
        let mut controller = CameraController::default();
        let mut settings = CameraSettings::default();
        let mut input = input();
        input.elapsed_seconds = 0.;
        let first = controller.update(&input, &mut settings, &mut EmptyWorld);
        assert_eq!(first.values.depth, 0.);
        assert_eq!(first.values.horizontal, 0.);
        assert!(first.hide_local_player);
        input.elapsed_seconds = 0.1;
        settle(&mut controller, &input, &mut settings);
        assert_eq!(controller.activation_blend(), 1.);
        input.command = Some(CameraCommand::Toggle);
        let leaving = controller.update(&input, &mut settings, &mut EmptyWorld);
        assert!(leaving.active && leaving.activation_transition);
        assert!(!settings.enabled);
        input.command = None;
        let mut finished = None;
        for _ in 0..10 {
            let output = controller.update(&input, &mut settings, &mut EmptyWorld);
            if output.requested_perspective == Some(Perspective::FirstPerson) {
                finished = Some(output);
                break;
            }
        }
        let finished = finished.expect("exit must complete");
        assert!(finished.solved && !finished.active && finished.hide_local_player);
    }

    #[test]
    fn spyglass_exits_without_disabling_saved_camera_then_resumes() {
        let mut controller = CameraController::default();
        let mut settings = CameraSettings::default();
        let mut input = input();
        settle(&mut controller, &input, &mut settings);
        input.using_spyglass = true;
        for _ in 0..10 {
            let output = controller.update(&input, &mut settings, &mut EmptyWorld);
            if let Some(perspective) = output.requested_perspective {
                input.perspective = perspective;
            }
        }
        assert_eq!(input.perspective, Perspective::FirstPerson);
        assert!(settings.enabled);
        assert!(controller.spyglass_takeover);
        input.using_spyglass = false;
        let restored = controller.update(&input, &mut settings, &mut EmptyWorld);
        assert_eq!(
            restored.requested_perspective,
            Some(Perspective::ThirdPersonBack)
        );
        assert!(restored.active && controller.activation_requested());
    }

    #[test]
    fn orbit_mouse_is_consumed_and_release_applies_current_and_previous_look() {
        let mut controller = CameraController::default();
        let mut settings = CameraSettings::default();
        let mut input = input();
        settle(&mut controller, &input, &mut settings);
        input.toggle_orbit = true;
        input.mouse_delta = Some([100., 1000.]);
        let orbit = controller.update(&input, &mut settings, &mut EmptyWorld);
        assert!(orbit.orbit_active && orbit.consume_mouse_delta);
        assert_eq!(
            orbit.rotation,
            LookAngles {
                yaw: 15.,
                pitch: 90.
            }
        );
        input.mouse_delta = None;
        let released = controller.update(&input, &mut settings, &mut EmptyWorld);
        assert!(!released.orbit_active);
        assert_eq!(
            released.apply_player_look,
            Some(LookAngles {
                yaw: 15.,
                pitch: 90.
            })
        );
        assert_eq!(released.rotation.yaw, 15.);
    }

    #[test]
    fn contextual_precedence_and_proximity_neutral_exclude_feedback() {
        let mut dynamic = DynamicCamera::default();
        dynamic.fence = Transition {
            blend: 1.,
            profile: Some(profile(1.)),
        };
        dynamic.proximity.blend = 1.;
        dynamic.proximity_profile = Some(profile(2.));
        assert_eq!(
            dynamic.contextual(CameraValues::default(), true).horizontal,
            2.
        );
        assert_eq!(
            dynamic
                .contextual(CameraValues::default(), false)
                .horizontal,
            1.
        );
        dynamic.climbing = Transition {
            blend: 1.,
            profile: Some(profile(3.)),
        };
        dynamic.elytra = Transition {
            blend: 1.,
            profile: Some(profile(4.)),
        };
        assert_eq!(
            dynamic.contextual(CameraValues::default(), true).horizontal,
            4.
        );
        dynamic.passenger = Transition {
            blend: 1.,
            profile: Some(profile(5.)),
        };
        dynamic.map = Transition {
            blend: 1.,
            profile: Some(profile(6.)),
        };
        dynamic.ranged = Transition {
            blend: 1.,
            profile: Some(profile(7.)),
        };
        assert_eq!(
            dynamic.contextual(CameraValues::default(), true).horizontal,
            7.
        );
    }

    #[test]
    fn fence_remains_latched_in_air_and_clears_on_other_ground() {
        let mut controller = CameraController::default();
        let mut settings = CameraSettings::default();
        let mut input = input();
        settle(&mut controller, &input, &mut settings);
        input.profiles.fence = Some(profile(0.));
        input.supported_by_fence = true;
        controller.update(&input, &mut settings, &mut EmptyWorld);
        assert!(controller.dynamic.fence_latched);
        input.supported_by_fence = false;
        controller.update(&input, &mut settings, &mut EmptyWorld);
        assert!(controller.dynamic.fence_latched);
        input.grounded = true;
        controller.update(&input, &mut settings, &mut EmptyWorld);
        assert!(!controller.dynamic.fence_latched);
    }

    #[test]
    fn side_ray_collision_is_instant_and_recovers_only_outwards() {
        struct SideWall;
        impl CameraWorld for SideWall {
            fn clip(&mut self, from: DVec3, to: DVec3, _: ClipKind) -> Option<BlockHit> {
                (from.x > 0.1).then_some(BlockHit {
                    position: from.lerp(to, 0.5),
                    block: IVec3::ZERO,
                    normal: IVec3::X,
                })
            }
        }
        let fraction = safe_travel_fraction(
            &mut SideWall,
            DVec3::ZERO,
            DVec3::new(0., 0., -2.),
            DVec3::X,
            DVec3::Y,
        );
        assert!((fraction - 0.46).abs() < 1e-10);
        let mut recovery = CollisionRecovery::default();
        assert_eq!(recovery.update(fraction, 0.), fraction);
        assert!(recovery.update(1., 0.05) > fraction);
        assert_eq!(recovery.update(0.1, 0.05), 0.1);
        recovery.reset();
        assert_eq!(recovery.fraction(), 1.);
    }

    #[test]
    fn visibility_skips_inside_body_and_fades_only_aligned_collisions() {
        let desired = DVec3::new(1.17, -0.11, -2.5);
        let sample = |actual, transition| {
            AvatarVisibility::sample(
                desired,
                actual,
                DVec3::X,
                DVec3::Y,
                0.6,
                1.8,
                1.62,
                transition,
            )
        };
        assert_eq!(sample(desired, false), AvatarVisibility::default());
        assert!(sample(DVec3::ZERO, false).skip_avatar);
        let close = sample(DVec3::new(0., 0., -0.6), false);
        assert!(close.alpha >= 0.15 && close.alpha < 1. && !close.skip_avatar);
        assert_eq!(sample(DVec3::new(0.4, 0., -0.6), false).alpha, 1.);
    }

    #[test]
    fn optical_ray_uses_camera_origin_not_near_plane_and_rejects_singular_matrix() {
        let projection = Mat4::perspective_rh_gl(70_f32.to_radians(), 1.5, 0.05, 1000.);
        let camera = DVec3::new(30_000_000., 64., -30_000_000.);
        let ray = center_aim_ray(camera, Mat4::IDENTITY, projection).unwrap();
        assert!(ray.origin.distance(camera) < 1e-6);
        assert!(ray.direction.distance(DVec3::NEG_Z) < 1e-6);
        assert!(center_aim_ray(camera, Mat4::ZERO, projection).is_none());
    }

    #[test]
    fn settings_roundtrip_includes_every_camera_toggle() {
        let mut settings = CameraSettings::default();
        settings.fence_camera = false;
        settings.map_camera = false;
        settings.spear_and_trident_camera_fix = false;
        settings.ladder_body_lock = false;
        let mut values = std::collections::BTreeMap::new();
        settings.write(|key, value| {
            values.insert(key.to_owned(), value);
        });
        let restored = CameraSettings::read(|key| values.get(key).cloned());
        assert!(!restored.fence_camera && !restored.map_camera);
        assert!(!restored.spear_and_trident_camera_fix && !restored.ladder_body_lock);
        assert_eq!(restored.values, settings.values);
    }

    #[test]
    fn incomplete_context_profile_fades_previous_profile_instead_of_snapping() {
        let mut transition = Transition::default();
        for _ in 0..30 {
            transition.update(true, Some(profile(0.)), 0.1);
        }
        let previous = transition.blend;
        transition.update(true, Some(CameraProfile::default()), 0.1);
        assert!(transition.blend > 0. && transition.blend < previous);
        assert_eq!(transition.profile.unwrap().horizontal, 0.);
    }

    #[test]
    fn map_latch_uses_player_pitch_and_is_suppressed_while_riding_without_profile() {
        let mut controller = CameraController::default();
        let mut settings = CameraSettings::default();
        let mut input = input();
        input.holding_filled_map = true;
        input.pitch = 43.;
        assert!(
            controller
                .update(&input, &mut settings, &mut EmptyWorld)
                .map_view_active
        );
        input.pitch = 35.;
        assert!(
            controller
                .update(&input, &mut settings, &mut EmptyWorld)
                .map_view_active
        );
        input.pitch = 31.;
        assert!(
            !controller
                .update(&input, &mut settings, &mut EmptyWorld)
                .map_view_active
        );
        input.pitch = 80.;
        input.riding = true;
        assert!(
            !controller
                .update(&input, &mut settings, &mut EmptyWorld)
                .map_view_active
        );
    }

    #[test]
    fn ranged_profile_uses_timed_throwable_before_held_but_bow_overrides_both() {
        let mut controller = CameraController::default();
        let mut settings = CameraSettings::default();
        let mut input = input();
        settle(&mut controller, &input, &mut settings);
        input.timed_throwable_pose = true;
        input.holding_throwable = true;
        input.profiles.timed_throwable = Some(profile(0.1));
        input.profiles.held_throwable = Some(profile(0.2));
        input.profiles.ranged = Some(profile(0.3));
        controller.update(&input, &mut settings, &mut EmptyWorld);
        assert_eq!(controller.dynamic.ranged.profile.unwrap().horizontal, 0.1);
        input.using_bow = true;
        controller.update(&input, &mut settings, &mut EmptyWorld);
        assert_eq!(controller.dynamic.ranged.profile.unwrap().horizontal, 0.3);
        settings.bow_and_crossbow_camera_fix = false;
        let previous = controller.dynamic.ranged.blend;
        controller.update(&input, &mut settings, &mut EmptyWorld);
        assert!(controller.dynamic.ranged.blend < previous);
    }

    #[test]
    fn proximity_uses_actual_eye_reach_and_body_aabb_distance() {
        struct TargetWorld;
        impl CameraWorld for TargetWorld {
            fn clip(&mut self, _: DVec3, _: DVec3, kind: ClipKind) -> Option<BlockHit> {
                (kind == ClipKind::Outline).then_some(BlockHit {
                    position: DVec3::new(0., 0., 2.),
                    block: IVec3::new(0, 0, 2),
                    normal: IVec3::NEG_Z,
                })
            }
        }
        let mut input = input();
        let (forward, right, up) = LookAngles { yaw: 0., pitch: 0. }.basis();
        let distance = proximity_sight_distance(
            &mut TargetWorld,
            &input,
            CameraValues::default(),
            forward,
            right,
            up,
        );
        assert!((distance - 1.7).abs() < 1e-10);
        // A swimming/rendered-eye adjustment must not extend actual-eye reach.
        input.pivot = DVec3::new(0., 0., 1.5);
        input.block_reach = 1.;
        assert!(
            proximity_sight_distance(
                &mut TargetWorld,
                &input,
                CameraValues::default(),
                forward,
                right,
                up,
            )
            .is_infinite()
        );
    }

    #[test]
    fn avatar_alpha_modifies_only_alpha_and_requests_translucent_material() {
        let visibility = AvatarVisibility {
            alpha: 0.5,
            skip_avatar: false,
        };
        assert_eq!(visibility.apply_argb(0xff12_3456), 0x8012_3456);
        assert!(visibility.is_translucent());
        assert!(!AvatarVisibility::default().is_translucent());
    }
}
