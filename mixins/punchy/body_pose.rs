// Moves animation adapter using Horizon's shared body-pose interface. Armor,
// sleeves, held items and the normal skin renderer retain their native paths.
use glam::{EulerRot, Mat4, Quat, Vec3};
pub use punchy_engine as engine;
use punchy_engine::moves_animation::Frame;

#[derive(Clone, Debug)]
pub struct Body {
    pub frame: Frame,
    pub age: f32,
}
// Apply to the shared root transform, not individual limbs: armor, held items,
// capes and face layers must all use the same swimming coordinate system.
std::thread_local! {
    static SWIM_BASE: std::cell::Cell<bool> = const {std::cell::Cell::new(false)};
}
pub fn base_transform(m: &super::Mob, alpha: f32, origin: Mat4) -> core::ops::ControlFlow<Mat4> {
    let ours = m.emote.as_ref().is_some_and(|pose| {
        let any: &dyn std::any::Any = &**pose;
        any.downcast_ref::<Body>()
            .is_some_and(|body| body.frame.suppress_vanilla_swim)
    });
    if !ours || m.extra.swim.amount(alpha) <= 0. || SWIM_BASE.with(|s| s.replace(true)) {
        return core::ops::ControlFlow::Continue(());
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            SWIM_BASE.with(|s| s.set(false));
        }
    }
    let _guard = Guard;
    // Never mutate the live entity (including its physics/eye-height state).
    let mut render = m.clone();
    render.extra.swim = Default::default();
    core::ops::ControlFlow::Break(super::super::render::base_transform_at(
        &render, alpha, origin,
    ))
}
impl horizon_hooks::players::BodyPose for Body {
    fn pose(&self, age: f32, rest: &[[f32; 9]; 6], poses: &mut [[f32; 9]; 6]) -> bool {
        let partial = (age - self.age).clamp(0., 1.);
        for (index, name) in horizon_hooks::players::BODY_PARTS.iter().enumerate() {
            let parent = self.frame.parent_matrix(name, partial);
            let mut matrix = Mat4::from_scale_rotation_translation(
                Vec3::new(poses[index][6], poses[index][7], poses[index][8]),
                Quat::from_euler(
                    EulerRot::ZYX,
                    poses[index][5],
                    poses[index][4],
                    poses[index][3],
                ),
                Vec3::new(poses[index][0], poses[index][1], poses[index][2]),
            );
            let bind = rest[index];
            let bind_rotation = Quat::from_euler(EulerRot::ZYX, bind[5], bind[4], bind[3]);
            let current = poses[index];
            let current_rotation =
                Quat::from_euler(EulerRot::ZYX, current[5], current[4], current[3]);
            let relative = (bind_rotation.inverse() * current_rotation).normalize();
            let (z, y, x) = relative.to_euler(EulerRot::ZYX);
            let bind_scale = Vec3::from_slice(&bind[6..9]).max(Vec3::splat(0.00001));
            let native = punchy_engine::Pose {
                position: Vec3::from_slice(&current[..3]) - Vec3::from_slice(&bind[..3]),
                rotation: relative,
                euler: Vec3::new(x, y, z).map(f32::to_degrees),
                scale: Vec3::from_slice(&current[6..9]) / bind_scale,
            };
            if let Some(delta) = self.frame.compose(name, partial, native) {
                matrix = Mat4::from_scale_rotation_translation(
                    delta.scale * bind_scale,
                    bind_rotation * delta.rotation,
                    Vec3::from_slice(&bind[..3]) + delta.position,
                );
            }
            let (scale, rotation, translation) = (parent * matrix).to_scale_rotation_translation();
            let (z, y, x) = rotation.to_euler(EulerRot::ZYX);
            if scale.is_finite() && rotation.is_finite() && translation.is_finite() {
                poses[index] = [
                    translation.x,
                    translation.y,
                    translation.z,
                    x,
                    y,
                    z,
                    scale.x,
                    scale.y,
                    scale.z,
                ];
            }
        }
        !self.frame.layers.is_empty()
    }
}

pub fn item(m: &super::Mob, alpha: f32, left: bool, value: Option<Mat4>) -> Option<Mat4> {
    let Some(body) = m.emote.as_ref().and_then(|p| {
        let any: &dyn std::any::Any = &**p;
        any.downcast_ref::<Body>()
    }) else {
        return value;
    };
    let name = if left { "left_item" } else { "right_item" };
    let partial = (m.age as f32 + alpha - body.age).clamp(0., 1.);
    let Some(pose) = body
        .frame
        .compose(name, partial, punchy_engine::Pose::default())
    else {
        return value;
    };
    value.map(|matrix| {
        matrix
            * punchy_engine::moves_render::item_matrix(
                pose.position * Vec3::new(1., -1., 1.),
                pose.euler,
                pose.scale,
            )
    })
}

// The pinned transformer cannot rewrite returns inside this renderer's `?`
// expressions. Re-enter its preserved vanilla path once, protected by an unwind
// guard, then apply our attachment delta. Every other model takes the direct path.
std::thread_local! { static ITEM_BASE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
pub fn item_transform(
    m: &super::Mob,
    alpha: f32,
    camera: glam::DVec3,
    data: &horizon_client::mobs::MobData,
    left: bool,
) -> core::ops::ControlFlow<Option<Mat4>> {
    let ours = m.emote.as_ref().is_some_and(|p| {
        let any: &dyn std::any::Any = &**p;
        any.is::<Body>()
    });
    if !ours || ITEM_BASE.with(|s| s.replace(true)) {
        return core::ops::ControlFlow::Continue(());
    }
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ITEM_BASE.with(|s| s.set(false));
        }
    }
    let _reset = Reset;
    let value = horizon_client::mobs::render::held_item_transform(m, alpha, camera, data, left);
    core::ops::ControlFlow::Break(item(m, alpha, left, value))
}
