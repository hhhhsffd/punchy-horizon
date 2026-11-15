// Camera-only settings. Indices are relative to the camera section.
use super::{Game, punchy_camera::settings_changed};
use horizon_client::mobs::anim::punchy_moves::engine::moves_camera as camera;

const FLAGS: [(&str, &str); 8] = [
    ("moves_camera_fence", "Fence camera"),
    ("moves_camera_ladder", "Ladder camera"),
    ("moves_camera_proximity", "Proximity camera"),
    ("moves_camera_elytra", "Elytra camera"),
    ("moves_camera_map", "Map camera"),
    ("moves_camera_bow_and_crossbow_fix", "Bow / crossbow camera"),
    (
        "moves_camera_spear_and_trident_fix",
        "Spear / trident camera",
    ),
    ("moves_camera_ladder_body_lock", "Ladder body lock"),
];
const PRESETS: [(&str, f32, f32, f32, f32); 4] = [
    ("Moves default", 1.17, -0.11, 2.5, 70.),
    ("Left shoulder", -1.17, -0.11, 2.5, 70.),
    ("Centered", 0., 0.2, 3.5, 70.),
    ("Wide", 1.17, 0.2, 4.5, 80.),
];

pub(crate) fn settings_buttons(game: &Game<'_>, buttons: &mut Vec<String>) {
    for (key, label) in FLAGS {
        let on = game.native_setting("punchy", key).as_deref() != Some("false");
        buttons.push(format!("{label}: {}", if on { "ON" } else { "OFF" }));
    }
    let preset = game
        .native_setting("punchy", "moves_camera_preset")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        % PRESETS.len();
    buttons.push(format!(
        "Apply camera preset: {} (left next / right previous)",
        PRESETS[preset].0
    ));
    for (key, label) in [
        ("moves_camera_map_enter", "Map look-down enter"),
        ("moves_camera_map_exit", "Map look-down exit"),
    ] {
        let value = game
            .native_setting("punchy", key)
            .filter(|v| v.parse::<f32>().is_ok_and(f32::is_finite))
            .map_or("pack-defined".into(), |v| format!("{v}°"));
        buttons.push(format!("{label}: {value} (left +1 / right -1)"));
    }
    buttons.push("Reset complete camera settings".into());
    buttons.push("Swap shoulder".into());
}

pub(crate) fn reset_settings(game: &Game<'_>) {
    camera::CameraSettings::default()
        .write(|key, value| game.native_set_setting("punchy", key, &value));
    for key in [
        "moves_camera_map_enter",
        "moves_camera_map_exit",
        "moves_camera_preset",
    ] {
        game.native_set_setting("punchy", key, "");
    }
    settings_changed(game);
}
pub(crate) fn settings_click(game: &Game<'_>, button: usize, back: bool) {
    let mut settings = camera::CameraSettings::read(|key| game.native_setting("punchy", key));
    match button {
        0..=7 => {
            let key = FLAGS[button].0;
            let on = game.native_setting("punchy", key).as_deref() != Some("false");
            game.native_set_setting("punchy", key, if on { "false" } else { "true" });
        }
        8 => {
            let index = game
                .native_setting("punchy", "moves_camera_preset")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(PRESETS.len() - 1);
            let index =
                (index % PRESETS.len() + if back { PRESETS.len() - 1 } else { 1 }) % PRESETS.len();
            let (_, horizontal, vertical, depth, fov) = PRESETS[index];
            settings.values = camera::CameraValues {
                horizontal,
                vertical,
                depth,
                fov,
            };
            settings.rotation_yaw = 0.;
            settings.rotation_pitch = 0.;
            settings.write(|key, value| game.native_set_setting("punchy", key, &value));
            game.native_set_setting("punchy", "moves_camera_preset", &index.to_string());
        }
        9..=10 => {
            let (key, fallback) = if button == 9 {
                ("moves_camera_map_enter", 42.)
            } else {
                ("moves_camera_map_exit", 32.)
            };
            let value = game
                .native_setting("punchy", key)
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| v.is_finite())
                .unwrap_or(fallback);
            game.native_set_setting(
                "punchy",
                key,
                &(value + if back { -1. } else { 1. })
                    .clamp(-90., 90.)
                    .to_string(),
            );
        }
        11 if !back => {
            reset_settings(game);
        }
        12 => {
            settings.values.horizontal = -settings.values.horizontal;
            settings.write(|key, value| game.native_set_setting("punchy", key, &value));
        }
        _ => {}
    }
    settings_changed(game);
}
