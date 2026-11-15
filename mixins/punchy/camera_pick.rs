fn moves_camera_pick(game: &crate::game::Game<'_>) -> core::ops::ControlFlow<Option<crate::game::Hit>> {
    match crate::game::punchy_camera::pick_block(game) {
        Some(hit) => core::ops::ControlFlow::Break(hit),
        None => core::ops::ControlFlow::Continue(()),
    }
}
