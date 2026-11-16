fn punchy_face_editor_scroll(
    game: &mut crate::game::Game<'_>,
    amount: f32,
) -> core::ops::ControlFlow<Result<()>> {
    if crate::game::punchy_face_ui::scroll(game, amount) {
        core::ops::ControlFlow::Break(Ok(()))
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
