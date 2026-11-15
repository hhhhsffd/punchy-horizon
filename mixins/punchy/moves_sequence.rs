//! Independently owned clip channels and the item-sequence state machine.
use super::{
    Frame, Input, Layer, condition, expr_number, matches_id,
    motion::{self, STEP, State},
    number,
};
use crate::moves_pack::{self, Pack};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

pub fn declared(pack: &Pack, id: &str, action: &str) -> bool {
    let group = moves_pack::item_group(id);
    let group = if group == "EMPTY" {
        &pack.config["empty_hand"]
    } else {
        &pack.config["item_groups"][group]
    };
    if !group["animations"][action].is_null() {
        return true;
    }
    pack.config["items"]
        .as_object()
        .into_iter()
        .flat_map(|v| v.iter())
        .any(|(selector, definition)| {
            matches_id(selector, id) && !definition["animations"][action].is_null()
        })
}
pub fn item_entry(pack: &Pack, id: &str, action: &str) -> Value {
    pack.entry_for_item(id, action)
}
pub fn phase_entry(pack: &Pack, id: &str, phase: &str) -> Value {
    let explicit = format!("sequence_{phase}");
    let mut entry = if declared(pack, id, &explicit) {
        item_entry(pack, id, &explicit)
    } else if declared(pack, id, phase) {
        item_entry(pack, id, phase)
    } else {
        return Value::Null;
    };
    let definition = pack.definition(id);
    let mut settings = definition["sequence"]["defaults"].clone();
    if settings.is_null() {
        settings = serde_json::json!({});
    }
    if let Some(sequence) = definition["sequence"].as_object() {
        for (key, value) in sequence {
            if !matches!(key.as_str(), "behavior" | "defaults") {
                settings[key] = value.clone();
            }
        }
    }
    moves_pack::merge(&mut settings, &entry);
    entry = settings;
    entry
}

pub fn resolved_entry(entry: &Value, one: bool, variant: usize) -> Value {
    let mut entry = if entry.is_string() {
        serde_json::json!({"clip":entry})
    } else {
        entry.clone()
    };
    if let Some(variants) = entry["variants"].as_array().filter(|a| !a.is_empty()) {
        let selected = variants[variant % variants.len()].clone();
        if selected.is_string() {
            entry["clip"] = selected;
        } else {
            moves_pack::merge(&mut entry, &selected);
        }
    }
    if one && !entry["one_handed"].is_null() {
        let selected = entry["one_handed"].clone();
        if selected.is_string() {
            entry["clip"] = selected;
        } else {
            moves_pack::merge(&mut entry, &selected);
        }
        if !entry["one_handed_overlays"].is_null() {
            entry["overlays"] = entry["one_handed_overlays"].clone();
        }
    }
    if let Some(object) = entry.as_object_mut() {
        object.remove("variants");
        if one {
            object.remove("one_handed");
            object.remove("one_handed_overlays");
        }
    }
    entry
}

/// Overlay ownership belongs to its parent sequence; it must not become a
/// second full-strength action after a fading parent.
#[allow(
    clippy::too_many_arguments,
    reason = "Explicit animation and world inputs keep the pure solver independent of the game"
)]
pub fn resolve(
    pack: &Pack,
    entry: &Value,
    key: &str,
    left: bool,
    one: bool,
    looping: bool,
    context: &BTreeMap<String, f64>,
    variant: usize,
) -> Vec<Layer> {
    if entry.is_null() {
        return vec![];
    }
    let entry = resolved_entry(entry, one, variant);
    if !condition(
        entry.get("when").or_else(|| entry.get("condition")),
        context,
        true,
    ) {
        return vec![];
    }
    let name = entry[key].as_str().or_else(|| entry["clip"].as_str());
    let Some(clip) = name.and_then(|name| pack.resolve(name, left)) else {
        return vec![];
    };
    let mirror = left && !(clip.name.ends_with("_left") || clip.name.ends_with("_left_alt"));
    let mut layer = Layer::new(clip);
    layer.looping = looping && layer.clip.looped;
    layer.hold = entry["hold_last_frame"]
        .as_bool()
        .unwrap_or(layer.clip.hold_on_last_frame);
    layer.additive = entry["additive"].as_bool().unwrap_or(false);
    layer.mirror = mirror;
    layer.visual_left = left;
    layer.weight = expr_number(
        entry.get("weight").or_else(|| entry.get("blending_effect")),
        context,
        1.,
    )
    .clamp(0., 16.);
    layer.retain = entry["base_pose_retain"].clone();
    layer.default_retain = number(&entry["locomotion_retain"], 0.).clamp(0., 1.);
    layer.bones = entry["bones"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .take(128)
        .collect();
    layer.rate = timer_rate(&entry, context);
    if entry["hold_last_frame"] == true
        || entry["start_at_end"] == true
        || entry["start_at_end"].as_f64().is_some_and(|n| n > 0.5)
    {
        layer.time = layer.clip.length;
        layer.previous_time = layer.time;
    }
    for overlay in entry["overlays"].as_array().into_iter().flatten().take(32) {
        if !condition(
            overlay.get("when").or_else(|| overlay.get("condition")),
            context,
            true,
        ) {
            continue;
        }
        let name = overlay.as_str().or_else(|| overlay["clip"].as_str());
        let Some(clip) = name.and_then(|name| pack.resolve(name, left)) else {
            continue;
        };
        let mut addition = Layer::new(clip);
        addition.mirror = left
            && !(addition.clip.name.ends_with("_left")
                || addition.clip.name.ends_with("_left_alt"));
        addition.looping = addition.clip.looped;
        addition.additive = overlay["mode"] == "additive";
        addition.exclusive = overlay["mode"] == "exclusive";
        addition.weight = expr_number(overlay.get("weight"), context, 1.).clamp(0., 1.);
        addition.bones = overlay["bones"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .take(128)
            .collect();
        addition.rate = layer.rate;
        if addition.additive {
            layer.overlays.push(addition);
        } else {
            // ClipSequencePlayer merges replacement tracks before sampling.
            // This preserves waist inheritance, the parent's duration/loop,
            // and later replacement overlays winning over earlier ones.
            let mut merged = (*layer.clip).clone();
            for (bone, track) in &addition.clip.tracks {
                if addition.bones.is_empty() || addition.bones.iter().any(|b| b == bone) {
                    merged.tracks.insert(bone.clone(), track.clone());
                }
            }
            layer.clip = Arc::new(merged);
        }
    }
    vec![layer]
}

pub fn timer_rate(entry: &Value, context: &BTreeMap<String, f64>) -> f32 {
    if !condition(entry.get("timer_condition"), context, true) {
        return 0.;
    }
    (expr_number(entry.get("timer_modifier"), context, 1.) * number(&entry["timer_scale"], 1.))
        .clamp(0., 100.)
}

pub fn retention(layers: &mut [Layer], entry: &Value, i: &Input, fallback: f32, moving_legs: bool) {
    let movement = motion::smooth((i.speed - 0.001) / 0.007);
    let influence = &entry["locomotion_influence"];
    let key = if !i.grounded {
        "jump"
    } else if i.sprinting {
        "sprint"
    } else {
        "walk"
    };
    let selected = number(&influence[key], fallback).clamp(0., 1.);
    let leg_influence = number(&entry["moving_legs_influence"], 1.).clamp(0., 1.);
    let suppressed = number(&entry["disable_locomotion_arm_swing"], 0.) as u32;
    for layer in layers {
        // Explicit retention belongs to the playback that was started with it,
        // including the outgoing side of an interrupted sequence transition.
        let authored_retain = layer.retain.clone();
        layer.default_retain = number(&entry["locomotion_retain"], selected).clamp(0., 1.);
        let mut retain = serde_json::json!({});
        for bone in [
            "root",
            "waist",
            "body",
            "head",
            "right_arm",
            "left_arm",
            "right_leg",
            "left_leg",
        ] {
            let authored = if layer.visual_left || layer.mirror {
                super::opposite_bone(bone)
            } else {
                bone.to_owned()
            };
            let arm = if bone == "left_arm" {
                1
            } else if bone == "right_arm" {
                2
            } else {
                0
            };
            let is_hand = bone
                == if layer.visual_left {
                    "left_arm"
                } else {
                    "right_arm"
                };
            let hand_only = influence["hand_only"] == true;
            let mut value = if !hand_only || is_hand {
                selected
            } else {
                fallback
            };
            if moving_legs && bone.ends_with("_leg") {
                value = number(
                    &entry["legs_locomotion_retain"],
                    value + (1. - value) * movement,
                )
                .clamp(0., 1.);
            }
            if suppressed & arm != 0 {
                value = 0.;
            }
            let retain_bone = if layer.retain_world_space {
                bone
            } else {
                &authored
            };
            if let Some(explicit) = authored_retain.get(retain_bone) {
                value = number(explicit, value);
            } else if matches!(bone, "body" | "head" | "left_arm" | "right_arm")
                && let Some(explicit) = authored_retain.get("waist")
            {
                value = number(explicit, value);
            }
            retain[bone] = serde_json::json!(value.clamp(0., 1.));
        }
        layer.retain = retain;
        layer.retain_world_space = true;
        if moving_legs {
            layer.leg_weight = 1. - movement * (1. - leg_influence);
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Playback {
    pub current: Vec<Layer>,
    outgoing: Option<Box<Playback>>,
    previous_elapsed: f32,
    elapsed: f32,
    duration: f32,
    pub age: f32,
    pub length: f32,
    pub looping: bool,
    pub holding: bool,
    pub cycle: u64,
}
impl Playback {
    pub fn duration_override(&mut self, seconds: f32) {
        self.duration = motion::finite(seconds, 0.16).clamp(0., 5.);
        self.elapsed = self.elapsed.min(self.duration);
        if self.duration == 0. {
            self.outgoing = None;
        }
    }
    pub fn active(&self) -> bool {
        !self.current.is_empty() || self.outgoing.is_some()
    }
    pub fn has_clip(&self) -> bool {
        !self.current.is_empty()
    }
    pub fn finished(&self) -> bool {
        self.current.is_empty() || !self.looping && self.age >= self.length
    }
    pub fn progress(&self) -> f32 {
        if self.current.is_empty() {
            1.
        } else {
            self.age / self.length.max(0.001)
        }
    }
    pub fn play(&mut self, mut layers: Vec<Layer>, seconds: f32, hold_untracked: bool) {
        let snapshot = self.emit(0);
        if hold_untracked && !layers.is_empty() {
            let frame = Frame {
                layers: snapshot.clone(),
                ..Default::default()
            };
            let mut held = BTreeMap::new();
            for bone in super::MODEL_BONES {
                if !layers.iter().any(|l| l.owns(bone))
                    && let Some((pose, _)) = frame.bone(bone, 1.)
                {
                    held.insert(bone.to_owned(), pose);
                }
            }
            layers[0].held = held;
        }
        // Retarget the moving source, not a flattened snapshot. Its interrupted
        // crossfade and every outgoing clock must continue to advance.
        self.commit();
        self.outgoing = self.active().then(|| Box::new(self.clone()));
        self.length = layers.iter().map(|l| l.clip.length).fold(0., f32::max);
        self.looping = layers.iter().any(|l| l.looping);
        self.holding = layers.iter().any(|l| l.hold);
        self.age = layers.first().map_or(0., |l| l.time);
        self.current = layers;
        self.elapsed = 0.;
        self.previous_elapsed = 0.;
        self.duration = motion::finite(seconds, 0.16).clamp(0., 5.);
        self.cycle = self.cycle.wrapping_add(1);
        if self.duration == 0. {
            self.outgoing = None;
        }
    }
    pub fn stop(&mut self, seconds: f32) {
        if !self.current.is_empty() {
            self.play(vec![], seconds, false);
        }
    }
    pub fn seek(&mut self, seconds: f32) {
        let seconds = motion::finite(seconds, 0.).clamp(0., self.length.max(0.));
        self.age = seconds;
        for layer in &mut self.current {
            layer.time = seconds;
        }
    }
    pub fn rate(&mut self, rate: f32) {
        for layer in &mut self.current {
            layer.rate = motion::finite(rate, 1.).clamp(-100., 100.);
            for overlay in &mut layer.overlays {
                overlay.rate = layer.rate;
            }
        }
    }
    pub fn hold_at(&mut self, time: f32) {
        if self.age >= time {
            self.age = time;
            for l in &mut self.current {
                l.time = time;
                l.previous_time = l.previous_time.min(time);
                l.rate = 0.;
            }
        }
    }
    pub fn tick(&mut self) {
        let rate = self.current.first().map_or(1., |l| l.rate.abs());
        self.age = (self.age + STEP * rate).min(86400.);
        self.previous_elapsed = self.elapsed;
        self.elapsed = (self.elapsed + STEP).min(self.duration);
        for layer in &mut self.current {
            layer.advance();
        }
        if let Some(outgoing) = &mut self.outgoing {
            outgoing.tick();
        }
        // Keep the source through the final interpolated render interval.
        if self.previous_elapsed >= self.duration {
            self.outgoing = None;
        }
    }
    fn commit(&mut self) {
        self.previous_elapsed = self.elapsed;
        for layer in &mut self.current {
            layer.previous_time = layer.time;
            for overlay in &mut layer.overlays {
                overlay.previous_time = overlay.time;
            }
        }
        if let Some(outgoing) = &mut self.outgoing {
            outgoing.commit();
        }
    }
    pub fn emit(&self, group: u16) -> Vec<Layer> {
        let edge = (self.duration > 0.).then(|| {
            [
                self.previous_elapsed / self.duration,
                self.elapsed / self.duration,
                1.,
            ]
        });
        let mut out = Vec::new();
        if let Some(outgoing) = &self.outgoing {
            for mut layer in outgoing.emit(group) {
                if let Some(mut edge) = edge {
                    edge[2] = -1.;
                    layer.fades.push(edge);
                }
                out.push(layer);
            }
        }
        for layer in &self.current {
            let mut layer = layer.clone();
            layer.group = group;
            if let Some(edge) = edge {
                layer.fades.push(edge);
            }
            out.push(layer);
        }
        out
    }
}

#[derive(Clone, Debug, Default)]
pub struct Channel {
    pub playback: Playback,
    pub key: String,
    pub entry: Value,
}
impl Channel {
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit animation and world inputs keep the pure solver independent of the game"
    )]
    pub fn set(
        &mut self,
        pack: &Pack,
        key: &str,
        entry: &Value,
        field: &str,
        left: bool,
        one: bool,
        looped: bool,
        context: &BTreeMap<String, f64>,
    ) {
        if self.key == key {
            let rate = timer_rate(entry, context);
            self.playback.rate(rate);
            return;
        }
        let layers = resolve(pack, entry, field, left, one, looped, context, 0);
        self.playback.play(
            layers,
            number(&entry["transition_seconds"], 0.16),
            entry["hold_untracked"] == true,
        );
        self.key = key.into();
        self.entry = entry.clone();
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit animation and world inputs keep the pure solver independent of the game"
    )]
    pub fn trigger(
        &mut self,
        pack: &Pack,
        entry: &Value,
        field: &str,
        left: bool,
        one: bool,
        context: &BTreeMap<String, f64>,
        variant: usize,
        force: bool,
    ) -> bool {
        let entry = resolved_entry(entry, one, variant);
        if !force
            && self.playback.has_clip()
            && self.playback.progress()
                < number(&entry["retrigger_progress"], 50.).clamp(0., 100.) / 100.
        {
            return false;
        }
        let layers = resolve(pack, &entry, field, left, false, false, context, 0);
        if layers.is_empty() {
            return false;
        }
        self.playback.play(
            layers,
            number(&entry["transition_seconds"], 0.1),
            entry["hold_untracked"] == true,
        );
        self.entry = entry;
        true
    }
    pub fn stop(&mut self, seconds: f32) {
        self.key.clear();
        self.playback.stop(seconds);
    }
    pub fn tick(&mut self, expire: bool) {
        self.playback.tick();
        if expire && self.playback.finished() && !self.playback.holding && !self.playback.looping {
            self.stop(number(&self.entry["fade_out_seconds"], 0.12));
        }
    }
    pub fn emit(&self, group: u16, i: &Input, fallback: f32, moving_legs: bool) -> Vec<Layer> {
        let mut layers = self.playback.emit(group);
        retention(&mut layers, &self.entry, i, fallback, moving_legs);
        layers
    }
}

#[derive(Clone, Debug, Default)]
pub struct ItemSequence {
    pub channel: Channel,
    pub phase: String,
    pub behavior: String,
    pub hand: usize,
    pub slam_started: bool,
    owner: String,
    slot: u8,
    initialized: bool,
    was_using: bool,
    hook: bool,
    other_occupied: bool,
    left: bool,
    confirmed: bool,
    attack_index: usize,
    last_attack: u64,
    use_ticks: u32,
    movement: f32,
    retain: f32,
    target_retain: f32,
    legs_influence: f32,
    target_legs_influence: f32,
    tick: u64,
    pending_attack: Option<(bool, f32)>,
    pending_age: u8,
}
impl ItemSequence {
    pub fn behavior(pack: &Pack, id: &str) -> String {
        pack.definition(id)["sequence"]["behavior"]
            .as_str()
            .unwrap_or("")
            .to_ascii_lowercase()
    }
    pub fn replaces_falling(&self, i: &Input) -> bool {
        self.hand == 0
            && self.behavior == "mace"
            && matches!(self.phase.as_str(), "charge" | "slam")
            && (self.phase != "slam" || !i.grounded)
            && self.channel.playback.current.iter().any(|l| {
                ["root", "left_leg", "right_leg"]
                    .iter()
                    .any(|b| l.clip.tracks.contains_key(*b))
            })
    }
    fn entry(&self, pack: &Pack, phase: &str) -> Value {
        phase_entry(pack, &self.owner, phase)
    }
    fn enter(
        &mut self,
        pack: &Pack,
        phase: &str,
        looped: bool,
        context: &BTreeMap<String, f64>,
        fast: bool,
    ) {
        let mut phase = phase.to_owned();
        if self.other_occupied
            && (phase == "idle"
                || phase == "equip"
                || phase == "use"
                || phase.starts_with("attack"))
        {
            let alternative = format!("{phase}_one_handed");
            if !self.entry(pack, &alternative).is_null() {
                phase = alternative;
            }
        }
        let mut entry = self.entry(pack, &phase);
        if entry.is_null() && phase.starts_with("attack") {
            entry = item_entry(pack, &self.owner, "attack");
        }
        // Variant settings (not just the selected clip) own transition,
        // retention and overlays, e.g. the zero-blend second trident attack.
        entry = resolved_entry(
            &entry,
            self.other_occupied,
            self.attack_index.saturating_sub(1),
        );
        let mut layers = resolve(pack, &entry, "clip", self.left, false, looped, context, 0);
        self.target_retain = number(&entry["locomotion_retain"], 0.15).clamp(0., 1.);
        self.target_legs_influence = number(&entry["moving_legs_influence"], 1.).clamp(0., 1.);
        if entry["over_idle"] == true && !phase.starts_with("idle") && !phase.starts_with("equip") {
            let base_entry = self.entry(pack, "idle");
            let idle = resolve(
                pack,
                &base_entry,
                "clip",
                self.left,
                self.other_occupied,
                false,
                context,
                0,
            );
            if let Some(base) = idle.first() {
                for layer in &mut layers {
                    layer.base = Some(base.clip.clone());
                }
            }
        }
        if phase == "hold" && entry["hold_last_frame"] == true {
            for layer in &mut layers {
                layer.time = layer.clip.length;
                layer.previous_time = layer.time;
                layer.hold = true;
            }
        }
        self.channel.playback.play(
            layers,
            number(&entry["transition_seconds"], 0.16).min(if fast { 0.08 } else { 5. }),
            phase == "hold",
        );
        self.channel.entry = entry;
        self.phase = phase;
        self.confirmed = false;
    }
    fn ready(&self, pack: &Pack) -> bool {
        matches!(
            self.phase.as_str(),
            "idle" | "idle_one_handed" | "fishing" | "charge"
        ) || self.phase.starts_with("equip")
            || self.channel.playback.progress()
                >= number(
                    &self.channel.entry["retrigger_progress"],
                    number(
                        &pack.definition(&self.owner)["sequence"]["defaults"]["retrigger_progress"],
                        70.,
                    ),
                )
                .clamp(0., 100.)
                    / 100.
    }
    pub fn suspend(&mut self) {
        self.channel.stop(0.18);
        self.phase.clear();
        self.was_using = false;
        self.pending_attack = None;
        self.channel.playback.tick();
    }
    pub fn tick(
        &mut self,
        pack: &Pack,
        i: &Input,
        state: State,
        context: &BTreeMap<String, f64>,
        emote: bool,
    ) {
        self.tick = self.tick.wrapping_add(1);
        if (i.attack || i.attack_presses > 0) && !i.external_attack {
            self.pending_attack = Some((i.grounded, i.fall_distance));
            self.pending_age = 0;
        } else {
            self.pending_age = self.pending_age.saturating_add(1);
            if self.pending_age > 24 {
                self.pending_attack = None;
            }
        }
        self.slam_started = false;
        if !self.initialized {
            self.retain = 0.15;
            self.target_retain = 0.15;
            self.legs_influence = 1.;
            self.target_legs_influence = 1.;
        }
        self.movement += (motion::smooth((i.speed - 0.001) / 0.007) - self.movement) * 0.4;
        self.legs_influence += (self.target_legs_influence - self.legs_influence) * 0.4;
        self.retain += (self.target_retain - self.retain) * 0.3;
        let allowed = state.ordinary() && !i.external_attack && !emote;
        let hands_only = state.hands_only();
        let accepted_hand = i.used.filter(|h| *h < 2);
        let use_event = i.use_event.as_deref().unwrap_or("");
        let accepted_behavior = accepted_hand
            .map(|hand| Self::behavior(pack, i.accepted_item.as_deref().unwrap_or(&i.items[hand])));
        let accepted = accepted_behavior
            .as_deref()
            .is_some_and(|behavior| match behavior {
                "bow" | "trident" => use_event == "release",
                "fishing" | "flint" | "shears" | "bucket" | "throwable" => true,
                _ => false,
            });
        // A confirmed accepted use wins over a concurrently held use button;
        // non-sequence use (food/shield) must not steal sequence ownership.
        let selected_using = i
            .using
            .filter(|(h, _)| *h < 2 && !accepted && !Self::behavior(pack, &i.items[*h]).is_empty());
        let continuing_offhand = self.hand == 1
            && !i.attack
            && !i.force_equip
            && self.slot == i.selected_slot
            && (self.was_using && matches!(self.phase.as_str(), "charge" | "hold")
                || !self.channel.playback.finished()
                    && (self.phase == "release"
                        || self.phase.starts_with("use")
                        || matches!(self.phase.as_str(), "cast" | "reel" | "fishing")))
            && (i.items[1] == self.owner
                || i.items[1].is_empty()
                || self.behavior == "bucket" && Self::behavior(pack, &i.items[1]) == "bucket");
        let selected = selected_using
            .map(|v| v.0)
            .or(accepted_hand.filter(|_| accepted))
            .unwrap_or(if continuing_offhand { 1 } else { 0 });
        let id = if accepted {
            i.accepted_item.as_deref().unwrap_or(&i.items[selected])
        } else if continuing_offhand && selected_using.is_none() {
            &self.owner
        } else {
            &i.items[selected]
        }
        .to_owned();
        let behavior = Self::behavior(pack, &id);
        let continuous = hands_only
            && matches!(behavior.as_str(), "bow" | "trident")
            && (selected_using.is_some() || self.was_using);
        let riding = i.riding.is_some();
        if !allowed
            && !accepted
            && !continuous
            && !riding
            && !(self.confirmed
                && self.slot == i.selected_slot
                && !self.channel.playback.finished())
        {
            self.suspend();
            return;
        }
        let bucket = behavior == "bucket"
            && self.behavior == "bucket"
            && selected == self.hand
            && self.slot == i.selected_slot
            && !i.force_equip;
        let consumed = self.confirmed
            && !self.channel.playback.finished()
            && i.items[self.hand].is_empty()
            && self.slot == i.selected_slot
            && !i.force_equip
            && accepted_hand.is_none();
        let changed = (id != self.owner
            || selected != self.hand
            || behavior != self.behavior
            || self.slot != i.selected_slot)
            && !consumed;
        let occupied = !i.items[1 - selected].is_empty();
        let left = i.main_left != (selected == 1);
        if changed && bucket && !accepted {
            self.owner = id.clone();
            if !self.phase.starts_with("use") || self.channel.playback.finished() {
                self.enter(pack, "idle", true, context, false);
            }
        } else if changed {
            let first = !self.initialized;
            self.initialized = true;
            self.owner = id.clone();
            self.hand = selected;
            self.behavior = behavior;
            self.slot = i.selected_slot;
            self.left = left;
            self.other_occupied = occupied;
            self.was_using = false;
            self.attack_index = 0;
            self.phase.clear();
            if self.behavior.is_empty() {
                self.channel.stop(0.18);
            } else if !first && !accepted {
                self.enter(pack, "equip", false, context, false);
            }
        } else if i.force_equip && !self.behavior.is_empty() && !accepted {
            self.enter(pack, "equip", false, context, false);
        }
        if self.behavior.is_empty() {
            self.hook = i.fishing;
            self.channel.playback.tick();
            return;
        }
        let handedness_changed = self.left != left;
        let occupied_changed = self.other_occupied != occupied;
        self.left = left;
        self.other_occupied = occupied;
        if handedness_changed
            || occupied_changed && matches!(self.phase.as_str(), "charge" | "hold" | "fishing")
        {
            let progress = self.channel.playback.progress();
            let phase = self.phase.clone();
            let looping = matches!(
                phase.as_str(),
                "idle" | "idle_one_handed" | "hold" | "fishing"
            );
            let confirmed = self.confirmed;
            self.enter(pack, &phase, looping, context, false);
            self.confirmed = confirmed;
            if phase != "hold" {
                self.channel
                    .playback
                    .seek(progress * self.channel.playback.length);
            }
        }
        let using = selected_using.is_some_and(|(h, _)| h == self.hand) && !accepted;
        if matches!(self.behavior.as_str(), "bow" | "trident") {
            if using {
                self.use_ticks = i.use_ticks.unwrap_or(if self.was_using {
                    self.use_ticks.saturating_add(1)
                } else {
                    0
                });
                if !self.was_using {
                    self.enter(pack, "charge", false, context, false);
                }
                if self.phase == "charge" {
                    let ticks = number(
                        &pack.definition(&self.owner)["sequence"]["charge_ticks"],
                        20.,
                    )
                    .clamp(1., 72000.);
                    let charge = motion::unit(self.use_ticks as f32 / ticks);
                    self.channel
                        .playback
                        .seek(charge * self.channel.playback.length);
                    if charge >= 1. && !self.entry(pack, "hold").is_null() {
                        self.enter(pack, "hold", true, context, false);
                    }
                }
            } else if self.was_using || accepted && use_event == "release" {
                let minimum = number(
                    &pack.definition(&self.owner)["sequence"]["minimum_charge_ticks"],
                    3.,
                )
                .max(0.);
                let released = accepted && use_event == "release"
                    || i.remote && self.use_ticks as f32 >= minimum;
                self.enter(
                    pack,
                    if released { "release" } else { "idle" },
                    false,
                    context,
                    released,
                );
                self.confirmed = released;
            }
            self.was_using = using;
        }
        if allowed && self.pending_attack.is_some() && !accepted && self.hand == 0 && !using {
            let (on_ground, fall_distance) = self.pending_attack.unwrap();
            let sequence = pack.definition(&self.owner)["sequence"].clone();
            if self.behavior == "mace"
                && !on_ground
                && fall_distance > number(&sequence["smash_distance"], 1.5)
            {
                self.enter(pack, "slam", false, context, false);
                self.slam_started = true;
                self.pending_attack = None;
            } else if self.ready(pack) {
                if self.tick.saturating_sub(self.last_attack) > 30 {
                    self.attack_index = 0;
                }
                self.attack_index = self.attack_index.wrapping_add(1);
                let phase = if matches!(self.behavior.as_str(), "flint" | "shears")
                    || self.behavior == "throwable" && self.entry(pack, "attack").is_null()
                {
                    "use"
                } else {
                    "attack"
                };
                self.enter(pack, phase, false, context, false);
                self.last_attack = self.tick;
                self.pending_attack = None;
            }
        }
        if allowed
            && self.hand == 0
            && self.behavior == "mace"
            && self.phase != "slam"
            && !self.phase.starts_with("attack")
        {
            let height = number(
                &pack.definition(&self.owner)["sequence"]["charge_height"],
                3.,
            )
            .max(0.);
            let falling = !i.grounded
                && i.vertical_speed < 0.
                && (self.phase == "charge"
                    || i.ground_clearance.unwrap_or(i.fall_distance) > height);
            if falling && self.phase != "charge" {
                self.enter(pack, "charge", false, context, false);
            } else if !falling && self.phase == "charge" {
                self.enter(pack, "idle", true, context, false);
            }
        }
        if accepted && accepted_hand == Some(self.hand) {
            let phase = if self.behavior == "fishing" {
                if use_event == "reel" || self.hook && use_event != "cast" {
                    "reel"
                } else {
                    "cast"
                }
            } else if matches!(
                self.behavior.as_str(),
                "flint" | "shears" | "bucket" | "throwable"
            ) {
                "use"
            } else {
                ""
            };
            if !phase.is_empty() {
                self.enter(pack, phase, false, context, true);
                self.confirmed = true;
            }
        }
        if self.behavior == "fishing" && !accepted {
            if i.fishing && !self.hook && self.phase != "cast" {
                self.enter(pack, "cast", false, context, false);
            }
            if !i.fishing && self.hook && self.phase != "reel" {
                self.enter(pack, "reel", false, context, false);
            }
        }
        if self.channel.playback.finished() && !using && self.phase != "charge" {
            let phase = if self.behavior == "fishing" && i.fishing {
                "fishing"
            } else {
                "idle"
            };
            if self.phase != phase && self.phase != "idle_one_handed" {
                self.enter(pack, phase, true, context, false);
            }
        }
        if self.phase.is_empty() {
            self.enter(
                pack,
                if self.behavior == "fishing" && i.fishing {
                    "fishing"
                } else {
                    "idle"
                },
                true,
                context,
                false,
            );
        }
        if matches!(self.phase.as_str(), "idle" | "idle_one_handed") {
            let idle = if occupied && !self.entry(pack, "idle_one_handed").is_null() {
                "idle_one_handed"
            } else {
                "idle"
            };
            if self.phase != idle {
                self.enter(pack, idle, true, context, false);
            } else if occupied_changed {
                // Inline one_handed definitions also change the idle clip.
                self.enter(pack, idle, true, context, false);
            }
        }
        if riding
            && (self.phase.starts_with("idle")
                || self.phase.starts_with("equip")
                || self.phase == "fishing")
        {
            self.channel.playback.stop(0.28);
        }
        self.hook = i.fishing;
        self.channel.playback.tick();
        if using && self.phase == "charge" && matches!(self.behavior.as_str(), "bow" | "trident") {
            let ticks = number(
                &pack.definition(&self.owner)["sequence"]["charge_ticks"],
                20.,
            )
            .clamp(1., 72000.);
            self.channel
                .playback
                .seek(motion::unit(self.use_ticks as f32 / ticks) * self.channel.playback.length);
        }
    }
    pub fn emit(&self, group: u16, i: &Input, state: State) -> Vec<Layer> {
        let mut layers = self.channel.playback.emit(group);
        let mut entry = self.channel.entry.clone();
        if entry.is_null() {
            return layers;
        }
        if entry["locomotion_influence"].is_null() {
            entry["locomotion_retain"] = serde_json::json!(self.retain);
        }
        retention(&mut layers, &entry, i, self.retain, true);
        for layer in &mut layers {
            layer.leg_weight = 1. - self.movement * (1. - self.legs_influence);
            if state.hands_only() {
                layer.restrict_to_hands();
            } else if i.riding.is_some() || !state.ordinary() && self.confirmed {
                layer
                    .exclude
                    .extend(["root", "left_leg", "right_leg"].map(str::to_owned));
            } else if !state.ordinary() {
                layer.weight = 0.;
            }
        }
        layers
    }
}

#[derive(Clone, Debug, Default)]
pub struct Crouch {
    pub channel: Channel,
    phase: String,
    initialized: bool,
    requested: bool,
    single: bool,
}
impl Crouch {
    pub fn tick(&mut self, pack: &Pack, i: &Input, allowed: bool, context: &BTreeMap<String, f64>) {
        let entry = pack.entry("GENERAL", "crouch");
        let single = entry["additive"] == true;
        if self.single != single {
            *self = Self::default();
        }
        self.single = single;
        if !allowed {
            *self = Self::default();
            return;
        }
        let first = !self.initialized;
        self.initialized = true;
        let requested = i.crouching;
        let play = if single {
            if requested && (first || !self.requested) {
                Some(if entry["enter"].is_null() {
                    "pose"
                } else {
                    "enter"
                })
            } else {
                None
            }
        } else if requested {
            if first || self.phase == "enter" && self.channel.playback.finished() {
                Some(if entry["pose"].is_null() {
                    "enter"
                } else {
                    "pose"
                })
            } else if !self.requested {
                Some(
                    if entry["enter"].is_null() || entry["enter"] == entry["pose"] {
                        "pose"
                    } else {
                        "enter"
                    },
                )
            } else {
                None
            }
        } else if self.requested && entry["exit"].is_string() {
            Some("exit")
        } else {
            None
        };
        if let Some(field) = play {
            let holding = !single
                && requested
                && (first
                    || self.phase == "enter" && self.channel.playback.finished()
                    || field == "pose");
            let layers = resolve(pack, &entry, field, false, false, holding, context, 0);
            self.channel.playback.play(layers, 0.1, false);
            self.channel.entry = entry.clone();
            self.phase = if holding { "hold" } else { field }.into();
            if holding && !self.channel.playback.looping {
                self.channel.playback.seek(self.channel.playback.length);
            }
        } else if !requested
            && (self.requested || self.phase == "exit" && self.channel.playback.finished())
        {
            self.channel.stop(0.125);
            self.phase.clear();
        }
        self.requested = requested;
        self.channel.playback.rate(
            if self.phase == "hold" && (single || !self.channel.playback.looping) {
                0.
            } else {
                number(&entry["timer_scale"], 1.)
            },
        );
        self.channel.playback.tick();
        if single && requested && self.channel.playback.finished() {
            self.phase = "hold".into();
            if !self.channel.playback.looping {
                self.channel.playback.seek(self.channel.playback.length);
                self.channel.playback.rate(0.);
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    use crate::moves_animation::Use;
    use std::sync::Arc;
    fn pack() -> Pack {
        let mut p = Pack::default();
        p.clips = crate::load_model_clips(br#"{"animations":{"charge":{"animation_length":1,"bones":{"right_arm":{"rotation":[30,0,0]}}},"release":{"animation_length":0.3,"bones":{"right_arm":{"rotation":[90,0,0]}}},"hold":{"loop":true,"animation_length":1,"bones":{"right_arm":{"rotation":[45,0,0]}}}}}"#)
            .unwrap().into_iter().map(|(n,c)|(n,Arc::new(c))).collect();
        p.config = serde_json::json!({"item_groups":{"BOW":{"sequence":{"behavior":"bow","charge_ticks":20,"minimum_charge_ticks":3},"animations":{"charge":{"clip":"charge"},"hold":{"clip":"hold"},"release":{"clip":"release"}}}}});
        p
    }
    #[test]
    fn bow_charge_tracks_use_ticks_and_releases_only_when_accepted() {
        let p = pack();
        let mut s = ItemSequence::default();
        let mut i = Input {
            grounded: true,
            items: ["minecraft:bow".into(), String::new()],
            using: Some((0, Use::Other)),
            use_ticks: Some(10),
            ..Default::default()
        };
        s.tick(&p, &i, State::Ground, &BTreeMap::new(), false);
        assert_eq!(s.phase, "charge");
        assert!((s.channel.playback.age - 0.5).abs() < 0.001);
        i.use_ticks = Some(20);
        s.tick(&p, &i, State::Ground, &BTreeMap::new(), false);
        assert_eq!(s.phase, "hold");
        i.using = None;
        i.used = Some(0);
        i.use_event = Some("release".into());
        s.tick(&p, &i, State::Ground, &BTreeMap::new(), false);
        assert_eq!(s.phase, "release");
    }
    #[test]
    fn transition_preserves_total_ownership() {
        let p = pack();
        let ctx = BTreeMap::new();
        let a = resolve(
            &p,
            &serde_json::json!({"clip":"charge"}),
            "clip",
            false,
            false,
            false,
            &ctx,
            0,
        );
        let b = resolve(
            &p,
            &serde_json::json!({"clip":"release"}),
            "clip",
            false,
            false,
            false,
            &ctx,
            0,
        );
        let mut playback = Playback::default();
        playback.play(a, 0., false);
        playback.tick();
        playback.play(b, 0.2, false);
        playback.tick();
        let frame = Frame {
            layers: playback.emit(0),
            ..Default::default()
        };
        let (_, weight) = frame.bone("right_arm", 1.).unwrap();
        assert!((weight - 1.).abs() < 0.001);
    }
    #[test]
    fn per_variant_one_hand_overrides_apply_after_variant_selection() {
        let entry = serde_json::json!({"clip":"base","variants":[{"clip":"a","one_handed":{"clip":"one","transition_seconds":0.3}}]});
        let result = resolved_entry(&entry, true, 0);
        assert_eq!(result["clip"], "one");
        assert_eq!(result["transition_seconds"], 0.3);
    }
    #[test]
    fn interrupted_crossfades_keep_moving_and_interpolate_the_render_interval() {
        let p = pack();
        let make = |name| vec![Layer::new(p.resolve(name, false).unwrap())];
        let mut playback = Playback::default();
        playback.play(make("charge"), 0., false);
        playback.tick();
        playback.play(make("release"), 0.2, false);
        playback.tick();
        let before = Frame {
            layers: playback.emit(0),
            ..Default::default()
        }
        .bone("right_arm", 1.)
        .unwrap()
        .0
        .euler
        .x;
        playback.play(make("hold"), 0.2, false);
        playback.tick();
        let frame = Frame {
            layers: playback.emit(0),
            ..Default::default()
        };
        assert!((frame.bone("right_arm", 0.).unwrap().0.euler.x - before).abs() < 0.001);
        // The inner charge -> release fade has advanced to 50%, not frozen
        // at the retarget snapshot's 15.625%.
        assert!((frame.bone("right_arm", 1.).unwrap().0.euler.x - 57.65625).abs() < 0.001);
        assert!((frame.bone("right_arm", 0.5).unwrap().1 - 1.).abs() < 0.001);
    }
    #[test]
    fn held_sequence_starts_at_end_but_additive_overlay_has_its_own_clock() {
        let p = pack();
        let layers = resolve(
            &p,
            &serde_json::json!({
                "clip":"charge", "hold_last_frame":true,
                "overlays":[{"clip":"release","mode":"additive"}]
            }),
            "clip",
            false,
            false,
            true,
            &BTreeMap::new(),
            0,
        );
        let mut playback = Playback::default();
        playback.play(layers, 0., false);
        assert_eq!(playback.current[0].time, 1.);
        assert_eq!(playback.current[0].overlays[0].time, 0.);
        playback.tick();
        assert!((playback.current[0].overlays[0].time - STEP).abs() < 0.0001);
    }
    #[test]
    fn replacement_overlay_uses_parent_duration_and_later_tracks_win() {
        let p = pack();
        let layers = resolve(
            &p,
            &serde_json::json!({
                "clip":"charge", "overlays":[
                    {"clip":"release","mode":"exclusive","bones":["right_arm"]},
                    {"clip":"hold","mode":"exclusive","bones":["right_arm"]}
                ]
            }),
            "clip",
            false,
            false,
            false,
            &BTreeMap::new(),
            0,
        );
        assert_eq!(layers[0].clip.length, 1.);
        assert!(layers[0].overlays.is_empty());
        assert!((layers[0].pose_at("right_arm", 1.).unwrap().euler.x - 45.).abs() < 0.001);
    }
    #[test]
    fn accepted_consumed_offhand_use_overrides_other_hand_using_and_keeps_owner() {
        let mut p = pack();
        p.config["item_groups"]["THROWABLE"] = serde_json::json!({
            "sequence":{"behavior":"throwable"},
            "animations":{"use":{"clip":"release"}}
        });
        let mut sequence = ItemSequence::default();
        let mut i = Input {
            grounded: true,
            items: ["minecraft:bow".into(), String::new()],
            using: Some((0, Use::Other)),
            used: Some(1),
            accepted_item: Some("minecraft:ender_pearl".into()),
            ..Default::default()
        };
        sequence.tick(&p, &i, State::Ground, &BTreeMap::new(), false);
        assert_eq!(sequence.hand, 1);
        assert_eq!(sequence.phase, "use");
        assert!(sequence.confirmed);
        i.using = None;
        i.used = None;
        i.accepted_item = None;
        sequence.tick(&p, &i, State::Ground, &BTreeMap::new(), false);
        assert_eq!(sequence.hand, 1);
        assert_eq!(sequence.owner, "minecraft:ender_pearl");
        assert_eq!(sequence.phase, "use");
    }
    #[test]
    fn attack_variant_settings_control_transition_and_replacement_overlays() {
        let mut p = pack();
        p.config["item_groups"]["TRIDENT"] = serde_json::json!({
            "sequence":{"behavior":"trident"},
            "animations":{"attack":{
                "retrigger_progress":0,"transition_seconds":0.2,
                "overlays":[{"clip":"hold","mode":"exclusive"}],
                "variants":["charge",{"clip":"release","transition_seconds":0,"overlays":[]}]
            }}
        });
        let mut sequence = ItemSequence::default();
        let mut i = Input {
            grounded: true,
            attack: true,
            items: ["minecraft:trident".into(), String::new()],
            ..Default::default()
        };
        sequence.tick(&p, &i, State::Ground, &BTreeMap::new(), false);
        assert_eq!(sequence.channel.entry["transition_seconds"], 0.2);
        i.attack = true;
        sequence.tick(&p, &i, State::Ground, &BTreeMap::new(), false);
        assert_eq!(sequence.channel.playback.current[0].clip.name, "release");
        assert_eq!(sequence.channel.playback.duration, 0.);
        assert!(sequence.channel.playback.outgoing.is_none());
    }
    #[test]
    fn crouch_enter_hands_off_to_looping_pose_instead_of_holding_enter() {
        let mut p = pack();
        p.config["defaults"]["animations"]["crouch"] = serde_json::json!({
            "enter":"release","pose":"hold","exit":"charge"
        });
        let mut crouch = Crouch::default();
        let mut i = Input {
            grounded: true,
            ..Default::default()
        };
        let context = BTreeMap::new();
        crouch.tick(&p, &i, true, &context);
        i.crouching = true;
        for _ in 0..10 {
            crouch.tick(&p, &i, true, &context);
        }
        assert_eq!(crouch.phase, "hold");
        assert_eq!(crouch.channel.playback.current[0].clip.name, "hold");
        assert!(crouch.channel.playback.looping);
        assert!(crouch.channel.playback.current[0].time > 0.);
        crouch.tick(&p, &i, false, &context);
        assert!(!crouch.channel.playback.active());
    }
}
