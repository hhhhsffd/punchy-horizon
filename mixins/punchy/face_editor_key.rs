fn punchy_face_editor_key(
    game: &mut crate::game::Game<'_>,
    key: winit::keyboard::KeyCode,
) -> core::ops::ControlFlow<Result<()>> {
    if crate::game::punchy_face_ui::key(game, key) {
        core::ops::ControlFlow::Break(Ok(()))
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
