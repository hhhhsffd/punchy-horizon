fn moves_swim_transform(
    m: &horizon_client::mobs::Mob,
    alpha: f32,
    origin: glam::Mat4,
) -> core::ops::ControlFlow<glam::Mat4> {
    horizon_client::mobs::anim::punchy_moves::base_transform(m, alpha, origin)
}
