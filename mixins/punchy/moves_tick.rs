fn moves_tick(game: &mut crate::game::Game<'_>, value: ()) {
    crate::game::punchy_moves::tick(game);
    value
}
