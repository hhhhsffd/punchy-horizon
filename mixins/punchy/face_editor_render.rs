fn punchy_face_editor_render(
    game: &crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    value: (),
) {
    let _ = value;
    crate::game::punchy_face_ui::render(game, batch);
}
