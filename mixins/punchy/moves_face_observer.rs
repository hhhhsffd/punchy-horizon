// Frame observations for the facial solver. Source motion uses blocks/tick;
// FaceConditions velocities use blocks/second and event ages use seconds.
use super::Game;
use horizon_client::mobs::{Mob, anim::punchy_moves::engine::moves_expressions::FaceConditions};
use horizon_core::position::BlockPos;
use std::{cell::RefCell, collections::BTreeMap};

thread_local! {
    static USES:RefCell<Vec<(u64,String)>>=const {RefCell::new(Vec::new())};
}
pub(super) fn clear() {
    USES.with(|s| s.borrow_mut().clear());
}
pub(super) fn held_item(game: &Game<'_>, hand: super::Hand) -> String {
    let stack = if hand == super::Hand::Main {
        game.held()
    } else {
        game.offhand.as_ref()
    };
    stack.map_or_else(String::new, |stack| stack.item.to_string())
}
pub(super) fn used(game: &Game<'_>, item: &str, success: bool) {
    if !success || !super::punchy_faces::enabled(game) {
        return;
    }
    USES.with(|s| {
        let mut s = s.borrow_mut();
        s.push((game.ticks, item.into()));
        if s.len() > 32 {
            s.remove(0);
        }
    });
}
#[derive(Default)]
pub(super) struct Observation {
    last_time: Option<f32>,
    items: [String; 2],
    using: Option<String>,
    fishing: bool,
    xp: Option<i32>,
    scan_at: Option<f32>,
    environment: BTreeMap<String, f32>,
    events: BTreeMap<String, f32>,
}
impl Observation {
    pub(super) fn update(&mut self, game: &Game<'_>, m: &Mob, own: bool, c: &mut FaceConditions) {
        let time = c.time_seconds;
        if self.last_time.is_some_and(|at| time < at) {
            *self = Self::default();
        }
        let first = self.last_time.is_none();
        self.last_time = Some(time);
        let items = [
            m.extra.main_hand.as_deref().unwrap_or("").to_owned(),
            m.extra.off_hand.as_deref().unwrap_or("").to_owned(),
        ];
        let using =
            (m.extra.using_item > 0).then(|| items[usize::from(m.extra.using_off_hand)].clone());
        if !first && !c.cues.contains_key("age") {
            if self.items != items {
                self.events.insert("swap".into(), time);
                if items.iter().any(|s| !s.is_empty()) {
                    self.events.insert("equip".into(), time);
                }
            }
            if let Some(old) = self
                .using
                .as_deref()
                .filter(|_| using.is_none() && self.items == items)
            {
                if matches!(
                    old,
                    "minecraft:bow" | "minecraft:crossbow" | "minecraft:trident"
                ) {
                    self.events.insert(
                        if old == "minecraft:trident" {
                            "hurl"
                        } else {
                            "release"
                        }
                        .into(),
                        time,
                    );
                }
            }
        }
        if own {
            let fishing = game.cast_rod().is_some();
            if !first && fishing != self.fishing {
                self.events
                    .insert(if fishing { "cast" } else { "reel" }.into(), time);
            }
            self.fishing = fishing;
            let xp = game.experience.level;
            if self.xp.is_some_and(|old| xp > old) {
                self.events.insert("levelup".into(), time);
            }
            self.xp = Some(xp);
            USES.with(|s| {
                for (tick, item) in s.borrow_mut().drain(..) {
                    let age = game.ticks.saturating_sub(tick) as f32 / 20.;
                    let at = time - age;
                    let event = match item.as_str() {
                        "minecraft:flint_and_steel" => "ignite",
                        "minecraft:shears" => "shear",
                        "minecraft:ender_pearl" => "pearl",
                        "minecraft:splash_potion" | "minecraft:lingering_potion" => "brew",
                        "minecraft:egg" | "minecraft:snowball" | "minecraft:experience_bottle" => {
                            "toss"
                        }
                        "minecraft:firework_rocket" if c.gliding => "boost",
                        _ => "interact",
                    };
                    if matches!(event, "toss" | "pearl" | "brew") {
                        for kind in ["toss", "pearl", "brew"] {
                            self.events.remove(kind);
                            c.events.remove(kind);
                        }
                    }
                    self.events.insert(event.into(), at);
                }
            });
        }
        self.items = items;
        self.using = using;
        self.events.retain(|_, at| time - *at < 5.);
        for (event, at) in &self.events {
            c.events.insert(event.clone(), (time - *at).max(0.));
        }
        if self.scan_at.is_none_or(|last| time - last >= 0.5) {
            self.environment = surroundings(game, m, &self.environment);
            self.scan_at = Some(time);
            if self.environment.get("lightning") == Some(&1.) {
                self.events.insert("flash".into(), time);
                c.events.insert("flash".into(), 0.);
            }
        }
        c.cues.extend(self.environment.clone());
        c.rain = c.cue("rain") > 0.;
        // Fast-changing signals never wait for the surroundings scan.
        let flag = |c: &mut FaceConditions, key: &str, value: bool| {
            c.cues.insert(key.into(), if value { 1. } else { 0. });
        };
        flag(c, "frozen", m.ticks_frozen >= 140);
        flag(c, "freezing", m.ticks_frozen > 0);
        flag(c, "burning", m.fire_ticks > 0);
        flag(c, "riptide", m.extra.auto_spin_attack);
        flag(c, "invisible", m.is_invisible());
        flag(c, "glowing", m.is_currently_glowing());
        for (cue, ids) in [
            ("blind", &["blindness", "darkness"][..]),
            ("nauseous", &["nausea"][..]),
            ("poisoned", &["poison"][..]),
            ("withering", &["wither"][..]),
            ("levitating", &["levitation"][..]),
            ("boosted", &["speed", "strength", "haste", "jump_boost"][..]),
        ] {
            let active = ids.iter().any(|id| {
                if own {
                    game.potions.effects.contains_key(*id)
                } else {
                    m.has_effect(id) || m.has_effect(&format!("minecraft:{id}"))
                }
            });
            flag(c, cue, active);
        }
        flag(
            c,
            "spyglass",
            self.using.as_deref() == Some("minecraft:spyglass"),
        );
        flag(
            c,
            "tooting",
            self.using.as_deref() == Some("minecraft:goat_horn"),
        );
        flag(
            c,
            "brushing",
            self.using.as_deref() == Some("minecraft:brush"),
        );
        flag(
            c,
            "loading",
            self.using.as_deref() == Some("minecraft:crossbow"),
        );
        flag(
            c,
            "charging_throw",
            self.using
                .as_deref()
                .is_some_and(|s| s == "minecraft:trident" || s.ends_with("_spear")),
        );
        let studying = c.head_pitch > 10.
            && self.items.iter().any(|s| {
                matches!(
                    s.as_str(),
                    "minecraft:filled_map"
                        | "minecraft:book"
                        | "minecraft:written_book"
                        | "minecraft:writable_book"
                        | "minecraft:enchanted_book"
                        | "minecraft:knowledge_book"
                )
            });
        flag(c, "studying", studying);
        flag(
            c,
            "holding_light",
            self.items.iter().any(|id| {
                game.pack
                    .assets
                    .registry
                    .default_state(id)
                    .is_some_and(|s| game.res.facts().get(s).emission >= 10)
            }),
        );
        let delta = if own {
            game.player
                .as_ref()
                .map_or(m.position.subtract(m.previous), |p| {
                    p.position.subtract(p.old_position)
                })
        } else {
            m.position.subtract(m.previous)
        };
        let speed = delta.x.hypot(delta.z) as f32;
        let fast = c.flying && speed > 0.5;
        let slamming = self.items[0] == "minecraft:mace"
            && !c.on_ground
            && !c.gliding
            && c.vertical_speed < -6.;
        flag(c, "fast_flight", fast);
        flag(c, "slamming", slamming);
        let yaw = m.body_yaw.to_radians();
        c.cues.insert(
            "forward_velocity".into(),
            ((delta.z * f64::from(yaw.cos()) - delta.x * f64::from(yaw.sin())) * 20.) as f32,
        );
        c.vertical_speed = (delta.y * 20.) as f32;
        c.gliding_speed = (delta.length() * 20.) as f32;
        let vehicle = if own { game.riding } else { m.extra.mount };
        let kind = vehicle.map_or(0., |id| {
            if game.boats.iter().any(|b| b.id == id) {
                return 1.;
            }
            if game.carts.iter().any(|cart| cart.id == id) {
                return 3.;
            }
            game.mobs
                .iter()
                .find(|mob| mob.id == id)
                .map_or(7., |mob| match &*mob.kind.id {
                    "minecraft:horse"
                    | "minecraft:donkey"
                    | "minecraft:mule"
                    | "minecraft:camel"
                    | "minecraft:skeleton_horse"
                    | "minecraft:zombie_horse" => 2.,
                    "minecraft:pig" | "minecraft:strider" => 4.,
                    "minecraft:happy_ghast" => 5.,
                    "minecraft:nautilus" | "minecraft:zombie_nautilus" => 6.,
                    _ => 7.,
                })
        });
        c.cues.insert("vehicle".into(), kind);
        let vehicle_delta = vehicle.and_then(|id| {
            game.mobs
                .iter()
                .find(|mob| mob.id == id)
                .map(|mob| mob.position.subtract(mob.previous))
                .or_else(|| {
                    game.boats
                        .iter()
                        .find(|boat| boat.id == id)
                        .map(|boat| boat.position.subtract(boat.previous))
                })
                .or_else(|| {
                    game.carts
                        .iter()
                        .find(|cart| cart.id == id)
                        .map(|cart| cart.position.subtract(cart.previous))
                })
        });
        c.cues.insert(
            "vehicle_speed".into(),
            vehicle_delta.map_or(0., |delta| (delta.x.hypot(delta.z) * 20.) as f32),
        );
        c.riding = vehicle.is_some();
        let id = if own {
            game.player.as_ref().map_or(m.id, |p| p.entity_id as u32)
        } else {
            m.id
        };
        flag(
            c,
            "carrying",
            game.mobs.iter().any(|other| other.extra.mount == Some(id)),
        );
        if own {
            flag(c, "biting", super::fishing::punchy_fishing::biting(game));
        }
        if own {
            flag(c, "mining", game.mining.is_some());
            if !c.cues.contains_key("map_view") {
                flag(
                    c,
                    "map_view",
                    game.pitch >= 55. && self.items.iter().any(|s| s == "minecraft:filled_map"),
                );
            }
            flag(
                c,
                "music",
                game.jukebox_sounds.keys().any(|&(x, y, z)| {
                    (m.position.x - f64::from(x)).powi(2)
                        + (m.position.y - f64::from(y)).powi(2)
                        + (m.position.z - f64::from(z)).powi(2)
                        <= 144.
                }),
            );
        }
    }
}
fn side(dx: f64, dz: f64, yaw: f32, previous: f32) -> f32 {
    let angle = ((-dx).atan2(dz).to_degrees() - f64::from(yaw) + 180.).rem_euclid(360.) - 180.;
    let side = if angle > 0. { -1. } else { 1. };
    let same = previous == side;
    if angle.abs() < if same { 8. } else { 15. } || angle.abs() > if same { 175. } else { 170. } {
        0.
    } else {
        side
    }
}
fn surroundings(game: &Game<'_>, m: &Mob, old: &BTreeMap<String, f32>) -> BTreeMap<String, f32> {
    let mut out = BTreeMap::new();
    let mut flag = |key: &str, value: bool| {
        out.insert(key.to_owned(), if value { 1. } else { 0. });
    };
    let eye = m.position.y + m.eye_height();
    let pos = BlockPos::new(
        m.position.x.floor() as i32,
        m.position.y.floor() as i32,
        m.position.z.floor() as i32,
    );
    let eye_pos = BlockPos::new(pos.x, eye.floor() as i32, pos.z);
    let sky = game.world.light(eye_pos.x, eye_pos.y, eye_pos.z) >> 4 >= 15;
    flag("rain", game.raining_at(eye_pos));
    let below = game
        .res
        .facts()
        .get(game.world.block(pos.x, pos.y - 1, pos.z));
    flag("on_fence", below.name.ends_with("_fence"));
    let biome = game.native_biome([pos.x, pos.y, pos.z]).unwrap_or_default();
    flag(
        "flowery",
        matches!(
            biome.as_str(),
            "minecraft:cherry_grove"
                | "minecraft:flower_forest"
                | "minecraft:meadow"
                | "minecraft:sunflower_plains"
        ),
    );
    flag(
        "eerie",
        matches!(
            biome.as_str(),
            "minecraft:dark_forest"
                | "minecraft:pale_garden"
                | "minecraft:deep_dark"
                | "minecraft:soul_sand_valley"
        ),
    );
    let dim = game.dim;
    flag("nether", dim == super::Dim::Nether);
    flag("end", dim == super::Dim::End);
    flag("deep", dim == super::Dim::Overworld && pos.y < 0 && !sky);
    if let Some(chunk) = game.world.chunks.get(&(pos.x >> 4, pos.z >> 4)) {
        let biomes = game
            .world_biomes
            .as_deref()
            .unwrap_or_else(|| game.res.biomes());
        let biome = biomes.get(chunk.biome(pos.x, pos.y, pos.z));
        flag("frosty", sky && biome.temperature_at(pos, 63) <= 0.15);
        flag(
            "hot",
            dim == super::Dim::Nether || sky && biome.temperature >= 1.5,
        );
    }
    let forward =
        horizon_client::mobs::anim::punchy_moves::engine::moves::ShoulderCamera::basis_vectors(
            m.head_yaw,
            m.head_pitch,
        )
        .0
        .as_dvec3();
    let from = glam::DVec3::new(m.position.x, eye, m.position.z);
    flag(
        "over_edge",
        m.on_ground
            && m.head_pitch
                >= if old.get("over_edge") == Some(&1.) {
                    30.
                } else {
                    35.
                }
            && drop_ahead(game, m.position, forward) >= 6,
    );
    flag(
        "lightning",
        game.bolts.iter().any(|bolt| {
            (bolt.at[0] - m.position.x).abs() <= 32.
                && (bolt.at[1] - m.position.y).abs() <= 32.
                && (bolt.at[2] - m.position.z).abs() <= 32.
        }),
    );
    flag("campfire", campfire_near(game, pos));
    let mut hostile = None;
    let mut hostile_d = if old.get("hostile_near") == Some(&1.) {
        64.
    } else {
        36.
    };
    let mut creeper = false;
    let mut warden = false;
    let mut watched = None;
    let mut adore = false;
    let adore_cos = if old.get("adoring") == Some(&1.) {
        40_f64
    } else {
        30_f64
    }
    .to_radians()
    .cos();
    let watch_cos = if old.get("watched") == Some(&1.) {
        12_f64
    } else {
        8_f64
    }
    .to_radians()
    .cos();
    for other in game
        .mobs
        .iter()
        .filter(|other| other.id != m.id && !other.removed && other.alive())
    {
        let dx = other.position.x - m.position.x;
        let dy = other.position.y - m.position.y;
        let dz = other.position.z - m.position.z;
        let d = dx * dx + dy * dy + dz * dz;
        if other.kind.category == "monster" && d <= hostile_d {
            hostile = Some((dx, dz));
            hostile_d = d;
        }
        if &*other.kind.id == "minecraft:creeper"
            && other.swell as f32 / (other.max_swell - 2).max(1) as f32 > 0.1
            && d <= if old.get("creeper_near") == Some(&1.) {
                81.
            } else {
                49.
            }
        {
            creeper = true;
        }
        if &*other.kind.id == "minecraft:warden"
            && d <= if old.get("warden_near") == Some(&1.) {
                400.
            } else {
                256.
            }
        {
            warden = true;
        }
        let owner = game
            .player_model
            .as_ref()
            .filter(|local| local.id == m.id)
            .map(|_| game.acting_uuid())
            .or_else(|| game.persist.uuids.get(&m.id).copied());
        if (other.is_baby()
            || other.extra.tame && other.extra.owner.is_some() && other.extra.owner == owner)
            && other.kind.category != "monster"
            && dx.abs() <= 4.5
            && dy.abs() <= 5.
            && dz.abs() <= 4.5
        {
            let delta = glam::DVec3::new(
                other.position.x,
                other.position.y + f64::from(other.kind.height) * 0.5,
                other.position.z,
            ) - from;
            if delta.length() > 0.1
                && delta.length() <= 5.
                && forward.dot(delta.normalize()) >= adore_cos
            {
                adore = true;
            }
        }
        if &*other.kind.id == "minecraft:player"
            && !other.invisible
            && !other.extra.spectator
            && d <= 576.
        {
            let delta = from
                - glam::DVec3::new(
                    other.position.x,
                    other.position.y + other.eye_height(),
                    other.position.z,
                );
            let gaze=horizon_client::mobs::anim::punchy_moves::engine::moves::ShoulderCamera::basis_vectors(other.head_yaw,other.head_pitch).0.as_dvec3();
            if watched.is_none()
                && delta.length() > 0.5
                && delta.length() <= 24.
                && gaze.dot(delta.normalize()) >= watch_cos
            {
                watched = Some((dx, dz));
            }
        }
    }
    flag("hostile_near", hostile.is_some());
    flag("creeper_near", creeper);
    flag("warden_near", warden);
    flag("watched", watched.is_some());
    flag("adoring", adore);
    out.insert(
        "hostile_side".into(),
        hostile.map_or(0., |(x, z)| {
            side(x, z, m.head_yaw, *old.get("hostile_side").unwrap_or(&0.))
        }),
    );
    out.insert(
        "watcher_side".into(),
        watched.map_or(0., |(x, z)| {
            side(x, z, m.head_yaw, *old.get("watcher_side").unwrap_or(&0.))
        }),
    );
    out
}
fn drop_ahead(game: &Game<'_>, at: horizon_core::geometry::Vec3, forward: glam::DVec3) -> u32 {
    let length = forward.x.hypot(forward.z);
    if length < 0.001 {
        return 0;
    }
    let x = (at.x + forward.x / length * 1.2).floor() as i32;
    let z = (at.z + forward.z / length * 1.2).floor() as i32;
    let y = (at.y - 0.5).floor() as i32;
    (0..=6)
        .take_while(|dy| {
            game.res
                .facts()
                .is_air(game.world.block(x, y - *dy as i32, z))
        })
        .count() as u32
}
fn campfire_near(game: &Game<'_>, at: BlockPos) -> bool {
    for dx in -4_i32..=4 {
        for dy in -4_i32..=4 {
            for dz in -4_i32..=4 {
                if dx * dx + dy * dy + dz * dz > 12 {
                    continue;
                }
                let facts = game
                    .res
                    .facts()
                    .get(game.world.block(at.x + dx, at.y + dy, at.z + dz));
                if matches!(
                    &*facts.name,
                    "minecraft:campfire" | "minecraft:soul_campfire"
                ) && facts.emission > 0
                {
                    return true;
                }
            }
        }
    }
    false
}
