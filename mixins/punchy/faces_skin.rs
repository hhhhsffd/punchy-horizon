fn moves_faces_skin(game: &mut crate::game::Game<'_>) -> core::ops::ControlFlow<()> {
    let Some(_guard)=crate::game::punchy_faces::resolve_guard() else {return core::ops::ControlFlow::Continue(());};
    game.update_remote_skins();
    crate::game::punchy_faces::frame(game);
    core::ops::ControlFlow::Break(())
}
