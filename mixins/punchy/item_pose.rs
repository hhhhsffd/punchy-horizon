fn moves_item_pose(
    m: &horizon_client::mobs::Mob,
    alpha: f32,
    camera: glam::DVec3,
    data: &horizon_client::mobs::MobData,
    left: bool,
) -> core::ops::ControlFlow<Option<glam::Mat4>> {
    horizon_client::mobs::anim::punchy_moves::item_transform(m, alpha, camera, data, left)
}
