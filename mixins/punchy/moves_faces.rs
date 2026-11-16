// In-game facial animation. Each avatar has a separate animated atlas skin;
// source pixels are retained so eyelids never feed back into eye detection.
use super::punchy_face_observer::Observation;
use super::{Game, Hand};
use horizon_client::mobs::render::punchy_face_layer;
use horizon_client::mobs::{
    Mob,
    anim::punchy_moves::engine::moves_expressions::{FaceConditions, FaceEngine, FaceSettings},
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};

const ID: &str = "punchy";
const PREFIX: &str = "punchy/animated_face/";
const CAP: usize = 64;
std::thread_local! {
    static FACES: RefCell<Session> = RefCell::new(Session::default());
    static RESOLVING: Cell<bool> = const {Cell::new(false)};
}
#[derive(Default)]
struct Session {
    library: Option<
        std::sync::Arc<
            horizon_client::mobs::anim::punchy_moves::engine::moves_expressions::FaceLibrary,
        >,
    >,
    entries: BTreeMap<(bool, u32), Entry>,
    connection: Option<u64>,
}
struct Entry {
    source: String,
    alias: String,
    engine: FaceEngine,
    pixels: Vec<u8>,
    reaction: String,
    observation: Observation,
    marks: Option<horizon_client::mobs::anim::punchy_moves::engine::moves_expressions::FaceMarks>,
}
pub(super) struct ResolveGuard;
impl Drop for ResolveGuard {
    fn drop(&mut self) {
        RESOLVING.with(|s| s.set(false));
    }
}
pub(super) fn resolve_guard() -> Option<ResolveGuard> {
    if RESOLVING.with(|s| s.replace(true)) {
        None
    } else {
        Some(ResolveGuard)
    }
}
pub(super) fn enabled(game: &Game<'_>) -> bool {
    super::punchy_moves::enabled(game)
        && game.native_setting(ID, "moves_faces_enabled").as_deref() != Some("false")
}
fn settings(game: &Game<'_>, embedded: Option<&FaceSettings>) -> FaceSettings {
    let value = |key| game.native_setting(ID, key);
    let mut profile = embedded.cloned().unwrap_or_default().to_json();
    if let Some(overrides) =
        value("moves_face_profile").and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    {
        merge_profile(&mut profile, &overrides);
    }
    let mut settings = FaceSettings::from_json(&profile);
    for (key, flag) in [
        ("moves_face_blinking", &mut settings.blinking),
        ("moves_face_glancing", &mut settings.glancing),
        ("moves_face_brows", &mut settings.brows),
        ("moves_face_blush", &mut settings.blush),
        ("moves_face_others", &mut settings.on_other_players),
    ] {
        if let Some(value) = value(key).and_then(|v| v.parse::<bool>().ok()) {
            *flag = value;
        }
    }
    for (key, field) in [
        ("moves_face_blink_seconds", &mut settings.blink_seconds),
        ("moves_face_strength", &mut settings.expression_strength),
    ] {
        if let Some(value) = value(key)
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite())
        {
            *field = value;
        }
    }
    settings.normalize();
    settings
}
fn merge_profile(base: &mut serde_json::Value, overrides: &serde_json::Value) {
    if let (Some(base), Some(overrides)) = (base.as_object_mut(), overrides.as_object()) {
        for (key, value) in overrides {
            if value.is_object() && base.get(key).is_some_and(serde_json::Value::is_object) {
                merge_profile(base.get_mut(key).unwrap(), value);
            } else {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}
pub(super) fn clear(game: &mut Game<'_>) {
    super::punchy_face_observer::clear();
    punchy_face_layer::clear();
    FACES.with(|s| {
        let mut s = s.borrow_mut();
        for ((own, id), entry) in &s.entries {
            let model = if *own {
                game.player_model.as_mut()
            } else {
                game.mobs.iter_mut().find(|m| m.id == *id)
            };
            if let Some(model) = model
                && model.skin.as_deref() == Some(&entry.alias)
            {
                model.skin = Some(entry.source.clone());
            }
        }
        *s = Session::default();
    });
}
pub(super) fn status() -> String {
    FACES.with(|s| {
        let s = s.borrow();
        let own = s
            .entries
            .iter()
            .find(|((own, _), _)| *own)
            .map(|(_, entry)| entry);
        own.map_or_else(
            || "Face: waiting for player skin".into(),
            |entry| {
                if entry.engine.rig.eyes.is_empty() {
                    "Face: no eyes detected on current skin".into()
                } else {
                    format!(
                        "Face: {} eyes | {}",
                        entry.engine.rig.eyes.len(),
                        entry.reaction
                    )
                }
            },
        )
    })
}
pub(super) fn diagnostic(game: &Game<'_>) -> serde_json::Value {
    FACES.with(|s|{
        let s=s.borrow();
        let own=s.entries.iter().find(|((own,_),_)|*own).map(|(_,entry)|{
            serde_json::json!({
                "eyes":entry.engine.rig.eyes.len(),
                "animated":game.player_model.as_ref().is_some_and(|m|m.skin.as_deref()==Some(entry.alias.as_str())),
                "uploaded":uploaded(game,&entry.alias),
                "reaction":entry.reaction,
                "wing_delta":game.player_model.as_ref().and_then(punchy_face_layer::wing_diagnostic),
            })
        });
        serde_json::json!({"players":s.entries.len(),"own":own,
            "pointer":super::punchy_face_ui::pointer(),
            "editor":super::punchy_face_ui::diagnostic(),
            "library_warnings":s.library.as_ref().map(|library| &library.warnings)})
    })
}
pub(super) fn lights(game: &Game<'_>, out: &mut Vec<super::native_hooks::NativeColoredLight>) {
    if !enabled(game) {
        return;
    }
    let mut sources = Vec::new();
    if let Some(model) = &game.player_model {
        sources.extend(punchy_face_layer::lights(
            model,
            game.clock.camera_fraction() as f32,
            game.res.mob_data(),
        ));
    }
    for model in game.mobs.iter().filter(|m| !m.removed).take(CAP) {
        sources.extend(punchy_face_layer::lights(
            model,
            game.clock.fraction() as f32,
            game.res.mob_data(),
        ));
    }
    let origin = game.player.as_ref().map_or(glam::DVec3::ZERO, |p| {
        glam::DVec3::new(p.position.x, p.position.y, p.position.z)
    });
    sources.sort_by(|a, b| {
        glam::DVec3::from_array(a.0)
            .distance_squared(origin)
            .total_cmp(&glam::DVec3::from_array(b.0).distance_squared(origin))
    });
    for (at, rgb, radius, intensity) in sources.into_iter().take(8_usize.saturating_sub(out.len()))
    {
        out.push(super::native_hooks::NativeColoredLight {
            at,
            rgb,
            radius,
            intensity,
        });
    }
}
fn conditions(game: &Game<'_>, m: &Mob, own: bool, time: f32) -> FaceConditions {
    let mut c = FaceConditions {
        time_seconds: time,
        entity_id: m.id,
        own_player: own,
        hidden: m.is_invisible() || m.extra.spectator,
        health: m.health,
        opacity: 1.,
        head_yaw: (m.head_yaw - m.body_yaw + 180.).rem_euclid(360.) - 180.,
        head_pitch: m.head_pitch,
        moving: m.limb_speed > 0.01,
        sprinting: m.extra.sprinting,
        crouching: m.extra.crouching,
        crawling: m.extra.avatar_pose == horizon_client::pose::Pose::Swimming && !m.fluids.in_water,
        on_ground: m.on_ground,
        vertical_speed: ((m.position.y - m.previous.y) * 20.) as f32,
        fall_distance: m.fall_distance as f32,
        swimming: m.extra.avatar_pose == horizon_client::pose::Pose::Swimming && m.fluids.in_water,
        underwater: m.was_eye_in_water,
        in_water: m.fluids.in_water,
        in_lava: m.fluids.in_lava,
        gliding: m.extra.fall_flying,
        climbing: m.on_climbable,
        riding: m.extra.passenger,
        sleeping: m.extra.avatar_pose == horizon_client::pose::Pose::Sleeping,
        dead: !m.alive(),
        hurt_seconds: if m.hurt_time > 0 {
            ((10 - m.hurt_time).max(0) as f32 + (time * 20.).fract()) / 20.
        } else {
            f32::INFINITY
        },
        air: (m.air as f32 / m.max_air_supply().max(1) as f32).clamp(0., 1.),
        attack: m.anim.attack_anim,
        main_arm: usize::from(m.extra.main_hand_left),
        night: game.dark_outside(),
        ..Default::default()
    };
    let main = m.extra.main_hand.as_deref().unwrap_or("");
    let using = m.extra.using_item > 0;
    let used = if m.extra.using_off_hand {
        m.extra.off_hand.as_deref().unwrap_or("")
    } else {
        main
    };
    c.aiming = using
        && matches!(
            used,
            "minecraft:bow" | "minecraft:crossbow" | "minecraft:trident"
        );
    c.blocking = using && used == "minecraft:shield";
    c.gliding_speed =
        ((m.velocity.x * m.velocity.x + m.velocity.y * m.velocity.y + m.velocity.z * m.velocity.z)
            .sqrt()
            * 20.) as f32;
    if own && let Some(p) = &game.player {
        c.health = p.health.health();
        c.hunger = p.health.food.food as f32;
        c.on_ground = p.ground.on_ground;
        c.moving = p.applied_input.iter().any(|v| v.abs() > 0.01);
        c.forward = p.applied_input[1];
        c.strafe = p.applied_input[0];
        c.flying = p.flying;
        c.dead = p.health.dead_or_dying();
        c.sleeping = game.sleeping.is_some();
        c.sprinting = p.sprinting;
        c.crouching = p.shift_key_down;
        c.underwater = p.fluids.was_eye_in_water;
        c.in_water = p.fluids.was_touching_water;
        c.in_lava = p.is_in_lava();
        c.vertical_speed = ((p.position.y - p.old_position.y) * 20.) as f32;
        c.gliding_speed = (p.velocity.length() * 20.) as f32;
        c.climbing = p.on_climbable;
        c.fall_distance = p.fall_distance as f32;
        c.hurt_seconds = if p.health.hurt_animation_time > 0 {
            ((10 - p.health.hurt_animation_time).max(0) as f32 + (time * 20.).fract()) / 20.
        } else {
            f32::INFINITY
        };
        let stack = game.used_hand().and_then(|hand| {
            if hand == Hand::Main {
                game.held()
            } else {
                game.offhand.as_ref()
            }
        });
        if let Some(stack) = stack {
            let components = game
                .pack
                .item_renderer
                .models
                .components(&horizon_client::item_render::stack_view(stack));
            let mut animation = components.get("minecraft:consumable").map_or(
                horizon_world::consumable::UseAnimation::None,
                horizon_client::first_person::consumable_animation,
            );
            game.native_use_animation(stack, &mut animation);
            c.eating = matches!(animation, horizon_world::consumable::UseAnimation::Eat);
            c.drinking = matches!(animation, horizon_world::consumable::UseAnimation::Drink);
        }
    }
    super::punchy_moves::face_signals(own, m.id, &mut c);
    if own
        && let Some(forced) = game
            .native_setting(ID, "moves_face_expression")
            .filter(|s| s != "auto")
    {
        c.forced_look = Some(forced);
    }
    c
}
/// Runs after Horizon has chosen the current skin, including inventory avatars.
pub(super) fn frame(game: &mut Game<'_>) {
    if !game.native_window() {
        return;
    }
    if !enabled(game) || game.player.is_none() {
        clear(game);
        return;
    }
    use std::hash::{Hash, Hasher};
    let mut world = std::collections::hash_map::DefaultHasher::new();
    game.started.hash(&mut world);
    game.dim.id().hash(&mut world);
    game.remote
        .as_ref()
        .map(|r| r.session.serial())
        .hash(&mut world);
    let connection = world.finish();
    if FACES.with(|s| s.borrow().connection.is_some_and(|c| c != connection)) {
        clear(game);
    }
    let preferences = settings(game, None);
    if !uploaded(game, punchy_face_layer::WHITE) {
        game.store_skin(punchy_face_layer::WHITE.into(), [255_u8; 4].repeat(64 * 64));
    }
    game.skin_downloads
        .borrow_mut()
        .drawn(punchy_face_layer::WHITE);
    let mut players = Vec::new();
    if let Some(m) = game.player_model.as_ref() {
        players.push((
            true,
            m.id,
            m.skin.clone().unwrap_or_else(|| m.kind.texture.to_string()),
            conditions(
                game,
                m,
                true,
                (m.age as f32 + game.clock.camera_fraction() as f32) / 20.,
            ),
        ));
    }
    for m in game
        .mobs
        .iter()
        .filter(|m| !m.removed && matches!(&*m.kind.id, "minecraft:player" | "minecraft:mannequin"))
        .take(CAP - 1)
    {
        players.push((
            false,
            m.id,
            m.skin.clone().unwrap_or_else(|| m.kind.texture.to_string()),
            conditions(
                game,
                m,
                false,
                (m.age as f32 + game.clock.fraction() as f32) / 20.,
            ),
        ));
    }
    let mut replacements = Vec::new();
    let mut restorations = Vec::new();
    FACES.with(|s| {
        let mut s = s.borrow_mut();
        s.connection = Some(connection);
        if s.library.is_none() {
            s.library=Some(std::sync::Arc::new(horizon_client::mobs::anim::punchy_moves::engine::moves_expressions::FaceLibrary::load(&game.options.resources)));
        }
        let library=s.library.as_ref().unwrap().clone();
        for (own, id, source, c) in &mut players {
            let key = (*own, *id);
            let source = if source.starts_with(PREFIX) {
                s.entries
                    .get(&key)
                    .map_or_else(|| source.clone(), |e| e.source.clone())
            } else {
                source.clone()
            };
            if s.entries.get(&key).is_none_or(|e| e.source != source) {
                let Some(pixels) = game.native_client_texture(&source) else {
                    continue;
                };
                if pixels.width != 64 || pixels.height != 64 {
                    continue;
                }
                let Ok(engine) = FaceEngine::new(
                    &pixels.rgba,
                    pixels.width,
                    pixels.height,
                    preferences.clone(),
                ) else {
                    continue;
                };
                s.entries.insert(
                    key,
                    Entry {
                        source,
                        alias: format!("{PREFIX}{}_{id}", if *own { "local" } else { "remote" }),
                        engine,
                        pixels: Vec::new(),
                        reaction: String::new(),
                        observation: Observation::default(),
                        marks: None,
                    },
                );
            }
            let entry = s.entries.get_mut(&key).unwrap();
            if *own {
                let marks = game.native_setting(ID, "moves_face_marks");
                let marks = marks.as_deref().filter(|v| !v.is_empty()).map(
                    horizon_client::mobs::anim::punchy_moves::engine::moves_expressions::FaceMarks::parse,
                );
                if entry.marks != marks {
                    if marks.is_some() { entry.engine.set_marks(marks.as_ref()); }
                    else { entry.engine.restore_embedded_marks(); }
                    entry.marks = marks;
                }
            }
            if !*own && !entry.engine.marked && !preferences.on_other_players {
                punchy_face_layer::remove(&entry.alias);
                restorations.push((key, entry.alias.clone(), entry.source.clone()));
                continue;
            }
            // Explicit local preferences take precedence over embedded skin style.
            let mut style = if *own {
                settings(game, entry.engine.embedded_settings.as_ref())
            } else if let Some(embedded) = &entry.engine.embedded_settings {
                // Marked remote skins retain their owner's appearance.
                let mut style = embedded.clone();
                style.on_other_players = preferences.on_other_players;
                style
            } else {
                preferences.clone()
            };
            library.apply(&mut style);
            entry.engine.set_settings(style);
            let model = if *own {
                game.player_model.as_ref()
            } else {
                game.mobs.iter().find(|m| m.id == *id)
            };
            if let Some(model) = model {
                entry.observation.update(game, model, *own, c);
            }
            c.quads = true;
            let output = entry.engine.render(c);
            entry.reaction = output.frame.reaction.clone();
            if output.detected
                && entry.engine.settings.enabled
                && !c.hidden
                && (c.own_player || entry.engine.settings.on_other_players || entry.engine.marked)
            {
                punchy_face_layer::set(&entry.alias, &output);
                if output.geometry_skin != entry.pixels || !uploaded(game, &entry.alias) {
                    game.store_skin(entry.alias.clone(), output.geometry_skin.clone());
                    if uploaded(game, &entry.alias) {
                        entry.pixels = output.geometry_skin;
                    }
                }
                if uploaded(game, &entry.alias) {
                    replacements.push((key, entry.alias.clone()));
                } else {
                    punchy_face_layer::remove(&entry.alias);
                    restorations.push((key, entry.alias.clone(), entry.source.clone()));
                }
            } else {
                punchy_face_layer::remove(&entry.alias);
                restorations.push((key, entry.alias.clone(), entry.source.clone()));
            }
        }
        s.entries.retain(|key, entry| {
            let keep = players.iter().any(|(own, id, _, _)| key == &(*own, *id));
            if !keep {
                punchy_face_layer::remove(&entry.alias);
                restorations.push((*key, entry.alias.clone(), entry.source.clone()));
            }
            keep
        });
    });
    for ((own, id), alias, source) in restorations {
        let model = if own {
            game.player_model.as_mut()
        } else {
            game.mobs.iter_mut().find(|m| m.id == id)
        };
        if let Some(model) = model
            && model.skin.as_deref() == Some(&alias)
        {
            model.skin = Some(source);
        }
    }
    for ((own, id), alias) in replacements {
        game.skin_downloads.borrow_mut().drawn(&alias);
        let model = if own {
            game.player_model.as_mut()
        } else {
            game.mobs.iter_mut().find(|m| m.id == id)
        };
        if let Some(model) = model {
            model.skin = Some(alias);
        }
    }
}
fn uploaded(game: &Game<'_>, alias: &str) -> bool {
    game.skin_slots.contains(alias) && game.gui.atlas.sprites.contains_key(alias)
}

pub(super) fn editor(
    game: &Game<'_>,
) -> Result<horizon_client::mobs::anim::punchy_moves::engine::moves_expressions::FaceEditor, String>
{
    use horizon_client::mobs::anim::punchy_moves::engine::moves_expressions::{
        FaceEditor, FaceMarks,
    };
    FACES.with(|s| {
        let s = s.borrow();
        if let Some((_, entry)) = s.entries.iter().find(|((own, _), _)| *own) {
            return FaceEditor::from_engine(&entry.engine);
        }
        let model = game
            .player_model
            .as_ref()
            .ok_or("Enter a world before opening the face editor")?;
        let source = model.skin.as_deref().unwrap_or(&model.kind.texture);
        let pixels = game
            .native_client_texture(source)
            .ok_or("Waiting for player skin; try again after it loads")?;
        let mut editor = FaceEditor::new(
            &pixels.rgba,
            pixels.width,
            pixels.height,
            settings(game, None),
        )?;
        if let Some(marks) = game
            .native_setting(ID, "moves_face_marks")
            .filter(|s| !s.is_empty())
        {
            editor.engine.set_marks(Some(&FaceMarks::parse(&marks)));
            editor = FaceEditor::from_engine(&editor.engine)?;
        }
        Ok(editor)
    })
}

const TOGGLES: [(&str, &str, bool); 6] = [
    ("moves_faces_enabled", "Facial animation", true),
    ("moves_face_blinking", "Face blinking", true),
    ("moves_face_glancing", "Face eye glances", true),
    ("moves_face_brows", "Face animated brows", true),
    ("moves_face_blush", "Face blush and reactions", true),
    (
        "moves_face_others",
        "Face animation on other players",
        false,
    ),
];
const LOOKS: [&str; 10] = [
    "auto", "neutral", "happy", "focused", "angry", "tired", "shut", "love", "scared", "excited",
];
pub(super) fn settings_buttons(game: &Game<'_>, buttons: &mut Vec<String>) {
    for (key, label, default) in TOGGLES {
        let on = game
            .native_setting(ID, key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default);
        buttons.push(format!("{label}: {}", if on { "ON" } else { "OFF" }));
    }
    let settings = settings(game, None);
    buttons.push(format!(
        "Face blink interval: {:.2}s (left +0.25 / right -0.25)",
        settings.blink_seconds
    ));
    buttons.push(format!(
        "Face expression strength: {:.2} (left +0.05 / right -0.05)",
        settings.expression_strength
    ));
    buttons.push(format!(
        "Face expression: {}",
        game.native_setting(ID, "moves_face_expression")
            .unwrap_or_else(|| "auto".into())
    ));
    buttons.push(format!("Open face editor | {}", status()));
}
pub(super) fn settings_click(game: &Game<'_>, index: usize, back: bool) {
    if let Some(&(key, _, default)) = TOGGLES.get(index) {
        let on = game
            .native_setting(ID, key)
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(default);
        game.native_set_setting(ID, key, if on { "false" } else { "true" });
    } else if index == 6 || index == 7 {
        let settings = settings(game, None);
        let (key, old, step, min, max) = if index == 6 {
            (
                "moves_face_blink_seconds",
                settings.blink_seconds,
                0.25,
                1.5,
                8.,
            )
        } else {
            (
                "moves_face_strength",
                settings.expression_strength,
                0.05,
                0.25,
                1.5,
            )
        };
        game.native_set_setting(
            ID,
            key,
            &format!(
                "{:.2}",
                (old + if back { -step } else { step }).clamp(min, max)
            ),
        );
    } else if index == 8 {
        let old = game
            .native_setting(ID, "moves_face_expression")
            .unwrap_or_else(|| "auto".into());
        let index = LOOKS.iter().position(|s| *s == old).unwrap_or(0);
        let next = if back {
            (index + LOOKS.len() - 1) % LOOKS.len()
        } else {
            (index + 1) % LOOKS.len()
        };
        game.native_set_setting(ID, "moves_face_expression", LOOKS[next]);
    } else if index == 9 && !back {
        super::punchy_face_ui::open(game);
    }
}
pub(super) fn reset_settings(game: &Game<'_>) {
    for (key, _, default) in TOGGLES {
        game.native_set_setting(ID, key, if default { "true" } else { "false" });
    }
    for (key, value) in [
        ("moves_face_profile", "{}"),
        ("moves_face_marks", ""),
        ("moves_face_expression", "auto"),
        ("moves_face_blink_seconds", "3.6"),
        ("moves_face_strength", "1"),
    ] {
        game.native_set_setting(ID, key, value);
    }
}
