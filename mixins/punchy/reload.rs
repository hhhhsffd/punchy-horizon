fn moves_reload(game: &mut crate::game::Game<'_>) {
    crate::game::punchy_face_ui::clear();
    crate::game::punchy_moves::clear();
    crate::game::punchy_faces::clear(game);
}
