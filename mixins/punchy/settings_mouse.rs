fn punchy_settings_mouse(
    game: &mut crate::game::Game<'_>,
    pressed: bool,
    right: bool,
) -> core::ops::ControlFlow<Result<()>> {
    use horizon_client::options_ui::Page;
    if crate::game::punchy_face_ui::mouse(game, pressed, right) {
        return core::ops::ControlFlow::Break(Ok(()));
    }
    if !right || !pressed || game.ui.options.punchy_binding_active() {
        return core::ops::ControlFlow::Continue(());
    }
    let Page::NativeMod(index) = game.ui.options.page() else {
        return core::ops::ControlFlow::Continue(());
    };
    if game
        .settings
        .mods
        .installed
        .get(index)
        .is_none_or(|m| m.id != "punchy")
    {
        return core::ops::ControlFlow::Continue(());
    }
    let size = game.gui_size();
    if let Some(button) = game
        .ui
        .options
        .punchy_setting_at(&game.settings, size, game.cursor)
    {
        crate::game::punchy::click_direction(game, button, true);
        game.native_settings_refresh(index);
        core::ops::ControlFlow::Break(Ok(()))
    } else {
        let events = game.ui.options.back(&mut game.settings);
        core::ops::ControlFlow::Break(game.handle_option_events(events))
    }
}
