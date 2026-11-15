use horizon_client::mobs::anim::punchy_moves::engine as punchy_engine;
// Native game signals; a separate Moves switch never disables first-person Punchy.
use super::*;
use punchy_engine::{
    moves_animation::{Controller, Input, Use},
    moves_pack::Pack,
};
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};
const ID: &str = "punchy";
thread_local! { static MOVES: RefCell<Session> = RefCell::new(Session::default()); }
#[derive(Default)]
struct Session {
    pack: Arc<Pack>,
    loaded: bool,
    controller: Controller,
    used: Option<usize>,
    hurt: i32,
    armor: Vec<String>,
    selected_emote: usize,
    emote_face: String,
    camera_claimed: bool,
    remote: BTreeMap<u32, RemoteState>,
    events: Pending,
    previous: Option<Input>,
}
#[derive(Default)]
struct RemoteState {
    controller: Controller,
    swing: f32,
    hurt: i32,
    previous: Option<Input>,
    events: Pending,
}
#[derive(Default)]
struct Pending {
    used: Option<AcceptedUse>,
    attacks: u8,
    attack_strength: Option<f32>,
    shield_hit: Option<usize>,
    shield_broken: Option<usize>,
    swapped: bool,
}
struct AcceptedUse {
    hand: usize,
    item: String,
    event: &'static str,
    crossbow: bool,
    firework: bool,
}
pub(super) fn enabled(game: &Game<'_>) -> bool {
    game.native_on(ID)
        && game.native_setting(ID, "enabled").as_deref() != Some("false")
        && game.native_setting(ID, "moves_enabled").as_deref() != Some("false")
}
pub(super) fn clear() {
    super::punchy_camera::reset();
    MOVES.with(|s| *s.borrow_mut() = Session::default());
}
pub(super) fn load(game: &Game<'_>) {
    MOVES.with(|s| {
        let mut s = s.borrow_mut();
        if s.loaded {
            return;
        }
        s.loaded = true;
        match Pack::load(&game.options.resources) {
            Ok(pack) => s.pack = Arc::new(pack),
            Err(e) => {
                s.pack = Arc::new(Pack {
                    warnings: vec![e],
                    ..Default::default()
                })
            }
        }
    });
}
pub(super) fn pack(game: &Game<'_>) -> Arc<Pack> {
    load(game);
    MOVES.with(|s| s.borrow().pack.clone())
}
pub(super) fn status(game: &Game<'_>) -> String {
    load(game);
    MOVES.with(|s| {
        let s = s.borrow();
        if let Some(error) = s.pack.warnings.first() {
            format!("Moves pack warning: {error}")
        } else if s.pack.clips.is_empty() {
            "Moves: enable Fluid & Foolish in Resource Packs (not bundled)".into()
        } else {
            format!(
                "Moves: {} external clips, {} files",
                s.pack.clips.len(),
                s.pack.files
            )
        }
    })
}
pub(super) fn emote_label() -> String {
    MOVES.with(|s| {
        let s = s.borrow();
        let entries = s.pack.emotes();
        entries
            .get(s.selected_emote % entries.len().max(1))
            .map_or("No external emotes loaded".into(), |e| {
                format!("Emote: {}", e.0)
            })
    })
}
pub(super) fn play_emote_at(game: &Game<'_>, index: usize) {
    MOVES.with(|s| s.borrow_mut().selected_emote = index);
    play_emote(game);
}
pub(super) fn keybindings(game: &Game<'_>) -> Vec<[String; 3]> {
    load(game);
    MOVES.with(|s| {
        s.borrow().pack.config["emotes"]["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter(|(_, e)| {
                e["animation"].as_str().is_some()
                    && (e["display_name"].as_str().is_some() || e["id"].as_str().is_some())
            })
            .filter_map(|(i, e)| {
                let id = e["id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| i.to_string());
                let label = e["display_name"].as_str().unwrap_or(&id);
                let key = e["keybind"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_ascii_lowercase();
                let key = if key.starts_with("key.") {
                    key
                } else {
                    format!("key.keyboard.{key}")
                };
                Some([format!("moves_emote_{id}"), format!("Emote: {label}"), key])
            })
            .collect()
    })
}
pub(super) fn emote_key(game: &Game<'_>, key: &str) -> bool {
    if matches!(
        key,
        "key.keyboard.f6" | "key.keyboard.f7" | "key.keyboard.f8"
    ) {
        return false;
    }
    if !enabled(game) {
        return false;
    }
    let bindings = keybindings(game);
    for (index, binding) in bindings.iter().enumerate() {
        if key == game.native_mod_key(ID, &binding[0], &binding[2]) {
            play_emote_at(game, index);
            return true;
        }
    }
    false
}
pub(super) fn select_emote(back: bool) {
    MOVES.with(|s| {
        let mut s = s.borrow_mut();
        let n = s.pack.emotes().len().max(1);
        s.selected_emote = if back {
            (s.selected_emote + n - 1) % n
        } else {
            (s.selected_emote + 1) % n
        };
    });
}
pub(super) fn play_emote(game: &Game<'_>) {
    if !enabled(game) {
        return;
    }
    load(game);
    MOVES.with(|s| {
        let mut s = s.borrow_mut();
        let pack = s.pack.clone();
        let entries = pack.emotes();
        if let Some((_, clip)) = entries.get(s.selected_emote % entries.len().max(1)) {
            if s.controller.play_emote(&pack, clip) {
                s.emote_face = pack.config["emotes"]["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|entry| entry["animation"].as_str() == Some(clip))
                    .and_then(|entry| entry["face"].as_str())
                    .unwrap_or("")
                    .into();
            }
        }
    });
}
pub(super) fn stop_emote() {
    MOVES.with(|s| s.borrow_mut().controller.stop_emote());
}
pub(super) fn used(hand: Hand, success: bool) {
    if success {
        MOVES.with(|s| s.borrow_mut().used = Some(usize::from(hand == Hand::Off)));
    }
}
/// Capture before the game may consume, replace or swap the acting stack.
/// Pair with `finish_use` around the accepted game's use, not a mouse press.
pub(super) struct UseCapture {
    hand: usize,
    item: String,
    charged: bool,
    fishing: bool,
}
pub(super) fn capture_use(game: &Game<'_>, hand: Hand) -> Option<UseCapture> {
    if !game.native_window() || !enabled(game) {
        return None;
    }
    let stack = if hand == Hand::Main {
        game.inventory.get(game.selected).and_then(Option::as_ref)
    } else {
        game.offhand.as_ref()
    };
    Some(UseCapture {
        hand: usize::from(hand == Hand::Off),
        item: stack.map_or(String::new(), |s| s.item.to_string()),
        charged: stack
            .and_then(|s| s.data())
            .is_some_and(|d| !d.charged.is_empty()),
        fishing: game.cast_rod().is_some(),
    })
}
pub(super) fn finish_use(game: &Game<'_>, before: Option<UseCapture>, success: bool) {
    let Some(before) = before.filter(|_| success && game.native_window() && enabled(game)) else {
        return;
    };
    let event = if before.item == "minecraft:fishing_rod" {
        if before.fishing { "reel" } else { "cast" }
    } else {
        "use"
    };
    let crossbow = before.item == "minecraft:crossbow" && before.charged;
    let firework = before.item == "minecraft:firework_rocket"
        && game.player.as_ref().is_some_and(|p| p.fall_flying);
    MOVES.with(|s| {
        s.borrow_mut().events.used = Some(AcceptedUse {
            hand: before.hand,
            item: before.item,
            event,
            crossbow,
            firework,
        });
    });
}
/// `remote_start_attack` returns whether a block broke instantly, NOT whether
/// an attack was admitted. Observe its accepted swing/reset instead of $value.
pub(super) struct AttackCapture {
    swings: u32,
    ticker: i32,
    strength: f32,
    block: bool,
}
pub(super) fn capture_attack(game: &Game<'_>) -> Option<AttackCapture> {
    if !game.native_window() || !enabled(game) || game.spectator() {
        return None;
    }
    Some(AttackCapture {
        swings: game.swings_started,
        ticker: game.hand_strength.ticker,
        strength: attack_strength(game),
        block: !game.entity_targeted() && game.pick().is_some(),
    })
}
pub(super) fn finish_attack(game: &Game<'_>, before: Option<AttackCapture>) {
    let Some(before) = before else { return };
    if before.block || !game.native_window() || !enabled(game) {
        return;
    }
    let reset = game.hand_strength.ticker < before.ticker;
    if reset || (game.swings_started != before.swings && game.hand_strength.ticker == 0) {
        MOVES.with(|s| {
            let mut s = s.borrow_mut();
            s.events.attacks = s.events.attacks.saturating_add(1).min(16);
            s.events.attack_strength = Some(before.strength);
        });
    }
}
fn attack_strength(game: &Game<'_>) -> f32 {
    if game.legacy_rules() {
        1.
    } else if game.remote.is_some() {
        let speed = game.main_hand_attack_speed();
        game.hand_strength.scale(0.5, speed)
    } else {
        game.attack_strength_scale()
    }
}
/// Forward received entity statuses (29 blocked, 30 shield broke, 55 swap).
/// The hand comes from active use, never from a held right mouse button.
pub(super) fn entity_event(game: &Game<'_>, id: i32, event: u8) {
    if !game.native_window() || !enabled(game) || !matches!(event, 29 | 30 | 55) {
        return;
    }
    MOVES.with(|s| {
        let mut s = s.borrow_mut();
        let own = game.player.as_ref().is_some_and(|p| p.entity_id == id);
        let (previous, pending) = if own {
            let Session {
                previous, events, ..
            } = &mut *s;
            (previous.as_ref(), events)
        } else {
            let Some(state) = s.remote.get_mut(&(id as u32)) else {
                return;
            };
            (state.previous.as_ref(), &mut state.events)
        };
        if event == 55 {
            pending.swapped = true;
            return;
        }
        let hand = if own {
            game.used_hand().map(|h| usize::from(h == Hand::Off))
        } else {
            game.mobs
                .iter()
                .find(|m| m.id == id as u32)
                .filter(|m| m.extra.using_item > 0)
                .map(|m| usize::from(m.extra.using_off_hand))
        }
        .or_else(|| previous.and_then(|i| i.using.map(|v| v.0)))
        .or_else(|| previous.and_then(|i| i.items.iter().position(|id| id == "minecraft:shield")));
        if event == 29 {
            pending.shield_hit = hand;
        } else {
            pending.shield_broken = hand;
        }
    });
}
fn is_ours(pose: &Arc<dyn horizon_hooks::players::BodyPose>) -> bool {
    let any: &dyn std::any::Any = &**pose;
    any.is::<horizon_client::mobs::anim::punchy_moves::Body>()
}
/// Called every frame too, so menus release our pose even while world ticks pause.
pub(super) fn frame(game: &mut Game<'_>) {
    if !game.native_window() {
        return;
    }
    super::punchy_camera::frame(game);
    let menu = game.screen != Screen::Playing || game.screen_over_playing() || !game.captured;
    MOVES.with(|s| {
        let mut s = s.borrow_mut();
        if menu {
            s.controller.stop_emote();
            s.events = Pending::default();
            s.used = None;
            s.previous = None;
        }
        if game.player.is_none() || !enabled(game) {
            s.controller = Controller::default();
            s.remote.clear();
            s.used = None;
            s.armor.clear();
            s.events = Pending::default();
            s.previous = None;
        }
    });
    if menu
        || !enabled(game)
        || game
            .player
            .as_ref()
            .is_none_or(|p| p.health.dead_or_dying())
        || game.sleeping.is_some()
        || game.spectator()
        || game
            .native_setting(ID, "moves_animations_enabled")
            .as_deref()
            == Some("false")
    {
        if let Some(model) = game.player_model.as_mut() {
            if model.emote.as_ref().is_some_and(is_ours) {
                model.emote = None;
            }
        }
    }
    if !enabled(game)
        || game
            .native_setting(ID, "moves_animations_enabled")
            .as_deref()
            == Some("false")
    {
        for model in game.mobs.iter_mut() {
            if model.emote.as_ref().is_some_and(is_ours) {
                model.emote = None;
            }
        }
    }
    if let Some(path) = std::env::var_os("PUNCHY_MOVES_DIAGNOSTICS") {
        let value = MOVES.with(|s| {
            let s=s.borrow();
            serde_json::json!({"enabled":enabled(game),"clips":s.pack.clips.len(),
                "warnings":s.pack.warnings,"camera_claimed":s.camera_claimed,
                "pose_owned":game.player_model.as_ref().and_then(|m|m.emote.as_ref()).is_some_and(is_ours),
                "layers":s.controller.frame.layers.iter().map(|l|l.clip.name.as_str()).collect::<Vec<_>>(),
                "camera":super::punchy_camera::diagnostic(),
                "faces":super::punchy_faces::diagnostic(game)})
        });
        if let Ok(bytes) = serde_json::to_vec(&value) {
            let _ = std::fs::write(path, bytes);
        }
    }
}
pub(super) fn camera_allowed(game: &Game<'_>) -> bool {
    enabled(game)
        && game.player.is_some()
        && !game.spectator()
        && game.sleeping.is_none()
        && game.screen == Screen::Playing
        && !game.screen_over_playing()
        && game.captured
        && game.camera_type == perspective::CameraType::ThirdPersonBack
        && !MOVES.with(|s| s.borrow().camera_claimed)
}
/// Starts before Spatial GUI's cancellable camera_view hook. Vanilla clears it
/// only when it computes third-person distance. A foreign camera keeps its
/// lease through its entire closing animation, without optional cross-mod calls.
pub(super) fn camera_query(claimed: bool) {
    MOVES.with(|s| s.borrow_mut().camera_claimed = claimed);
}
pub(super) fn camera_settings() -> Option<serde_json::Value> {
    MOVES.with(|s| s.borrow().controller.camera.clone())
}
pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_window() {
        return;
    }
    frame(game);
    if !enabled(game) {
        return;
    }
    load(game);
    remote_tick(game);
    if game.screen != Screen::Playing || game.screen_over_playing() || !game.captured {
        return;
    }
    let gravity = game
        .player
        .as_mut()
        .and_then(|p| p.attribute("minecraft:gravity").ok())
        .unwrap_or(0.08);
    let Some(p) = game.player.as_ref() else {
        clear();
        return;
    };
    if p.health.dead_or_dying() || game.sleeping.is_some() || game.spectator() {
        return;
    }
    let using = game.used_hand().map(|h| {
        let stack = if h == Hand::Main {
            game.inventory.get(game.selected).and_then(Option::as_ref)
        } else {
            game.offhand.as_ref()
        };
        let kind = stack.map_or(Use::Other, |stack| use_kind(game, stack));
        (usize::from(h == Hand::Off), kind)
    });
    let items = [
        game.inventory
            .get(game.selected)
            .and_then(Option::as_ref)
            .map_or(String::new(), |v| v.item.to_string()),
        game.offhand
            .as_ref()
            .map_or(String::new(), |v| v.item.to_string()),
    ];
    let armor: Vec<_> = game
        .armor
        .iter()
        .map(|v| v.as_ref().map_or(String::new(), |s| s.item.to_string()))
        .collect();
    let vehicle = game.riding.and_then(|id| vehicle(game, id));
    let riding = game.riding.map(|_| {
        vehicle
            .as_ref()
            .map_or_else(|| "minecraft:unknown".into(), |v| v.0.clone())
    });
    let music = game.jukebox_sounds.keys().any(|&(x, y, z)| {
        let d = glam::DVec3::new(
            p.position.x - x as f64,
            p.position.y - y as f64,
            p.position.z - z as f64,
        );
        d.x * d.x + d.y * d.y + d.z * d.z < 144.
    });
    let input = Input {
        velocity: glam::Vec3::new(
            p.velocity.x as f32,
            p.velocity.y as f32,
            p.velocity.z as f32,
        ),
        body_yaw: game.player_model.as_ref().map(|m| m.body_yaw),
        selected_slot: game.selected as u8,
        remote: false,
        in_water: p.fluids.was_touching_water,
        in_lava: p.is_in_lava(),
        position_y: Some(p.position.y as f32),
        eye_y: Some((p.position.y + f64::from(p.dimensions.eye_height)) as f32),
        use_ticks: game.using.map(|(_, ticks)| ticks),
        use_speed_multiplier: p.item_use_speed,
        attack_strength: Some(attack_strength(game)),
        random_seed: uuid_seed(game.acting_uuid()),
        external_attack: game
            .player_model
            .as_ref()
            .and_then(|m| m.emote.as_ref())
            .is_some_and(|p| !is_ours(p)),
        freeze_progress: Some((p.ticks_frozen as f32 / 140.).clamp(0., 1.)),
        food_level: Some(p.health.food.food.clamp(0, 20) as u8),
        yaw: game.yaw,
        pitch: game.pitch,
        movement: p.applied_input,
        speed: (p.velocity.x * p.velocity.x + p.velocity.z * p.velocity.z).sqrt() as f32,
        vertical_speed: p.velocity.y as f32,
        fall_distance: p.fall_distance as f32,
        grounded: p.ground.on_ground,
        sprinting: p.sprinting,
        crouching: p.shift_key_down,
        swimming: p.swimming,
        underwater: p.fluids.was_eye_in_water,
        crawling: p.pose == horizon_client::pose::Pose::Swimming && !p.swimming,
        climbing: p.on_climbable,
        flying: p.flying,
        elytra: p.fall_flying,
        riding,
        cold: p.ticks_frozen > 0,
        hungry: p.health.food.food <= 6,
        nausea: game.potions.effects.get("nausea").is_some(),
        music,
        using,
        items,
        main_left: game.settings.main_hand_left,
        mining: game.mining.is_some(),
        charged: using
            .map_or_else(
                || game.inventory.get(game.selected).and_then(Option::as_ref),
                |(hand, _)| {
                    if hand == 1 {
                        game.offhand.as_ref()
                    } else {
                        game.inventory.get(game.selected).and_then(Option::as_ref)
                    }
                },
            )
            .and_then(|v| v.data())
            .is_some_and(|d| !d.charged.is_empty()),
        fishing: game.cast_rod().is_some(),
        fishing_bite: super::fishing::punchy_fishing::biting(game),
        ..Default::default()
    };
    let mut input = input;
    super::punchy_probes::capture(game, &mut input, gravity);
    let eye = glam::DVec3::new(
        p.position.x,
        p.position.y + f64::from(p.dimensions.eye_height),
        p.position.z,
    );
    let (forward, _, _) = punchy_engine::moves::ShoulderCamera::basis_vectors(game.yaw, game.pitch);
    input.block_distance = game
        .clip_ray(eye.to_array(), forward.as_dvec3().to_array(), 4.)
        .map_or(f32::INFINITY, |h| {
            eye.distance(glam::DVec3::from_array(h.point)) as f32
        });
    if let Some((_, yaw, velocity)) = vehicle {
        input.vehicle_yaw = yaw;
        input.vehicle_velocity = velocity;
    }
    let hurt_time = p.health.hurt_animation_time;
    let hurt_from_back = p.health.hurt_dir.to_radians().cos() < 0.;
    MOVES.with(|s| {
        let mut s = s.borrow_mut();
        let mut input = input;
        // A use, placement or mining swing is not a primary action. The
        // accepted attack hook supplies pulses even when vanilla swing
        // playback cannot restart yet.
        input.attack_presses = s.events.attacks;
        input.attack = input.attack_presses > 0 && !input.mining;
        input.attack_strength = s.events.attack_strength.or(input.attack_strength);
        input.used = s.used.take();
        let pending = std::mem::take(&mut s.events);
        if let Some(used) = pending.used {
            input.used = Some(used.hand);
            input.accepted_item = Some(used.item);
            input.use_event = Some(used.event.into());
            input.crossbow_shot = used.crossbow.then_some(used.hand);
            input.firework_boost = used.firework;
        } else if let Some(hand) = input.used {
            // Compatibility for callers of the original used(hand, success)
            // API. The pre/post capture hook is the authoritative path.
            input.accepted_item = s
                .previous
                .as_ref()
                .filter(|i| i.selected_slot == input.selected_slot)
                .map(|i| i.items[hand].clone());
        }
        input.shield_hit = pending.shield_hit;
        input.shield_broken = pending.shield_broken;
        input.swapped = pending.swapped;
        if let Some(previous) = &s.previous {
            input.use_released = previous.using.is_some()
                && input.using.is_none()
                && previous.selected_slot == input.selected_slot
                && previous.items == input.items
                && !game.keys.held(Mapping::Use);
            input.swapped |= previous.selected_slot == input.selected_slot
                && previous.items != input.items
                && previous.items[0] == input.items[1]
                && previous.items[1] == input.items[0];
        }
        input.hurt = hurt_time > s.hurt;
        input.hurt_from_back = hurt_from_back;
        s.hurt = hurt_time;
        if !s.armor.is_empty() && s.armor != armor {
            input.armor_changed = Some(s.armor.first() != armor.first());
        }
        s.armor = armor;
        let pack = s.pack.clone();
        s.controller.tick(&pack, &input);
        s.previous = Some(input);
        let animation_enabled = game
            .native_setting(ID, "moves_animations_enabled")
            .as_deref()
            != Some("false");
        if let Some(model) = game.player_model.as_mut() {
            // Another native emote owns this slot: never stomp it.
            if model.emote.as_ref().is_none_or(is_ours)
                && !pack.clips.is_empty()
                && animation_enabled
            {
                if let Some(yaw) = s.controller.frame.body_yaw {
                    model.body_yaw += (yaw - model.body_yaw + 180.).rem_euclid(360.) - 180.;
                }
                model.emote = Some(Arc::new(horizon_client::mobs::anim::punchy_moves::Body {
                    frame: s.controller.frame.clone(),
                    age: model.age as f32,
                }));
            }
        }
    });
}

fn remote_tick(game: &mut Game<'_>) {
    let animation_enabled = game
        .native_setting(ID, "moves_animations_enabled")
        .as_deref()
        != Some("false");
    let mut inputs = Vec::new();
    let grace = MOVES.with(|s| {
        punchy_engine::moves_animation::number(
            &s.borrow().pack.entry("GENERAL", "swimming")["surface_grace_distance"],
            1.35,
        )
    });
    for m in game
        .mobs
        .iter()
        .filter(|m| matches!(&*m.kind.id, "minecraft:player" | "minecraft:mannequin") && !m.removed)
        .take(256)
    {
        let speed = (m.velocity.x * m.velocity.x + m.velocity.z * m.velocity.z).sqrt() as f32;
        let yaw = m.body_yaw.to_radians();
        let forward =
            (m.velocity.z * f64::from(yaw.cos()) - m.velocity.x * f64::from(yaw.sin())) as f32;
        let left =
            (m.velocity.x * f64::from(yaw.cos()) + m.velocity.z * f64::from(yaw.sin())) as f32;
        let item = m.extra.main_hand.as_deref().unwrap_or("").to_owned();
        let off = m.extra.off_hand.as_deref().unwrap_or("").to_owned();
        let using = (m.extra.using_item > 0).then(|| {
            let hand = usize::from(m.extra.using_off_hand);
            let fallback = ItemStack::new(
                if hand == 0 {
                    item.as_str()
                } else {
                    off.as_str()
                },
                1,
            );
            let stack = if hand == 0 {
                m.extra.main_hand_stack().unwrap_or(&fallback)
            } else {
                &fallback
            };
            (hand, use_kind(game, stack))
        });
        let mut i = Input {
            remote: true,
            body_yaw: Some(m.body_yaw),
            velocity: glam::Vec3::new(
                m.velocity.x as f32,
                m.velocity.y as f32,
                m.velocity.z as f32,
            ),
            in_water: m.fluids.in_water,
            in_lava: m.fluids.in_lava,
            use_ticks: Some(m.extra.using_item.max(0) as u32),
            yaw: m.head_yaw,
            pitch: m.head_pitch,
            movement: [left, forward],
            speed,
            vertical_speed: m.velocity.y as f32,
            fall_distance: m.fall_distance as f32,
            grounded: m.on_ground,
            sprinting: m.extra.sprinting,
            crouching: m.extra.crouching,
            swimming: m.extra.avatar_pose == horizon_client::pose::Pose::Swimming
                && m.fluids.in_water,
            underwater: m.was_eye_in_water,
            crawling: m.extra.avatar_pose == horizon_client::pose::Pose::Swimming
                && !m.fluids.in_water,
            climbing: m.on_climbable,
            elytra: m.extra.fall_flying,
            external_attack: m.extra.auto_spin_attack
                || m.emote.as_ref().is_some_and(|p| !is_ours(p)),
            riding: m
                .extra
                .mount
                .map(|id| vehicle(game, id).map_or_else(|| "minecraft:unknown".into(), |v| v.0)),
            sleeping: m.extra.avatar_pose == horizon_client::pose::Pose::Sleeping,
            spectator: m.extra.spectator,
            dead: m.health <= 0. || m.death_time > 0,
            cold: m.ticks_frozen > 0,
            nausea: m
                .extra
                .effects
                .iter()
                .any(|(id, _, _)| *id == "minecraft:nausea" || *id == "nausea"),
            music: music_near(game, m.position),
            random_seed: u64::from(m.id),
            charged: m
                .extra
                .main_hand_stack()
                .and_then(|s| s.data())
                .is_some_and(|d| !d.charged.is_empty()),
            items: [item, off],
            main_left: m.extra.main_hand_left,
            using,
            block_distance: f32::INFINITY,
            ..Default::default()
        };
        super::punchy_probes::capture_remote(game, m, &mut i, grace);
        if let Some((_, yaw, velocity)) = m.extra.mount.and_then(|id| vehicle(game, id)) {
            i.vehicle_yaw = yaw;
            i.vehicle_velocity = velocity;
        }
        inputs.push((m.id, m.age, m.anim.attack_anim, m.hurt_time, i));
    }
    MOVES.with(|s| {
        let mut s = s.borrow_mut();
        let pack = s.pack.clone();
        s.remote.retain(|id, _| inputs.iter().any(|i| i.0 == *id));
        for (id, age, swing, hurt, mut input) in inputs {
            let state = s.remote.entry(id).or_default();
            input.attack =
                input.using.is_none() && swing > 0. && (state.swing == 0. || swing < state.swing);
            input.hurt = hurt > state.hurt;
            let pending = std::mem::take(&mut state.events);
            input.shield_hit = pending.shield_hit;
            input.shield_broken = pending.shield_broken;
            input.swapped = pending.swapped;
            if let Some(previous) = &state.previous {
                input.use_released = previous.using.is_some()
                    && input.using.is_none()
                    && previous.items == input.items;
                // Equipment data is authoritative for a remote loaded bow.
                input.crossbow_shot = (previous.charged
                    && !input.charged
                    && previous.items[0] == input.items[0]
                    && input.items[0] == "minecraft:crossbow")
                    .then_some(0);
            }
            state.swing = swing;
            state.hurt = hurt;
            state.controller.tick(&pack, &input);
            let pose_allowed = !input.sleeping && !input.spectator && !input.dead;
            state.previous = Some(input);
            if let Some(m) = game.mobs.iter_mut().find(|m| m.id == id) {
                if m.emote.as_ref().is_none_or(is_ours) {
                    m.emote = if animation_enabled && pose_allowed && !pack.clips.is_empty() {
                        Some(Arc::new(horizon_client::mobs::anim::punchy_moves::Body {
                            frame: state.controller.frame.clone(),
                            age: age as f32,
                        }))
                    } else {
                        None
                    };
                }
            }
        }
    });
}

fn vehicle(game: &Game<'_>, id: u32) -> Option<(String, f32, glam::Vec3)> {
    let v = |v: horizon_core::geometry::Vec3| glam::Vec3::new(v.x as f32, v.y as f32, v.z as f32);
    if let Some(m) = game.mobs.iter().find(|m| m.id == id) {
        Some((m.kind.id.to_string(), m.yaw, v(m.velocity)))
    } else if let Some(b) = game.boats.iter().find(|b| b.id == id) {
        Some((b.entity_type(), b.yaw, v(b.velocity)))
    } else {
        game.carts
            .iter()
            .find(|cart| cart.id == id)
            .map(|cart| (cart.kind.id().into(), cart.yaw, v(cart.velocity)))
    }
}
fn use_kind(game: &Game<'_>, stack: &ItemStack) -> Use {
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
    match animation {
        horizon_world::consumable::UseAnimation::Eat => Use::Eat,
        horizon_world::consumable::UseAnimation::Drink => Use::Drink,
        _ => Use::Other,
    }
}

fn uuid_seed(uuid: [i32; 4]) -> u64 {
    let high = (u64::from(uuid[0] as u32) << 32) | u64::from(uuid[1] as u32);
    let low = (u64::from(uuid[2] as u32) << 32) | u64::from(uuid[3] as u32);
    high ^ low.rotate_left(21)
}

fn music_near(game: &Game<'_>, at: horizon_core::geometry::Vec3) -> bool {
    game.jukebox_sounds.keys().any(|&(x, y, z)| {
        (at.x - f64::from(x)).powi(2)
            + (at.y - f64::from(y)).powi(2)
            + (at.z - f64::from(z)).powi(2)
            < 144.
    })
}

/// Share the body's semantic clock with Expressions instead of independently
/// guessing jumps, landings, dives and equipment timelines from render frames.
pub(super) fn face_signals(
    own: bool,
    id: u32,
    c: &mut punchy_engine::moves_expressions::FaceConditions,
) {
    use punchy_engine::moves_animation::motion::State;
    MOVES.with(|session| {
        let session = session.borrow();
        let controller = if own {
            &session.controller
        } else if let Some(state) = session.remote.get(&id) {
            &state.controller
        } else {
            return;
        };
        let signals = &controller.signals;
        if own && controller.is_emote_playing() {
            c.emote = Some(session.emote_face.clone());
        }
        let previous = if own {
            session.previous.as_ref()
        } else {
            session.remote.get(&id).and_then(|s| s.previous.as_ref())
        };
        if let Some(previous) = previous {
            c.eating = previous.using.is_some_and(|(_, kind)| kind == Use::Eat);
            c.drinking = previous.using.is_some_and(|(_, kind)| kind == Use::Drink);
            c.cues
                .insert("hurt_behind".into(), f32::from(previous.hurt_from_back));
            c.cues.insert(
                "vehicle_speed".into(),
                previous
                    .vehicle_velocity
                    .x
                    .hypot(previous.vehicle_velocity.z)
                    * 20.,
            );
        }
        for (name, value) in &signals.values {
            c.cues.insert(name.clone(), *value as f32);
        }
        for (from, to) in [
            ("jumped", "liftoff"),
            ("landed", "landing"),
            ("crouched", "crouch"),
            ("stood_up", "stand"),
            ("splashed", "splash"),
            ("swapped", "swap"),
            ("run_started", "run_start"),
            ("run_stopped", "run_stop"),
            ("equipped", "equip"),
            ("idle_event", "fidget"),
            ("released", "release"),
            ("hurled", "hurl"),
            ("interacted", "interact"),
            ("ignited", "ignite"),
            ("sheared", "shear"),
            ("cast", "cast"),
            ("reeled", "reel"),
            ("lunged", "lunge"),
            ("boosted", "boost"),
            ("hurt", "hurt"),
        ] {
            let age = signals.since(from);
            if age != u64::MAX {
                c.events.insert(
                    to.into(),
                    (age as f32 + (c.time_seconds * 20.).fract()) * 0.05,
                );
            }
        }
        let throw_age = signals.since("threw");
        if throw_age != u64::MAX {
            let kind = match signals.values.get("thrown").copied().unwrap_or(1.) as u8 {
                2 => "pearl",
                3 => "brew",
                _ => "toss",
            };
            c.events.insert(
                kind.into(),
                (throw_age as f32 + (c.time_seconds * 20.).fract()) * 0.05,
            );
        }
        c.swimming = controller.state == State::Swim;
        c.bank = signals.values.get("elytra_bank").copied().unwrap_or(0.) as f32;
        c.moving = signals.values.get("moving").is_some_and(|v| *v > 0.);
        c.forward = signals.values.get("forward").copied().unwrap_or(0.) as f32
            - signals.values.get("backward").copied().unwrap_or(0.) as f32;
        c.strafe = signals.values.get("left").copied().unwrap_or(0.) as f32
            - signals.values.get("right").copied().unwrap_or(0.) as f32;
        c.cues.insert(
            "shallow_water".into(),
            signals.values.get("shallow_water").copied().unwrap_or(0.) as f32,
        );
        // Expressions' cannonball reaction means an active committed dive,
        // not simply a water block somewhere below a bridge.
        c.cues.insert(
            "water_below".into(),
            f32::from(controller.state == State::WaterAir),
        );
        c.cues.insert(
            "lava_below".into(),
            f32::from(controller.state == State::LavaAir),
        );
        c.cues.insert(
            "climb_direction".into(),
            if c.vertical_speed == 0. {
                0.
            } else {
                c.vertical_speed.signum()
            },
        );
        // Collect authored face channels from every current layer and overlay;
        // Frame::compose supplies exactly the same blending/retention as body.
        fn channels(
            layer: &punchy_engine::moves_animation::Layer,
            names: &mut std::collections::BTreeSet<String>,
        ) {
            names.extend(
                layer
                    .clip
                    .tracks
                    .keys()
                    .filter(|name| name.starts_with("face_"))
                    .take(64)
                    .cloned(),
            );
            for child in &layer.overlays {
                channels(child, names);
            }
        }
        let mut names = std::collections::BTreeSet::new();
        for layer in &controller.frame.layers {
            channels(layer, &mut names);
        }
        for name in names.into_iter().take(64) {
            let partial = (c.time_seconds * 20.).fract();
            if let Some(pose) =
                controller
                    .frame
                    .compose(&name, partial, punchy_engine::Pose::default())
            {
                // Body poses use model-space Y down; facial channels are
                // arbitrary scalar controls and retain the pack's raw Y.
                c.face_channels.insert(
                    name,
                    (pose.position * glam::Vec3::new(1., -1., 1.)).to_array(),
                );
            }
        }
    });
}
