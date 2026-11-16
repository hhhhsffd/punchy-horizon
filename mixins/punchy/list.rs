fn list_punchy(game: &crate::game::Game<'_>, mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    mods.push(crate::game::native_hooks::NativeMod {
        id: "punchy".into(),
        title: crate::game::punchy::TITLE.into(),
        description: "Self-contained Rust animation preview. Animation data included.".into(),
        keys: vec![
            [
                "face_editor".into(),
                "Open face editor".into(),
                "key.keyboard.f6".into(),
            ],
            [
                "inspect".into(),
                "Inspect item".into(),
                "key.keyboard.f10".into(),
            ],
            [
                "reload".into(),
                "Reload animations".into(),
                "key.keyboard.f12".into(),
            ],
            [
                "moves_orbit".into(),
                "Moves orbit camera".into(),
                "key.keyboard.f4".into(),
            ],
            [
                "live_hud".into(),
                "Punchy live editor".into(),
                "key.keyboard.f9".into(),
            ],
            [
                "moves_emote".into(),
                "Play selected Moves emote".into(),
                "key.keyboard.n".into(),
            ],
            [
                "moves_next_emote".into(),
                "Next Moves emote".into(),
                "key.keyboard.m".into(),
            ],
        ],
    });
    if let Some(mod_entry) = mods.last_mut() {
        mod_entry.keys.push([
            "moves_camera_toggle".into(),
            "Toggle Moves shoulder camera".into(),
            "key.keyboard.v".into(),
        ]);
        mod_entry
            .keys
            .extend(crate::game::punchy_moves::keybindings(game));
    }
}
