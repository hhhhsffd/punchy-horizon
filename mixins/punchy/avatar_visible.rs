fn moves_avatar_visible(game: &crate::game::Game<'_>) -> core::ops::ControlFlow<bool> {
    if crate::game::punchy_camera::hides_avatar(game) {
        core::ops::ControlFlow::Break(false)
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
