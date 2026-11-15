use horizon_client::mobs::anim::punchy_moves::engine as punchy_engine;
// Horizon glue. This is an integration preview, not complete Punchy renderer parity.
use super::*;
use glam::{EulerRot, Mat4, Quat, Vec3};
use punchy_engine::player::Playback;
use punchy_engine::viewmodel::{Parameter, Target, Viewmodel};
use punchy_engine::{Clip, Pose, actions as action, moves, phases, resources};
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};

pub(super) const ID: &str = "punchy";
pub(super) const TITLE: &str = "Punchy + Moves (Rust) 0.2.0";
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum EditTarget {
    #[default]
    MovesCamera,
    Arms,
    Items,
}

impl EditTarget {
    fn previous(self) -> Self {
        self.next().next()
    }
    fn next(self) -> Self {
        match self {
            Self::MovesCamera => Self::Arms,
            Self::Arms => Self::Items,
            Self::Items => Self::MovesCamera,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::MovesCamera => "Punchy! Moves 3rd-Person Camera",
            Self::Arms => "1st-Person Arms Viewmodel",
            Self::Items => "1st-Person Held Items",
        }
    }
}

#[derive(Default)]
struct State {
    attempted: bool,
    source: Option<resources::Resources>,
    clips: BTreeMap<String, Arc<Clip>>,
    machine: action::ActionMachine,
    consume: phases::ConsumeController,
    mining: phases::MiningController,
    tick: i64,
    playback_id: u64,
    stack_keys: Vec<ItemStack>,
    forced_stack: Option<(usize, ItemStack)>,
    consume_hand: action::Hand,
    active: Option<Playback>,
    previous_swings: u32,
    use_down: bool,
    successful_use: Option<action::Hand>,
    mining_attempt: bool,
    offhand_item: bool,
    inspect: bool,
    last_items: [String; 2],
    error: Option<String>,
    main_left: bool,
    render_alpha: f32,
    enabled: bool,
    tuning: Option<punchy_engine::tuning::Tuning>,
    status: String,
    render_count: u64,
    viewmodel: Viewmodel,
    edit_target: EditTarget,
    shoulder_camera: moves::ShoulderCamera,
    live_hud_active: bool,
    hud_selected_param: usize,
}
struct Catalog<'a> {
    state: &'a State,
}
impl action::ClipCatalog for Catalog<'_> {
    fn resolve(&self, name: &str, hand: action::Hand) -> Option<action::ClipMeta> {
        let left = self.state.main_left != (hand == action::Hand::Off);
        let side = if left { "left" } else { "right" };
        let handed = format!("{name}_{side}");
        let aliases: &[&str] = match name {
            "use_drink" => &["use_drink2", "drink_loop", "drink_loop2"],
            "use_shield" => &["use_shield"],
            "mining" => &[
                "mining_foward",
                "mining_forward",
                "mining_sideways",
                "mining_digging",
            ],
            "inspect" => &["new_inspect_right", "new_inspect_left"],
            _ => &[],
        };
        let mapped = self
            .state
            .tuning
            .as_ref()
            .and_then(|t| t.animation(&self.state.last_items[hand.index()], name, left));
        let custom = mapped
            .as_ref()
            .and_then(|m| m.name.as_deref())
            .and_then(|n| {
                punchy_engine::tuning::handed_candidates(n, left)
                    .into_iter()
                    .find_map(|n| self.state.clips.get(&n))
            });
        let clip = custom
            .or_else(|| self.state.clips.get(&handed))
            .or_else(|| self.state.clips.get(name))
            .or_else(|| aliases.iter().find_map(|n| self.state.clips.get(*n)))?;
        Some(action::ClipMeta {
            name: clip.name.clone(),
            length: clip.length,
            hold_on_last_frame: clip.hold_on_last_frame,
            dual_handed: punchy_engine::policy::dual_handed(
                clip,
                0.,
                self.state.offhand_item,
                self.state.offhand_item,
            ),
        })
    }
}
fn load(state: &mut State) {
    if state.attempted {
        return;
    }
    state.attempted = true;
    let result = resources::embedded().and_then(|r| {
        let clips = punchy_engine::load_clips(&r.animations)?;
        Ok((r, clips))
    });
    match result {
        Ok((source, clips)) => {
            state.status = format!("{} clips loaded", clips.len());
            state.clips = clips.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
            state.tuning = punchy_engine::tuning::Tuning::parse(&source.tuning).ok();
            state.source = Some(source);
            state.error = None;
        }
        Err(e) => {
            state.status = "Bundled animations could not load".into();
            state.error = Some(e);
        }
    }
}
fn enabled(game: &Game<'_>) -> bool {
    game.native_on(ID) && game.native_setting(ID, "enabled").as_deref() != Some("false")
}
fn global_speed(game: &Game<'_>) -> f32 {
    game.native_setting(ID, "speed")
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|s| s.is_finite())
        .unwrap_or(5.5)
        .clamp(0.5, 5.5)
}
fn hand(h: Hand) -> action::Hand {
    if h == Hand::Main {
        action::Hand::Main
    } else {
        action::Hand::Off
    }
}

pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_window() {
        return;
    }
    let viewmodel = read_viewmodel(game);
    let shoulder_camera = read_shoulder_camera(game);
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.viewmodel = viewmodel;
        s.shoulder_camera.enabled = shoulder_camera.enabled && super::punchy_moves::enabled(game);
        s.shoulder_camera.horizontal = shoulder_camera.horizontal;
        s.shoulder_camera.vertical = shoulder_camera.vertical;
        s.shoulder_camera.depth = shoulder_camera.depth;
        s.shoulder_camera.fov = shoulder_camera.fov;
        s.shoulder_camera.rotation_yaw = shoulder_camera.rotation_yaw;
        s.shoulder_camera.rotation_pitch = shoulder_camera.rotation_pitch;
        if let Some(camera) = super::punchy_moves::camera_settings().filter(|_| {
            game.native_setting(ID, "moves_dynamic_framing").as_deref() != Some("false")
        }) {
            use punchy_engine::moves_animation::number;
            let blend = number(&camera["_blend"], 1.).clamp(0., 1.);
            let mixed = |key: &str, base: f32| base + (number(&camera[key], base) - base) * blend;
            s.shoulder_camera.horizontal = moves::CameraParameter::Horizontal
                .clamp(mixed("horizontal", shoulder_camera.horizontal));
            s.shoulder_camera.vertical =
                moves::CameraParameter::Vertical.clamp(mixed("vertical", shoulder_camera.vertical));
            s.shoulder_camera.depth =
                moves::CameraParameter::Depth.clamp(mixed("depth", shoulder_camera.depth));
            s.shoulder_camera.fov =
                moves::CameraParameter::Fov.clamp(mixed("fov", shoulder_camera.fov));
        }
        if !s.shoulder_camera.enabled {
            s.shoulder_camera.free_camera = false;
        }
        s.shoulder_camera.update(1.0 / 20.0);
    });
    if !enabled(game) || game.player.is_none() {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.active = None;
            s.machine = Default::default();
            s.consume = Default::default();
            s.mining = Default::default();
            s.forced_stack = None;
            s.successful_use = None;
            s.mining_attempt = false;
            s.previous_swings = game.swings_started;
        });
        return;
    }
    let using = game.used_hand();
    let items = [game.held().cloned(), game.offhand.clone()];
    let use_down = game.native_mapping("key.use").is_some_and(|m| m.0);
    let attack_down = game.native_mapping("key.attack").is_some_and(|m| m.0);
    let speed = global_speed(game);
    STATE.with(|cell| {
        let mut s=cell.borrow_mut();load(&mut s);
        if s.source.is_none() {return;}
        s.main_left=game.settings.main_hand_left;
        s.offhand_item=items[1].is_some();
        let mut input=action::Input { animation_speed:speed, use_down,
            mining:game.mining.is_some(),
            entity_target:game.pick_entity_hit().flatten().is_some(),
            block_target:game.pick().is_some(),
            inspect:std::mem::take(&mut s.inspect),
            ..Default::default() };
        let changed=game.swings_started!=s.previous_swings;
        s.previous_swings=game.swings_started;
        let mining_attempt=std::mem::take(&mut s.mining_attempt);
        input.attack=(changed && attack_down && !input.mining && !mining_attempt).then_some(hand(game.swing.attack_hand()));
        input.mining_trigger=mining_attempt || changed && input.mining;
        let use_hand=hand(using.unwrap_or(Hand::Main));
        let input_driven=s.tuning.as_ref().and_then(|t|t.animation(&s.last_items[use_hand.index()],"useItem",s.main_left!=(use_hand==action::Hand::Off))).is_some_and(|e|e.trigger_on_input);
        input.use_trigger=s.successful_use.take().or_else(|| (use_down && !s.use_down && input_driven).then_some(use_hand));
        s.use_down=use_down;
        for (i,stack) in items.iter().enumerate() {
            input.hands[i].kind=stack.as_ref().map_or(action::ItemKind::Empty,|v|action::ItemKind::from_id(&v.item));
            input.hands[i].crossbow_charged=stack.as_ref().and_then(|v|v.data()).is_some_and(|d|!d.charged.is_empty());
            let id=stack.as_ref().map_or("",|v|v.item.as_ref()).to_owned();
            if id!=s.last_items[i] {
                input.hotbar_changed|=i==0;
                input.equip=Some(action::EquipEvent {hand:if i==0 {action::Hand::Main}else{action::Hand::Off},entering:!id.is_empty()});
                s.last_items[i]=id;
            }
        }
        if let Some(h)=using {
            let i=usize::from(h==Hand::Off);
            if let Some(stack)=items[i].as_ref() {
                let owner=game.player_owner(Some(stack));
                let mut kind=input.hands[i].kind;
                if kind==action::ItemKind::Other {
                    let components=game.pack.item_renderer.models.components(&horizon_client::item_render::stack_view(stack));
                    let mut animation=components.get("minecraft:consumable").map_or(horizon_world::consumable::UseAnimation::None,horizon_client::first_person::consumable_animation);
                    game.native_use_animation(stack,&mut animation);
                    kind=match animation {horizon_world::consumable::UseAnimation::Eat=>action::ItemKind::Eat,
                        horizon_world::consumable::UseAnimation::Drink=>action::ItemKind::Drink,_=>kind};
                }
                input.using=Some(action::UseState {hand:hand(h),kind,
                    elapsed_ticks:owner.use_duration-owner.use_item_remaining_ticks-1,
                    charge_duration_ticks:owner.crossbow_charge_duration as i32,..Default::default()});
            }
        }
        if let Some(tuning)=s.tuning.as_ref() {
            let h=input.attack.unwrap_or(action::Hand::Main);
            let left=s.main_left!=(h==action::Hand::Off);
            if let Some(entry)=tuning.animation(&s.last_items[h.index()],"attack",left) {
                input.attack_options.custom_entry_present=true;
                input.attack_options.combo=if entry.variations.is_empty(){entry.name.into_iter().collect()}else{entry.variations};
                input.attack_options.critical_clip=entry.critical;
                input.attack_options.trigger_on_input=entry.trigger_on_input;
                input.attack_options.skip_blend_in=entry.skip_blend_in;
            }
        }
        input.has_bow_projectile_or_creative=game.creative || game.projectile_slot("minecraft:bow").is_some();
        input.bow_shot_configured=s.tuning.as_ref().is_some_and(|t|s.last_items.iter().enumerate().any(|(i,id)|t.animation(id,"use_bow_shot",s.main_left!=(i==1)).is_some()));
        if let Some(p)=&s.active {
            input.playback_active=!p.stopping;
            input.playback_clip=Some(p.clip.name.clone());input.playback_hand=Some(p.hand);
        }

        let phase_request = phase_tick(&mut s, &mut input, &items, speed, attack_down);

        // Split ownership to let the catalog borrow clip data during arbitration.
        let mut machine=std::mem::take(&mut s.machine);
        let intent=machine.tick(&input,&Catalog{state:&s});s.machine=machine;
        let core_won = intent.is_some();
        if let Some(intent)=intent {
            match intent.operation {
                action::Operation::Play(p) => {
                    if let Some(clip)=s.clips.get(&p.clip.name).cloned() {

                        s.status=format!("Playing {}",clip.name);
                        s.playback_id=s.playback_id.wrapping_add(1);
                        let from=s.active.take().map(Box::new);
                        s.active=Some(Playback{from,out_weight:1.,previous_out_weight:1.,id:s.playback_id,expression:p.start_time,previous_expression:p.start_time,return_fade:0.28,skip_tick:false,clip,hand:p.hand,previous:p.start_time,time:p.start_time,
                            multiplier:p.speed_multiplier,looping:p.looping,released:false,hold:p.hold_on_last_frame,
                            fade:p.blend_seconds.unwrap_or(0.2).max(0.001),stopping:false,
                            weight:if p.skip_blend_in {1.0}else{0.0},previous_weight:if p.skip_blend_in {1.}else{0.}});
                    }
                }
                action::Operation::Stop{hand,mode} => {
                    if let Some(p)=s.active.as_mut().filter(|p|p.hand==hand) {
                        p.looping=false;p.released=true;p.hold=false;
                        if let action::StopMode::BlendOut(seconds)=mode {p.stopping=true;p.return_fade=seconds.max(0.001);}
                    }
                }
            }
        }
        if !core_won {
            if let Some((consume, request)) = phase_request { apply_phase(&mut s, consume, &request, speed); }
        }
        if let Some(p)=s.active.as_mut() {
            p.advance(speed);
            if p.stopping && p.out_weight<=0. {s.active=None;}
        }
        if let Some(path)=std::env::var_os("PUNCHY_DIAGNOSTICS") {
            let value=serde_json::json!({"loaded":s.source.is_some(),"resource_source":"embedded","clips":s.clips.len(),"error":s.error,
                "active":s.active.as_ref().map(|p|serde_json::json!({"name":p.clip.name,"time":p.time,"weight":p.weight,"speed":punchy_engine::playback::rate(&p.clip,p.time,speed,p.multiplier,1.)})),
                "swings":s.previous_swings,"mining":input.mining,"consume_phase":format!("{:?}",s.consume.phase()),"mining_phase":format!("{:?}",s.mining.phase())});
            if let Ok(bytes)=serde_json::to_vec_pretty(&value) {let _=std::fs::write(path,bytes);}
        }
    });
}

pub(super) fn key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if game.native_on("spatial_gui")
        && matches!(
            key,
            "key.keyboard.f6" | "key.keyboard.f7" | "key.keyboard.f8"
        )
    {
        return;
    }
    if !pressed || !game.native_playing() || !enabled(game) {
        return;
    }
    if game.screen != Screen::Playing || game.screen_over_playing() || !game.captured {
        return;
    }
    if super::punchy_moves::emote_key(game, key) {
        return;
    }
    if key == game.native_mod_key(ID, "moves_camera_toggle", "key.keyboard.v")
        && super::punchy_moves::enabled(game)
    {
        super::punchy_camera::toggle();
    }
    if key == game.native_mod_key(ID, "inspect", "key.keyboard.f10") {
        STATE.with(|s| s.borrow_mut().inspect = true);
    }
    if key == game.native_mod_key(ID, "reload", "key.keyboard.f12") {
        STATE.with(|s| *s.borrow_mut() = State::default());
        super::punchy_moves::clear();
        super::punchy_faces::clear(game);
    }
    if key == game.native_mod_key(ID, "moves_orbit", "key.keyboard.f4")
        && super::punchy_moves::camera_allowed(game)
        && read_shoulder_camera(game).enabled
    {
        super::punchy_camera::toggle_orbit();
    }
    if key == game.native_mod_key(ID, "moves_emote", "key.keyboard.n") {
        super::punchy_moves::play_emote(game);
    }
    if key == game.native_mod_key(ID, "moves_next_emote", "key.keyboard.m") {
        super::punchy_moves::select_emote(false);
    }
    if key == game.native_mod_key(ID, "live_hud", "key.keyboard.f9") {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.live_hud_active = !s.live_hud_active;
            if s.live_hud_active {
                s.viewmodel = read_viewmodel(game);
                let camera = read_shoulder_camera(game);
                s.shoulder_camera.horizontal = camera.horizontal;
                s.shoulder_camera.vertical = camera.vertical;
                s.shoulder_camera.depth = camera.depth;
                s.shoulder_camera.fov = camera.fov;
            }
        });
    }

    let is_live_hud = STATE.with(|s| s.borrow().live_hud_active);
    if is_live_hud {
        match key {
            "key.keyboard.tab" => {
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    s.edit_target = s.edit_target.next();
                });
            }
            "key.keyboard.up" | "key.keyboard.page.up" => {
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    s.hud_selected_param = if s.hud_selected_param == 0 {
                        3
                    } else {
                        s.hud_selected_param - 1
                    };
                });
            }
            "key.keyboard.down" | "key.keyboard.page.down" => {
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    s.hud_selected_param = (s.hud_selected_param + 1) % 4;
                });
            }
            "key.keyboard.left" | "key.keyboard.minus" | "key.keyboard.left.bracket" => {
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    let target = s.edit_target;
                    let param_idx = s.hud_selected_param;
                    step_target_param(game, &mut s, target, param_idx, false);
                });
            }
            "key.keyboard.right" | "key.keyboard.equal" | "key.keyboard.right.bracket" => {
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    let target = s.edit_target;
                    let param_idx = s.hud_selected_param;
                    step_target_param(game, &mut s, target, param_idx, true);
                });
            }
            "key.keyboard.backspace" => {
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    let target = s.edit_target;
                    let param_idx = s.hud_selected_param;
                    reset_target_param(game, &mut s, target, param_idx);
                });
            }
            "key.keyboard.r" => {
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    let target = s.edit_target;
                    reset_target_all(game, &mut s, target);
                });
            }
            _ => {}
        }
    }
}

fn read_viewmodel(game: &Game<'_>) -> Viewmodel {
    Viewmodel::read(|key| game.native_setting(ID, key))
}
fn read_shoulder_camera(game: &Game<'_>) -> moves::ShoulderCamera {
    moves::ShoulderCamera::read(|key| game.native_setting(ID, key))
}

fn get_target_param_display(s: &State, target: EditTarget, index: usize) -> (String, String) {
    match target {
        EditTarget::MovesCamera => {
            let p = moves::CameraParameter::ALL[index.min(3)];
            let val = match p {
                moves::CameraParameter::Horizontal => s.shoulder_camera.horizontal,
                moves::CameraParameter::Vertical => s.shoulder_camera.vertical,
                moves::CameraParameter::Depth => s.shoulder_camera.depth,
                moves::CameraParameter::Fov => s.shoulder_camera.fov,
            };
            (p.label().to_string(), p.display(val))
        }
        EditTarget::Arms | EditTarget::Items => {
            let vm_target = if target == EditTarget::Arms {
                Target::Arms
            } else {
                Target::Items
            };
            let p = Parameter::ALL[index.min(3)];
            let val = s.viewmodel.get(vm_target).value(p);
            (p.label().to_string(), p.display(val))
        }
    }
}

fn step_target_param(
    game: &Game<'_>,
    s: &mut State,
    target: EditTarget,
    index: usize,
    increase: bool,
) {
    match target {
        EditTarget::MovesCamera => {
            let cp = moves::CameraParameter::ALL[index.min(3)];
            let current = match cp {
                moves::CameraParameter::Horizontal => s.shoulder_camera.horizontal,
                moves::CameraParameter::Vertical => s.shoulder_camera.vertical,
                moves::CameraParameter::Depth => s.shoulder_camera.depth,
                moves::CameraParameter::Fov => s.shoulder_camera.fov,
            };
            let stepped = cp.step(current, increase);
            game.native_set_setting(ID, cp.key(), &format!("{:.2}", stepped));
            match cp {
                moves::CameraParameter::Horizontal => s.shoulder_camera.horizontal = stepped,
                moves::CameraParameter::Vertical => s.shoulder_camera.vertical = stepped,
                moves::CameraParameter::Depth => s.shoulder_camera.depth = stepped,
                moves::CameraParameter::Fov => s.shoulder_camera.fov = stepped,
            }
        }
        EditTarget::Arms | EditTarget::Items => {
            let vm_target = if target == EditTarget::Arms {
                Target::Arms
            } else {
                Target::Items
            };
            let p = Parameter::ALL[index.min(3)];
            let value = s.viewmodel.get(vm_target).value(p);
            let stepped = p.step(value, increase);
            game.native_set_setting(ID, p.key(vm_target), &format!("{:.2}", stepped));
            s.viewmodel = read_viewmodel(game);
        }
    }
}

fn reset_target_param(game: &Game<'_>, s: &mut State, target: EditTarget, index: usize) {
    match target {
        EditTarget::MovesCamera => {
            let cp = moves::CameraParameter::ALL[index.min(3)];
            let def = cp.default_value();
            game.native_set_setting(ID, cp.key(), &def.to_string());
            match cp {
                moves::CameraParameter::Horizontal => s.shoulder_camera.horizontal = def,
                moves::CameraParameter::Vertical => s.shoulder_camera.vertical = def,
                moves::CameraParameter::Depth => s.shoulder_camera.depth = def,
                moves::CameraParameter::Fov => s.shoulder_camera.fov = def,
            }
        }
        EditTarget::Arms | EditTarget::Items => {
            let vm_target = if target == EditTarget::Arms {
                Target::Arms
            } else {
                Target::Items
            };
            let p = Parameter::ALL[index.min(3)];
            let def = p.default_value();
            game.native_set_setting(ID, p.key(vm_target), &def.to_string());
            s.viewmodel = read_viewmodel(game);
        }
    }
}

fn reset_target_all(game: &Game<'_>, s: &mut State, target: EditTarget) {
    match target {
        EditTarget::MovesCamera => {
            for cp in moves::CameraParameter::ALL {
                game.native_set_setting(ID, cp.key(), &cp.default_value().to_string());
            }
            s.shoulder_camera.horizontal = moves::CameraParameter::Horizontal.default_value();
            s.shoulder_camera.vertical = moves::CameraParameter::Vertical.default_value();
            s.shoulder_camera.depth = moves::CameraParameter::Depth.default_value();
            s.shoulder_camera.fov = moves::CameraParameter::Fov.default_value();
        }
        EditTarget::Arms | EditTarget::Items => {
            let vm_target = if target == EditTarget::Arms {
                Target::Arms
            } else {
                Target::Items
            };
            for p in Parameter::ALL {
                game.native_set_setting(ID, p.key(vm_target), &p.default_value().to_string());
            }
            s.viewmodel = read_viewmodel(game);
        }
    }
}

pub(super) fn settings(game: &Game<'_>, buttons: &mut Vec<String>) {
    let on = enabled(game);
    // 0: Master preview toggle
    buttons.push(format!(
        "Punchy Preview: {}",
        if on { "ENABLED" } else { "DISABLED" }
    ));

    // 1: Live HUD Editor toggle (F9)
    let live_hud = STATE.with(|s| s.borrow().live_hud_active);
    buttons.push(format!(
        "In-Game Live HUD Editor (F9): {}",
        if live_hud { "OPEN" } else { "CLOSED" }
    ));

    // 2: Third-Person Shoulder Cam toggle
    let shoulder_on = game.native_setting(ID, "moves_camera_enabled").as_deref() != Some("false");
    buttons.push(format!(
        "Third-Person Shoulder Cam: {}",
        if shoulder_on { "ENABLED" } else { "DISABLED" }
    ));

    // 3: Orbit Camera toggle (F8)
    let orbit = super::punchy_camera::orbiting();
    buttons.push(format!(
        "Orbit / Free Camera (F4): {}",
        if orbit { "ACTIVE" } else { "INACTIVE" }
    ));

    // 4: Hand Poses toggle
    buttons.push(format!(
        "Hand Poses: {}",
        if game.native_setting(ID, "hand_poses").as_deref() == Some("horizon") {
            "Horizon (Vanilla)"
        } else {
            "Punchy (Full Motion)"
        }
    ));

    // 5: Animation speed
    buttons.push(format!(
        "Animation Speed: {:.1}x (click to change)",
        global_speed(game)
    ));

    let viewmodel = read_viewmodel(game);
    let shoulder_camera = read_shoulder_camera(game);

    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.viewmodel = viewmodel;
        s.shoulder_camera.horizontal = shoulder_camera.horizontal;
        s.shoulder_camera.vertical = shoulder_camera.vertical;
        s.shoulder_camera.depth = shoulder_camera.depth;
        s.shoulder_camera.fov = shoulder_camera.fov;
        let target = s.edit_target;

        // 6: Section switcher
        buttons.push(format!(
            "▶ Section: [ {} ] (click to switch)",
            target.label()
        ));

        // 7..14: Dedicated [-] and [+] buttons for each parameter
        match target {
            EditTarget::MovesCamera => {
                for cp in moves::CameraParameter::ALL {
                    let val = match cp {
                        moves::CameraParameter::Horizontal => s.shoulder_camera.horizontal,
                        moves::CameraParameter::Vertical => s.shoulder_camera.vertical,
                        moves::CameraParameter::Depth => s.shoulder_camera.depth,
                        moves::CameraParameter::Fov => s.shoulder_camera.fov,
                    };
                    let step_str = match cp {
                        moves::CameraParameter::Fov => "5°",
                        moves::CameraParameter::Depth => "0.25",
                        _ => "0.10",
                    };
                    buttons.push(format!(
                        "[-] {}: {} (-{step_str})",
                        cp.label(),
                        cp.display(val)
                    ));
                    buttons.push(format!(
                        "[+] {}: {} (+{step_str})",
                        cp.label(),
                        cp.display(val)
                    ));
                }
            }
            EditTarget::Arms | EditTarget::Items => {
                let vm_target = if target == EditTarget::Arms {
                    Target::Arms
                } else {
                    Target::Items
                };
                for p in Parameter::ALL {
                    let val = s.viewmodel.get(vm_target).value(p);
                    let step_str = if p == Parameter::Scale {
                        "5%"
                    } else {
                        "0.5 px"
                    };
                    buttons.push(format!(
                        "[-] {}: {} (-{step_str})",
                        p.label(),
                        p.display(val)
                    ));
                    buttons.push(format!(
                        "[+] {}: {} (+{step_str})",
                        p.label(),
                        p.display(val)
                    ));
                }
            }
        }

        // 15: Reset current section
        buttons.push(format!("Reset {} to defaults", target.label()));

        // 16: Reset all
        buttons.push("Reset all mod settings to defaults".into());

        // 17: Reload
        buttons.push("Reload animations & resource packs (F12)".into());

        // 18: Diagnostics / status
        load(&mut s);
        buttons.push(s.error.clone().unwrap_or_else(|| {
            format!(
                "Status: {} | Press F9 in-game for Live HUD Editor",
                s.status
            )
        }));
    });
    buttons.push(format!(
        "Moves master: {}",
        if super::punchy_moves::enabled(game) {
            "ON"
        } else {
            "OFF"
        }
    )); // 19
    buttons.push(format!(
        "Moves body animations: {}",
        if game
            .native_setting(ID, "moves_animations_enabled")
            .as_deref()
            != Some("false")
        {
            "ON"
        } else {
            "OFF"
        }
    )); // 20
    buttons.push(format!(
        "Dynamic camera framing: {}",
        if game.native_setting(ID, "moves_dynamic_framing").as_deref() != Some("false") {
            "ON"
        } else {
            "OFF"
        }
    )); // 21
    buttons.push(super::punchy_moves::emote_label()); // 22
    buttons.push("Play selected emote (N)".into()); // 23
    buttons.push("Stop emote".into()); // 24
    for key in ["moves_camera_rot_yaw", "moves_camera_rot_pitch"] {
        let value = game
            .native_setting(ID, key)
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(0.)
            .clamp(-30., 30.);
        buttons.push(format!(
            "Camera {}: {value:+.0}° (left +1 / right -1)",
            if key.ends_with("yaw") { "yaw" } else { "pitch" }
        ));
    } // 25..26
    buttons.push(super::punchy_moves::status(game)); // 27
    super::punchy_faces::settings_buttons(game, buttons); // 28..37
    super::punchy_camera::settings_buttons(game, buttons); // 38..
}

pub(super) fn click(game: &Game<'_>, button: usize) {
    click_direction(game, button, false);
}
pub(super) fn click_direction(game: &Game<'_>, button: usize, back: bool) {
    if back && matches!(button,15..=18 | 23..=24 | 27) {
        return;
    }
    match button {
        0 => game.native_set_setting(ID, "enabled", if enabled(game) { "false" } else { "true" }),
        1 => STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.live_hud_active = !s.live_hud_active;
        }),
        2 => {
            let current =
                game.native_setting(ID, "moves_camera_enabled").as_deref() != Some("false");
            game.native_set_setting(
                ID,
                "moves_camera_enabled",
                if current { "false" } else { "true" },
            );
        }
        3 if super::punchy_moves::enabled(game) && read_shoulder_camera(game).enabled => {
            super::punchy_camera::toggle_orbit()
        }
        4 => game.native_set_setting(
            ID,
            "hand_poses",
            if game.native_setting(ID, "hand_poses").as_deref() == Some("horizon") {
                "punchy"
            } else {
                "horizon"
            },
        ),
        5 => {
            let speed = global_speed(game);
            game.native_set_setting(
                ID,
                "speed",
                &if back && speed <= 0.5 {
                    "5.5".into()
                } else if back {
                    format!("{:.1}", speed - 0.5)
                } else if speed >= 5.5 {
                    "0.5".into()
                } else {
                    format!("{:.1}", speed + 0.5)
                },
            );
        }
        6 => STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.edit_target = if back {
                s.edit_target.previous()
            } else {
                s.edit_target.next()
            };
        }),
        7..=14 => STATE.with(|s| {
            let mut s = s.borrow_mut();
            let target = s.edit_target;
            let param_idx = (button - 7) / 2;
            let increase = !back && (button - 7) % 2 == 1;
            step_target_param(game, &mut s, target, param_idx, increase);
        }),
        15 => STATE.with(|s| {
            let mut s = s.borrow_mut();
            let target = s.edit_target;
            reset_target_all(game, &mut s, target);
        }),
        16 => STATE.with(|s| {
            super::punchy_faces::reset_settings(game);
            super::punchy_camera_settings::reset_settings(game);
            let mut s = s.borrow_mut();
            for target in [Target::Arms, Target::Items] {
                for p in Parameter::ALL {
                    game.native_set_setting(ID, p.key(target), &p.default_value().to_string());
                }
            }
            for cp in moves::CameraParameter::ALL {
                game.native_set_setting(ID, cp.key(), &cp.default_value().to_string());
            }
            s.shoulder_camera.horizontal = moves::CameraParameter::Horizontal.default_value();
            s.shoulder_camera.vertical = moves::CameraParameter::Vertical.default_value();
            s.shoulder_camera.depth = moves::CameraParameter::Depth.default_value();
            s.shoulder_camera.fov = moves::CameraParameter::Fov.default_value();
            s.viewmodel = read_viewmodel(game);
            for (key, value) in [
                ("enabled", "true"),
                ("speed", "5.5"),
                ("hand_poses", "punchy"),
                ("moves_enabled", "true"),
                ("moves_camera_enabled", "true"),
                ("moves_animations_enabled", "true"),
                ("moves_dynamic_framing", "true"),
                ("moves_camera_rot_yaw", "0"),
                ("moves_camera_rot_pitch", "0"),
            ] {
                game.native_set_setting(ID, key, value);
            }
            s.shoulder_camera = Default::default();
            s.live_hud_active = false;
            super::punchy_moves::clear();
        }),
        17 => {
            STATE.with(|s| *s.borrow_mut() = State::default());
            super::punchy_moves::clear();
        }
        19..=21 => {
            let key = match button {
                19 => "moves_enabled",
                20 => "moves_animations_enabled",
                _ => "moves_dynamic_framing",
            };
            let current = game.native_setting(ID, key).as_deref() != Some("false");
            game.native_set_setting(ID, key, if current { "false" } else { "true" });
            if button == 19 {
                STATE.with(|s| s.borrow_mut().shoulder_camera.free_camera = false);
            }
        }
        22 => super::punchy_moves::select_emote(back),
        23 if !back => super::punchy_moves::play_emote(game),
        24 if !back => super::punchy_moves::stop_emote(),
        25..=26 => {
            let key = if button == 25 {
                "moves_camera_rot_yaw"
            } else {
                "moves_camera_rot_pitch"
            };
            let v = game
                .native_setting(ID, key)
                .and_then(|s| s.parse::<f32>().ok())
                .filter(|v| v.is_finite())
                .unwrap_or(0.);
            game.native_set_setting(
                ID,
                key,
                &(v + if back { -1. } else { 1. })
                    .clamp(-30., 30.)
                    .to_string(),
            );
        }
        28..=37 => super::punchy_faces::settings_click(game, button - 28, back),
        38..=50 => super::punchy_camera::settings_click(game, button - 38, back),
        _ => {}
    }
    super::punchy_camera::settings_changed(game);
}

pub(super) fn render_gui(
    game: &Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    gui_hidden: bool,
    _focused: bool,
) {
    if !enabled(game) || gui_hidden || game.screen != Screen::Playing || game.screen_over_playing()
    {
        return;
    }

    STATE.with(|s| {
        let s = s.borrow();
        if !s.live_hud_active {
            if super::punchy_camera::orbiting() {
                let text = "Punchy Orbit Mode: F4 to lock";
                let tw = game.native_text_width(text);
                let x = (batch.width - tw - 16.0).max(10.0);
                game.native_fill(batch, [x, 8.0, tw + 12.0, 16.0], 0xCC111827);
                game.native_text(batch, text, [x + 6.0, 12.0], 0xFCD34D, true);
            }
            return;
        }

        let w = 270.0_f32;
        let h = 144.0_f32;
        let x = 10.0_f32;
        let y = 10.0_f32;

        // Outer drop shadow
        game.native_fill(batch, [x - 1.0, y - 1.0, w + 2.0, h + 2.0], 0x88000000);
        // Card background (Dark slate)
        game.native_fill(batch, [x, y, w, h], 0xEE0F172A);
        // Header background
        game.native_fill(batch, [x, y, w, 20.0], 0xFF1E293B);
        // Cyan accent top border
        game.native_fill(batch, [x, y, w, 2.0], 0xFF38BDF8);

        // Header Title
        game.native_text(
            batch,
            "PUNCHY LIVE EDITOR",
            [x + 8.0, y + 6.0],
            0x38BDF8,
            true,
        );
        game.native_text(
            batch,
            "[F9 to close]",
            [x + w - 75.0, y + 6.0],
            0x94A3B8,
            true,
        );

        // Category Tab Row
        let cat_label = format!("< {} >", s.edit_target.label());
        game.native_text(
            batch,
            "Category [Tab]:",
            [x + 8.0, y + 25.0],
            0xCBD5E1,
            true,
        );
        game.native_text(batch, &cat_label, [x + 95.0, y + 25.0], 0xFCD34D, true);

        // Divider
        game.native_fill(batch, [x + 6.0, y + 38.0, w - 12.0, 1.0], 0x33FFFFFF);

        // 4 Parameters
        for i in 0..4 {
            let row_y = y + 42.0 + (i as f32 * 17.0);
            let (label, val_str) = get_target_param_display(&s, s.edit_target, i);
            let is_selected = i == s.hud_selected_param;

            if is_selected {
                // Highlight row background
                game.native_fill(batch, [x + 4.0, row_y - 1.0, w - 8.0, 16.0], 0x3338BDF8);
                // Active indicator
                game.native_text(batch, ">", [x + 6.0, row_y + 3.0], 0x38BDF8, true);
                game.native_text(batch, &label, [x + 16.0, row_y + 3.0], 0xFFFFFF, true);
                let val_w = game.native_text_width(&val_str);
                game.native_text(
                    batch,
                    &val_str,
                    [x + w - 12.0 - val_w, row_y + 3.0],
                    0x38BDF8,
                    true,
                );
            } else {
                game.native_text(batch, &label, [x + 16.0, row_y + 3.0], 0x94A3B8, true);
                let val_w = game.native_text_width(&val_str);
                game.native_text(
                    batch,
                    &val_str,
                    [x + w - 12.0 - val_w, row_y + 3.0],
                    0x64748B,
                    true,
                );
            }
        }

        // Bottom divider
        game.native_fill(batch, [x + 6.0, y + 112.0, w - 12.0, 1.0], 0x33FFFFFF);

        // Footer hints
        game.native_text(
            batch,
            "↑/↓ Select   ←/→ Adjust live",
            [x + 8.0, y + 116.0],
            0x94A3B8,
            true,
        );
        game.native_text(
            batch,
            "Bksp: Reset param   R: Reset group",
            [x + 8.0, y + 128.0],
            0x64748B,
            true,
        );
    });
}

// Authored skeletal delta calibrated against the same bone's rest transform.
// This preserves Horizon's item renderer and equipment coordinates; the full
// Punchy item mesh/physics/attachment renderer is a separate unfinished port.
fn bone_delta(s: &State, name: &str, alpha: f32) -> Mat4 {
    let Some(p) = &s.active else {
        return Mat4::IDENTITY;
    };
    let Some(base) = s.source.as_ref().and_then(|r| r.bones.get(name)) else {
        return Mat4::IDENTITY;
    };
    let time = if p.time < p.previous {
        p.time
    } else {
        p.previous + (p.time - p.previous) * alpha
    };
    let expression = p.previous_expression + (p.expression - p.previous_expression) * alpha;
    let sample = p.clip.sample(name, time, expression);
    let weight = p.previous_weight + (p.weight - p.previous_weight) * alpha;
    let sample = Pose::default().blend(sample, weight);
    let mut rest = base.matrix();
    rest.w_axis = (base.position / 16.).extend(1.);
    let posed = if p.clip.effect_active("use_keyframes_v2", time) {
        let rot = base.rotation + sample.euler;
        Mat4::from_scale_rotation_translation(
            sample.scale,
            Quat::from_euler(
                EulerRot::ZYX,
                rot.z.to_radians(),
                rot.y.to_radians(),
                rot.x.to_radians(),
            ),
            (base.position + sample.position) / 16.,
        )
    } else {
        rest * sample.matrix()
    };
    posed * rest.inverse()
}
fn hand_delta(s: &State, left: bool, alpha: f32) -> Mat4 {
    let Some(p) = &s.active else {
        return Mat4::IDENTITY;
    };
    let active_left = s.main_left != (p.hand == action::Hand::Off);
    if left != active_left
        && !punchy_engine::policy::dual_handed(&p.clip, p.time, s.offhand_item, s.offhand_item)
    {
        return Mat4::IDENTITY;
    }
    let side = if left { "left" } else { "right" };
    // Minecraft's authored model axes use X/Y opposite to first-person view.
    let flip = Mat4::from_scale(Vec3::new(-1., -1., 1.));
    flip * bone_delta(s, "camera", alpha)
        * bone_delta(s, "body", alpha)
        * bone_delta(s, &format!("{side}_arm"), alpha)
        * flip
}
pub(super) fn arms(game: &mut Game<'_>, alpha: f64, out: &mut Vec<Vertex>) {
    if !enabled(game)
        || game.spectator()
        || game.ui.hide_gui
        || game.sleeping.is_some()
        || !game.camera_type.first_person()
        || game.own_invisible()
        || game
            .player
            .as_ref()
            .is_none_or(|p| p.health.dead_or_dying())
        || game.screen == Screen::Dead
    {
        return;
    }
    let items = rendered_items(game);
    if items.iter().flatten().any(|s| maps::map_id(s).is_some()) {
        return;
    }
    let shown = game.hands_to_render();
    let a = alpha as f32;
    let eye = game.eye_position().unwrap_or_default();
    let light = game.world.light(
        eye[0].floor() as i32,
        eye[1].floor() as i32,
        eye[2].floor() as i32,
    );
    let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
    let world = game.sway.to_world(a, &game.view);
    // Extra authored arms must obey the same hide/poke hooks as the held-item
    // path. Otherwise Spatial GUI hides an item but leaves this second arm
    // renderer visible, or its reach gesture only moves the item.
    let mut arm_world = [world; 2];
    let mut canceled = [false; 2];
    let heights = game.heights.interpolated(a);
    for (i, hand) in Hand::BOTH.into_iter().enumerate() {
        let attack = if game.swing.attack_hand() == hand {
            game.swing.animation(a)
        } else {
            0.
        };
        let height = 1. - heights[i];
        game.native_render_hand(hand, items[i].as_ref(), a, attack, height, &mut canceled[i]);
        let (mut x, mut y, mut z, mut pitch, mut yaw, mut roll, mut scale) =
            (0., 0., 0., 0., 0., 0., 1.);
        game.native_render_hand_pose(
            hand,
            items[i].as_ref(),
            a,
            attack,
            height,
            &mut x,
            &mut y,
            &mut z,
            &mut pitch,
            &mut yaw,
            &mut roll,
            &mut scale,
        );
        arm_world[i] *= native_hooks::hand_pose_matrix([x, y, z, pitch, yaw, roll, scale]);
    }
    STATE.with(|cell| {
        let s = cell.borrow();
        if s.source.is_none() {
            return;
        }
        for (i, item) in items.iter().enumerate() {
            if canceled[i] {
                continue;
            }
            let dual = s.active.as_ref().is_some_and(|p| {
                punchy_engine::policy::dual_handed(&p.clip, p.time, s.offhand_item, s.offhand_item)
            });
            let hidden = i == 1
                && s.active.as_ref().is_some_and(|p| {
                    punchy_engine::policy::hide_empty_offhand(&p.clip, p.time, s.offhand_item)
                });
            if hidden {
                continue;
            }
            if (!shown[i] && !dual) || (item.is_none() && (i == 0 || !dual)) {
                continue;
            }
            let left = game.settings.main_hand_left != (i == 1);
            let m = arm_world[i] * arm_pose(left, 0., 0.);
            game.first_person_arm(left, m, packed, out);
        }
    });
}

pub(super) fn begin(game: &mut Game<'_>, alpha: f64) {
    STATE.with(|cell| {
        let mut s = cell.borrow_mut();
        s.enabled = enabled(game) && game.native_window();
        s.render_alpha = (alpha as f32).clamp(0., 1.);
        s.main_left = game.settings.main_hand_left;
        let deltas = [
            hand_delta(&s, false, s.render_alpha),
            hand_delta(&s, true, s.render_alpha),
        ];
        let active = if let Some(p) = &s.active {
            let left = s.main_left != (p.hand == action::Hand::Off);
            let dual = punchy_engine::policy::dual_handed(&p.clip, p.time, s.offhand_item, s.offhand_item);
            [!left || dual, left || dual]
        } else {
            [false, false]
        };
        let mut authored = [None, None];
        if s.enabled && game.native_setting(ID, "hand_poses").as_deref() != Some("horizon") {
            if let (Some(source), Some(tuning)) = (&s.source, &s.tuning) {
                if let Ok(skeleton) = punchy_engine::skeleton::Skeleton::new(&source.bones)
                {
                    for left in [false, true] {
                        let i = usize::from(left);
                        let h = usize::from(s.main_left != left);
                        let id = &s.last_items[h];
                        let owns = s.active.as_ref().is_some_and(|p| {
                            (s.main_left != (p.hand == action::Hand::Off)) == left
                                || punchy_engine::policy::dual_handed(&p.clip, p.time, s.offhand_item, s.offhand_item)
                        });
                        let animation = s.active.as_ref().map(|p| {
                            let t = if p.time < p.previous {
                                p.time
                            } else {
                                p.previous + (p.time - p.previous) * s.render_alpha
                            };
                            (&*p.clip, t, p.previous_expression+(p.expression-p.previous_expression)*s.render_alpha)
                        });
                        let weight = s.active.as_ref().map_or(1., |p| {
                            p.previous_weight + (p.weight - p.previous_weight) * s.render_alpha
                        });
                        let arm = tuning.arm_transform(id, left).unwrap_or_default();
                        let item = tuning.item_transform(id, left).unwrap_or_default();
                        let suppress_tuning = animation.is_some_and(|(c, t, _)| {
                            c.effect_active("disable_arm_tuning", t)
                                || (c.effect_active("use_keyframes_v2", t)
                                    && !c.effect_active("keep_arm_tuning", t))
                        });
                        let factor = if owns && suppress_tuning { 1. - weight } else { 1. };
                        let frame=s.active.as_ref().map_or_else(
                            ||punchy_engine::attachments::sample_frame(&skeleton,left,None,1.,true),
                            |p|p.frame(&skeleton,left,s.main_left,s.offhand_item,s.render_alpha));
                        let mut pose = punchy_engine::attachments::attach(
                            frame,
                            left,
                            &arm,
                            &item,
                            factor,
                            if owns { 1. - weight } else { 1. },
                            id.ends_with(":chest")
                                || id.ends_with(":piston")
                                || id.ends_with(":sticky_piston"),
                        );
                        let slim = game.player_model.as_ref().is_some_and(|p| p.slim);
                        if let Some(model) = game.res.player_arm(slim, left).as_ref() {
                            let base = Mat4::from_translation(
                                Vec3::new(model.pose[0], model.pose[1], model.pose[2]) / 16.,
                            ) * Mat4::from_rotation_z(if left { -0.1 } else { 0.1 });
                            pose.arm *= base.inverse();
                            authored[i] = Some(pose);
                        }
                    }
                }
            }
        }
        s.render_count=s.render_count.saturating_add(1);
        if let Some(path)=std::env::var_os("PUNCHY_RENDER_DIAGNOSTICS") {
            let value=serde_json::json!({"frames":s.render_count,"authored_poses":authored.iter().flatten().count(),"active":active,
                "arm_matrices":authored.map(|p|p.map(|p|s.viewmodel.arms.apply(p.arm).to_cols_array())),"item_matrices":authored.map(|p|p.map(|p|s.viewmodel.items.apply(p.item).to_cols_array())),"viewmodel":{"arms":[s.viewmodel.arms.offset.x,s.viewmodel.arms.offset.y,s.viewmodel.arms.offset.z,s.viewmodel.arms.scale],"items":[s.viewmodel.items.offset.x,s.viewmodel.items.offset.y,s.viewmodel.items.offset.z,s.viewmodel.items.scale]}});
            if let Ok(bytes)=serde_json::to_vec_pretty(&value){let _=std::fs::write(path,bytes);}
        }
        punchy_engine::horizon_pose::set_frame(punchy_engine::horizon_pose::Frame {
            authored,
            enabled: s.enabled,
            deltas,
            active,
        });
    });
}

pub(super) fn ready(game: &Game<'_>) -> bool {
    enabled(game) && STATE.with(|cell| cell.borrow().source.is_some())
}
pub(super) fn item_pose(
    left: bool,
    height: f32,
    attack: f32,
    pose: horizon_client::first_person::ItemPose,
) -> Mat4 {
    if let Some(pose) = punchy_engine::horizon_pose::authored(left) {
        return adjust_item(pose.item);
    }
    let swing = if punchy_engine::horizon_pose::suppress_swing(left) {
        0.
    } else {
        attack
    };
    adjust_item(punchy_engine::horizon_pose::transform(
        left,
        horizon_client::first_person::item_pose(left, height, swing, pose),
    ))
}
pub(super) fn arm_pose(left: bool, height: f32, attack: f32) -> Mat4 {
    if let Some(pose) = punchy_engine::horizon_pose::authored(left) {
        return adjust_arm(pose.arm);
    }
    let swing = if punchy_engine::horizon_pose::suppress_swing(left) {
        0.
    } else {
        attack
    };
    adjust_arm(punchy_engine::horizon_pose::transform(
        left,
        horizon_client::first_person::player_arm_pose(left, height, swing),
    ))
}

pub(super) fn adjust_arm(pose: Mat4) -> Mat4 {
    STATE.with(|s| s.borrow().viewmodel.arms.apply(pose))
}
pub(super) fn adjust_item(pose: Mat4) -> Mat4 {
    STATE.with(|s| s.borrow().viewmodel.items.apply(pose))
}
pub(super) fn authored(left: bool) -> bool {
    punchy_engine::horizon_pose::authored(left).is_some()
}

// Called only after a successful client-side interaction, including offhand.
pub(super) fn used(game: &Game<'_>, used_hand: Hand, success: bool) {
    super::punchy_moves::used(used_hand, success);
    if success && game.native_window() && enabled(game) {
        STATE.with(|s| s.borrow_mut().successful_use = Some(hand(used_hand)));
    }
}

thread_local! { static USE_BASE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
pub(super) struct UseGuard;
impl Drop for UseGuard {
    fn drop(&mut self) {
        USE_BASE.with(|s| s.set(false));
    }
}
pub(super) fn capture_use(game: &Game<'_>) -> Option<UseGuard> {
    if !game.native_window()
        || !(enabled(game) || super::punchy_moves::enabled(game))
        || USE_BASE.with(|s| s.replace(true))
    {
        None
    } else {
        Some(UseGuard)
    }
}

fn phase_hand(hand: action::Hand) -> phases::Hand {
    if hand == action::Hand::Main {
        phases::Hand::Main
    } else {
        phases::Hand::Off
    }
}
fn action_hand(hand: phases::Hand) -> action::Hand {
    if hand == phases::Hand::Main {
        action::Hand::Main
    } else {
        action::Hand::Off
    }
}
fn phase_playback(s: &State) -> phases::Playback {
    s.active
        .as_ref()
        .map_or_else(Default::default, |p| phases::Playback {
            clip: Some(p.clip.name.clone()),
            id: p.id,
            playing: !p.stopping,
            blending: p.stopping || p.weight < 1.,
            time: p.time,
            expression_time: p.expression,
            ..Default::default()
        })
}
fn phase_selection(
    s: &State,
    item: &str,
    kind: &str,
    hand: action::Hand,
    speed: f32,
) -> Option<phases::Selection> {
    let entry =
        s.tuning
            .as_ref()?
            .animation(item, kind, s.main_left != (hand == action::Hand::Off))?;
    let resolve = |name: &str| {
        let (_, clip) = entry.resolve(name, &s.clips)?;
        Some(phases::ClipSpec {
            name: clip.name.clone(),
            length: clip.length,
            speed_multiplier: punchy_engine::playback::manager_multiplier(&clip.name, speed, false),
            hold: clip.hold_on_last_frame,
        })
    };
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    item.hash(&mut hash);
    kind.hash(&mut hash);
    entry.raw.to_string().hash(&mut hash);
    Some(phases::Selection {
        entry_id: hash.finish(),
        start: resolve(entry.start.as_deref()?)?,
        looping: resolve(entry.looping.as_deref()?)?,
        end: resolve(entry.end.as_deref()?)?,
        skip_blend_in: entry.skip_blend_in,
        skip_blend_out: entry.skip_blend_out,
        stop_immediate: entry.stop_immediate,
    })
}
fn stack_token(s: &mut State, stack: Option<&ItemStack>, mining: bool) -> u64 {
    let Some(stack) = stack else {
        return 0;
    };
    let mut key = stack.clone();
    key.count = 1;
    if mining {
        key.damage = 0;
        if let Some(extra) = key.extra.as_mut() {
            extra.enchantments.clear();
            extra.other.retain(|(id, _)| id.starts_with("minecraft:"));
        }
    }
    if let Some(index) = s.stack_keys.iter().position(|k| *k == key) {
        return index as u64 + 1;
    }
    s.stack_keys.push(key);
    s.stack_keys.len() as u64
}
fn phase_effects(s: &mut State, effects: Vec<phases::Effect>) {
    for effect in effects {
        match effect {
            phases::Effect::ClearUseTracker => s.successful_use = None,
            phases::Effect::SetForcedStack { hand, token } => {
                s.forced_stack = token
                    .checked_sub(1)
                    .and_then(|i| s.stack_keys.get(i as usize))
                    .cloned()
                    .map(|v| (action_hand(hand).index(), v));
            }
            phases::Effect::ClearForcedStack => s.forced_stack = None,
            phases::Effect::ResetPoseImmediate => s.active = None,
            phases::Effect::SetLooping(value) => {
                if let Some(p) = &mut s.active {
                    p.looping = value;
                    p.released = !value;
                }
            }
            phases::Effect::SetSpeedMultiplier(value) => {
                if let Some(p) = &mut s.active {
                    p.multiplier = value;
                }
            }
            phases::Effect::SetBlendOutDurationNext(value) => {
                if let Some(p) = &mut s.active {
                    p.return_fade = value;
                }
            }
            // Source-hand and rendered-sync notifications are represented by the
            // playback hand and the forced stack at this renderer boundary.
            _ => {}
        }
    }
}
fn phase_tick(
    s: &mut State,
    input: &mut action::Input,
    items: &[Option<ItemStack>; 2],
    speed: f32,
    attack_down: bool,
) -> Option<(bool, phases::Request)> {
    s.tick += 1;
    let playback = phase_playback(s);
    let mining_selection =
        phase_selection(s, &s.last_items[0], "mining", action::Hand::Main, speed);
    let mining_stack = stack_token(s, items[0].as_ref(), true);
    let mining = s.mining.tick(&phases::MiningInput {
        tick: s.tick,
        raw_mining: input.mining,
        triggered: input.mining_trigger,
        attack_down,
        aiming_trackable_block: input.block_target,
        stack: mining_stack,
        selection: mining_selection.clone(),
        playback: playback.clone(),
        ..Default::default()
    });
    let using = input.using;
    if let Some(u) = using {
        s.consume_hand = u.hand;
    }
    let h = s.consume_hand.index();
    let kind = using.map(|u| u.kind);
    let consume_kind = match kind {
        Some(action::ItemKind::Eat) => Some("eating"),
        Some(action::ItemKind::Drink) => Some("drinking"),
        _ => None,
    };
    let selection = consume_kind
        .and_then(|kind| phase_selection(s, &s.last_items[h], kind, s.consume_hand, speed));
    let tokens = [
        stack_token(s, items[0].as_ref(), false),
        stack_token(s, items[1].as_ref(), false),
    ];
    let consume = s.consume.tick(&phases::ConsumeInput {
        use_down: input.use_down,
        using_item: using.is_some(),
        using_consume: consume_kind.is_some(),
        hand: phase_hand(s.consume_hand),
        using_stack: tokens[h],
        current_hand_stack: tokens[h],
        opposite_hand_stack: tokens[1 - h],
        animation_speed: speed,
        selection: selection.clone().map(phases::ConsumeSelection::Phases),
        playback: playback.clone(),
        ..Default::default()
    });
    if selection.is_some() {
        input.using = None;
        input.use_trigger = None;
    }
    if mining_selection.is_some() {
        input.mining_trigger = false;
        input.mining = false;
    }
    if s.consume.equip_blocked() {
        input.equip = None;
    }
    phase_effects(s, mining.effects);
    phase_effects(s, consume.effects);
    let request = consume
        .request
        .filter(|r| r.may_apply(&playback))
        .map(|r| (true, r))
        .or_else(|| {
            mining
                .request
                .filter(|r| r.may_apply(&playback))
                .map(|r| (false, r))
        });
    input.external_priority = request.as_ref().map(|(_, r)| r.priority);
    request
}
fn apply_phase(s: &mut State, consume: bool, request: &phases::Request, speed: f32) {
    let mut consume_tick = false;
    let play = match &request.command {
        phases::Command::Play(p) => Some(p.clone()),
        phases::Command::Handoff {
            clip,
            hand,
            speed_multiplier,
            tick_seconds,
            blend_seconds,
            fallback,
        } => {
            let playback = phase_playback(s);
            let Some(target) = s.clips.get(&clip.name) else {
                return;
            };
            let source_length = s.active.as_ref().map_or(0., |p| p.clip.length);
            let source_delta = s.active.as_ref().map_or(0., |p| {
                tick_seconds
                    * punchy_engine::playback::rate(&p.clip, p.time, speed, p.multiplier, 1.)
            });
            match phases::exact_handoff(
                &playback,
                source_length,
                source_delta,
                clip.length,
                *tick_seconds,
                *blend_seconds,
                |seconds| {
                    seconds
                        * punchy_engine::playback::rate(target, 0., speed, *speed_multiplier, 1.)
                },
            ) {
                phases::HandoffDecision::Wait => return,
                phases::HandoffDecision::Fallback => Some(fallback.clone()),
                phases::HandoffDecision::Start {
                    time,
                    expression_time,
                    consumes_tick,
                } => {
                    consume_tick = consumes_tick;
                    let mut p = fallback.clone();
                    p.clip = clip.clone();
                    p.hand = *hand;
                    p.looping = true;
                    p.hold = false;
                    p.speed_multiplier = *speed_multiplier;
                    p.blend_seconds = *blend_seconds;
                    p.start_time = time;
                    p.expression_time = expression_time;
                    p.stop_after_current = false;
                    Some(p)
                }
            }
        }
        phases::Command::FinishCurrent { release_hold } => {
            if let Some(p) = &mut s.active {
                p.looping = false;
                p.released = true;
                if *release_hold {
                    p.hold = false;
                }
            }
            None
        }
        phases::Command::StopImmediate { blend_seconds } => {
            if let Some(p) = &mut s.active {
                p.stopping = true;
                p.return_fade = blend_seconds.max(0.001);
            }
            None
        }
    };
    if let Some(play) = play {
        let Some(clip) = s.clips.get(&play.clip.name).cloned() else {
            return;
        };
        s.playback_id = s.playback_id.wrapping_add(1);
        let from = s.active.take().map(Box::new);
        s.active = Some(Playback {
            from,
            out_weight: 1.,
            previous_out_weight: 1.,
            id: s.playback_id,
            clip,
            hand: action_hand(play.hand),
            previous: play.start_time,
            time: play.start_time,
            expression: play.expression_time,
            previous_expression: play.expression_time,
            multiplier: play.speed_multiplier,
            looping: play.looping,
            released: play.stop_after_current,
            hold: play.hold,
            fade: play.blend_seconds.max(0.001),
            return_fade: play.return_blend_seconds.unwrap_or(0.28),
            skip_tick: consume_tick,
            stopping: false,
            weight: if play.blend_seconds <= 0. { 1. } else { 0. },
            previous_weight: if play.blend_seconds <= 0. { 1. } else { 0. },
        });
        let playback = phase_playback(s);
        if consume {
            s.consume.commit(request, &playback);
        } else {
            s.mining.commit(request, &playback, s.tick);
        }
    }
}

pub(super) fn rendered_items(game: &Game<'_>) -> [Option<ItemStack>; 2] {
    let mut items = game.visible_hand_items();
    STATE.with(|s| {
        if let Some((hand, stack)) = &s.borrow().forced_stack {
            items[*hand] = Some(stack.clone());
        }
    });
    items
}

// Capture the target before creative prediction removes the block. Source
// records start attempts too; relying only on Game.mining loses instant breaks.
pub(super) fn mining_attempt(game: &Game<'_>) {
    if game.native_window() && enabled(game) && !game.spectator() {
        STATE.with(|s| s.borrow_mut().mining_attempt = true);
    }
}

pub(super) fn camera_pose(
    game: &Game<'_>,
    partial: f32,
    position: &mut [f64; 3],
    yaw: &mut f32,
    pitch: &mut f32,
) {
    super::punchy_camera::pose(game, partial, position, yaw, pitch);
}
pub(super) fn compute_fov(game: &Game<'_>, _partial: f32, _camera: [f64; 3], degrees: &mut f32) {
    super::punchy_camera::fov(game, degrees);
}
pub(super) fn turn_player(game: &Game<'_>, xo: f64, yo: f64) -> std::ops::ControlFlow<()> {
    if super::punchy_camera::turn(game, xo, yo) {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
