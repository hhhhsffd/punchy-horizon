fn moves_camera_pick_entity(
    game: &crate::game::Game<'_>,
) -> core::ops::ControlFlow<Option<(f64, Option<i32>)>> {
    match crate::game::punchy_camera::pick_entity(game) {
        Some(hit) => core::ops::ControlFlow::Break(hit),
        None => core::ops::ControlFlow::Continue(()),
    }
}
