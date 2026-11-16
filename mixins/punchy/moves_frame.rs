fn moves_frame(game: &mut crate::game::Game<'_>) {
    crate::game::punchy_face_ui::flush_shortcut(game);
    crate::game::punchy_moves::frame(game);
    crate::game::punchy_faces::frame(game);
}
