fn moves_faces_emission(
    m: &horizon_client::mobs::Mob,
    alpha: f32,
    base: glam::Mat4,
    atlas: horizon_client::mobs::render::Atlas<'_>,
    light: u32,
    data: &horizon_client::mobs::MobData,
    extra: &[(&horizon_client::mobs::Part, [f32; 4], u32, u32, bool)],
    body_visible: bool,
    out: &mut Vec<horizon_client::mesher::Vertex>,
) -> core::ops::ControlFlow<()> {
    let Some(_guard) = horizon_client::mobs::render::punchy_face_layer::begin(m, body_visible)
    else {
        return core::ops::ControlFlow::Continue(());
    };
    // HEAD re-entry is required by the pinned transformer: render_visible
    // contains closures and ?, which it cannot instrument with RETURN.
    // An avatar with fractional face geometry takes the native CPU draw path
    // so skin, face and full-bright pieces share ordering. Other avatars retain
    // GPU skinning.
    horizon_client::mobs::render::render_visible(
        m,
        alpha,
        base,
        atlas,
        light,
        data,
        extra,
        body_visible,
        out,
        None,
    );
    horizon_client::mobs::render::punchy_face_layer::render(
        m,
        alpha,
        base,
        atlas,
        light,
        data,
        body_visible,
        out,
    );
    core::ops::ControlFlow::Break(())
}
