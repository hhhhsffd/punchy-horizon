// FishingHook.DATA_BITING is not a held input: read the hook's actual state.
pub(in crate::game) fn biting(game: &crate::game::Game<'_>) -> bool {
    game.hook.as_ref().is_some_and(|hook| hook.nibble > 0)
}
