fn moves_faces_lights(
    game: &crate::game::Game<'_>,
    lights: &mut Vec<crate::game::native_hooks::NativeColoredLight>,
) {
    crate::game::punchy_faces::lights(game, lights);
}
