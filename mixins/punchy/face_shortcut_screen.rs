fn face_shortcut_screen(
    game: &crate::game::Game<'_>,
    key: &str,
    repeat: bool,
    canceled: &mut bool,
) -> core::ops::ControlFlow<()> {
    if crate::game::punchy_face_ui::shortcut(game, key, repeat) {
        *canceled = true;
        return core::ops::ControlFlow::Break(());
    }
    core::ops::ControlFlow::Continue(())
}
