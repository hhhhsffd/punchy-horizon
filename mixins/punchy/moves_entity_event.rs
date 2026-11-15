fn moves_entity_event(game: &crate::game::Game<'_>, id: u32, event: i8) {
    crate::game::punchy_moves::entity_event(game, id as i32, event as u8);
}
