fn accepted_use(
    game: &mut crate::game::Game<'_>,
    hand: horizon_client::first_person::Hand,
) -> core::ops::ControlFlow<bool> {
    let Some(_guard) = crate::game::punchy::capture_use(game) else {
        return core::ops::ControlFlow::Continue(());
    };
    let item = crate::game::punchy_face_observer::held_item(game, hand);
    let before = crate::game::punchy_moves::capture_use(game, hand);
    let success = game.remote_use_item_on(hand);
    crate::game::punchy_moves::finish_use(game, before, success);
    crate::game::punchy_face_observer::used(game, &item, success);
    crate::game::punchy::used(game, hand, success);
    core::ops::ControlFlow::Break(success)
}
