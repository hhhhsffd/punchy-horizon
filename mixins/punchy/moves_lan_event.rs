fn moves_lan_event(game: &crate::game::Game<'_>, id: i32, event: i8) {
    crate::game::punchy_moves::entity_event(game, id, event as u8);
}
