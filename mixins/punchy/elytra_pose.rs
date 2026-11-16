fn moves_elytra_pose(
    game: &crate::game::Game<'_>,
    m: &horizon_client::mobs::Mob,
    chest: Option<&str>,
    camera: glam::DVec3,
    partial: f32,
    light: u32,
    out: &mut Vec<horizon_client::mesher::Vertex>,
) -> core::ops::ControlFlow<()> {
    let ours = m.emote.as_ref().is_some_and(|pose| {
        let any: &dyn std::any::Any = &**pose;
        any.is::<horizon_client::mobs::anim::punchy_moves::Body>()
    });
    if !ours || chest.is_none() {
        return core::ops::ControlFlow::Continue(());
    }
    let base = horizon_client::mobs::render::base_transform(m, partial, camera);
    let original = base;
    let Some(base) = horizon_client::mobs::render::punchy_face_layer::body_attachment(
        m,
        partial,
        base,
        game.res.mob_data(),
    ) else {
        return core::ops::ControlFlow::Continue(());
    };
    let stack = m.extra.armor[1]
        .as_ref()
        .filter(|s| chest == Some(&*s.item));
    horizon_client::mobs::render::punchy_face_layer::record_wings(m, partial, original, base);
    // Reuse the native wing renderer, equipment assets, CIT/emission/glint and
    // flight folding; only its anchor acquires our torso/root animation delta.
    game.wings_geometry(
        chest,
        stack,
        m.cape.as_deref(),
        false,
        base,
        m.extra.elytra.angles(partial),
        m.extra.crouching,
        light,
        game.wings_overlay(stack),
        out,
    );
    core::ops::ControlFlow::Break(())
}
