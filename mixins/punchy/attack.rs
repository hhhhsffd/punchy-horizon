fn moves_accepted_attack(game: &mut crate::game::Game<'_>) -> core::ops::ControlFlow<bool> {
    std::thread_local! {
        static BASE: std::cell::Cell<bool> = const {std::cell::Cell::new(false)};
    }
    if !game.native_window()
        || !crate::game::punchy_moves::enabled(game)
        || BASE.with(|flag| flag.replace(true))
    {
        return core::ops::ControlFlow::Continue(());
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            BASE.with(|flag| flag.set(false));
        }
    }
    let _guard = Guard;
    let before = crate::game::punchy_moves::capture_attack(game);
    let result = game.remote_start_attack();
    crate::game::punchy_moves::finish_attack(game, before);
    core::ops::ControlFlow::Break(result)
}
