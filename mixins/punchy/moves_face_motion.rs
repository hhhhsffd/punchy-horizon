//! Expression channels, reaction arbitration, deterministic blink/gaze and easing.
use super::{FaceSettings, finite};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const CHANNEL_NAMES: [&str; 21] = [
    "lid",
    "slant",
    "lower",
    "smile",
    "wide",
    "look_x",
    "look_y",
    "swing",
    "tremble",
    "brow_right",
    "brow_left",
    "tilt",
    "blush",
    "tears",
    "sweat",
    "bags",
    "anger",
    "sparkle",
    "wink_right",
    "wink_left",
    "bob",
];
pub const LOOK_NAMES: [&str; 40] = [
    "neutral",
    "happy",
    "joyful",
    "excited",
    "sparkly",
    "love",
    "proud",
    "smug",
    "relaxed",
    "zen",
    "wink",
    "sly",
    "sneaky",
    "curious",
    "focused",
    "squint",
    "determined",
    "furious",
    "menacing",
    "strain",
    "surprised",
    "shocked",
    "scared",
    "nervous",
    "worried",
    "sad",
    "crying",
    "pain",
    "wince",
    "dizzy",
    "knocked",
    "sick",
    "tired",
    "sleepy",
    "bored",
    "cold",
    "hot",
    "blinded",
    "wary",
    "shut",
];
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FaceForm {
    #[default]
    Open,
    Line,
    Arc,
    Cup,
    Press,
    Cross,
    Spiral,
    Heart,
    Blank,
}
impl FaceForm {
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Line => "line",
            Self::Arc => "arc",
            Self::Cup => "cup",
            Self::Press => "press",
            Self::Cross => "cross",
            Self::Spiral => "spiral",
            Self::Heart => "heart",
            Self::Blank => "blank",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "line" => Self::Line,
            "arc" => Self::Arc,
            "cup" => Self::Cup,
            "press" => Self::Press,
            "cross" => Self::Cross,
            "spiral" => Self::Spiral,
            "heart" => Self::Heart,
            "blank" => Self::Blank,
            _ => Self::Open,
        }
    }
    pub fn closed(self) -> bool {
        matches!(
            self,
            Self::Line | Self::Arc | Self::Cup | Self::Press | Self::Cross
        )
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct FaceExpression {
    pub name: String,
    pub forms: [FaceForm; 2],
    /// Channel order is exposed in `CHANNEL_NAMES`.
    pub parts: [f32; 21],
    pub blink_pace: Option<f32>,
    /// 0 = none, 1 = hurt, 2 = shocked hurt.
    pub startle: u8,
}
impl Default for FaceExpression {
    fn default() -> Self {
        Self {
            name: "neutral".into(),
            forms: [FaceForm::Open; 2],
            parts: [0.0; 21],
            blink_pace: None,
            startle: 0,
        }
    }
}
impl FaceExpression {
    pub fn named(name: &str) -> Self {
        let mut e = Self {
            name: name.to_ascii_lowercase(),
            ..Self::default()
        };
        let canonical = match e.name.as_str() {
            "calm" => "neutral",
            "cross" => "determined",
            "narrow" | "intent" => "focused",
            "startled" => "surprised",
            "glum" => "sad",
            "uneasy" => "nervous",
            "cheery" => "happy",
            "aghast" => "shocked",
            "hurting" => "pain",
            "drowsy" => "sleepy",
            "laugh" => "joyful",
            "angry" => "furious",
            "wink_left" => "wink",
            s => s,
        }
        .to_string();
        e.name = canonical.clone();
        let mut set = |items: &[(usize, f32)], form: Option<FaceForm>, pace: Option<f32>| {
            for &(i, v) in items {
                e.parts[i] = v;
            }
            if let Some(f) = form {
                e.forms = [f; 2];
            }
            e.blink_pace = pace;
        };
        match canonical.as_str() {
            "sleepy" => set(
                &[
                    (0, 0.5),
                    (6, 0.15),
                    (9, -0.25),
                    (10, -0.25),
                    (11, 0.3),
                    (15, 0.4),
                ],
                None,
                Some(0.6),
            ),
            "shut" => set(&[(9, -0.2), (10, -0.2)], Some(FaceForm::Line), Some(0.0)),
            "zen" => set(
                &[(9, 0.3), (10, 0.3), (12, 0.25)],
                Some(FaceForm::Cup),
                Some(0.0),
            ),
            "focused" => set(
                &[(0, 0.3), (2, 0.2), (9, -0.45), (10, -0.45), (11, -0.4)],
                None,
                None,
            ),
            "squint" => set(
                &[(0, 0.45), (2, 0.35), (9, -0.4), (10, -0.4), (11, -0.2)],
                None,
                None,
            ),
            "determined" => set(
                &[(1, 0.8), (0, 0.15), (9, -0.7), (10, -0.7), (11, -1.0)],
                None,
                None,
            ),
            "furious" => set(
                &[
                    (1, 1.0),
                    (0, 0.2),
                    (2, 0.15),
                    (9, -1.0),
                    (10, -1.0),
                    (11, -1.0),
                    (16, 1.0),
                    (8, 0.25),
                ],
                None,
                Some(0.0),
            ),
            "menacing" => set(
                &[(1, 0.6), (0, 0.5), (9, -0.8), (10, -0.8), (11, -1.0)],
                None,
                Some(0.5),
            ),
            "worried" => set(
                &[
                    (1, -0.7),
                    (0, 0.1),
                    (6, 0.1),
                    (9, 0.55),
                    (10, 0.55),
                    (11, 1.0),
                ],
                None,
                None,
            ),
            "nervous" => set(
                &[
                    (1, -0.5),
                    (9, 0.6),
                    (10, 0.6),
                    (11, 1.0),
                    (14, 1.0),
                    (7, 0.4),
                ],
                None,
                None,
            ),
            "sad" => set(
                &[
                    (1, -1.0),
                    (0, 0.35),
                    (6, 0.35),
                    (9, 0.35),
                    (10, 0.35),
                    (11, 1.0),
                ],
                None,
                Some(0.7),
            ),
            "crying" => set(
                &[
                    (1, -1.0),
                    (0, 0.4),
                    (6, 0.3),
                    (9, 0.5),
                    (10, 0.5),
                    (11, 1.0),
                    (13, 1.0),
                ],
                None,
                Some(0.5),
            ),
            "happy" => set(
                &[(3, 1.0), (9, 0.6), (10, 0.6), (12, 0.35)],
                None,
                Some(0.8),
            ),
            "joyful" => set(
                &[(9, 0.8), (10, 0.8), (12, 0.6)],
                Some(FaceForm::Arc),
                Some(0.0),
            ),
            "relaxed" => set(&[(0, 0.25), (9, 0.15), (10, 0.15)], None, Some(0.7)),
            "smug" => set(
                &[
                    (0, 0.5),
                    (3, 1.0),
                    (9, 0.1),
                    (10, 0.65),
                    (5, -0.3),
                    (12, 0.2),
                ],
                None,
                None,
            ),
            "proud" => set(
                &[(0, 0.25), (3, 1.0), (9, 0.45), (10, 0.45), (12, 0.3)],
                None,
                Some(0.6),
            ),
            "sly" => set(&[(0, 0.5), (5, 0.5), (9, -0.3), (10, 0.8)], None, None),
            "sneaky" => set(
                &[
                    (0, 0.35),
                    (5, 0.45),
                    (9, -0.35),
                    (10, -0.35),
                    (11, -0.3),
                    (7, 0.25),
                ],
                None,
                None,
            ),
            "curious" => set(
                &[(4, 0.5), (5, 0.3), (6, -0.1), (9, 0.1), (10, 0.9)],
                None,
                None,
            ),
            "wink" | "wink_right" => {
                set(
                    &[(3, 1.0), (9, 0.3), (10, -0.1), (12, 0.3)],
                    None,
                    Some(0.0),
                );
                e.forms = [FaceForm::Open, FaceForm::Arc];
                if canonical == "wink_right" {
                    e = e.mirrored();
                    e.name = "wink".into();
                }
            }
            "surprised" => set(
                &[(4, 1.0), (6, -0.15), (9, 1.2), (10, 1.2)],
                None,
                Some(0.0),
            ),
            "shocked" => set(
                &[(4, 1.0), (9, 1.4), (10, 1.4), (14, 0.8)],
                Some(FaceForm::Blank),
                Some(0.0),
            ),
            "scared" => set(
                &[
                    (4, 0.5),
                    (1, -0.3),
                    (8, 1.0),
                    (9, 0.9),
                    (10, 0.9),
                    (11, 1.0),
                    (14, 1.0),
                ],
                None,
                Some(0.0),
            ),
            "sparkly" => set(
                &[(4, 0.5), (6, -0.15), (17, 1.0), (9, 0.8), (10, 0.8)],
                None,
                Some(0.5),
            ),
            "excited" => set(
                &[(4, 0.5), (17, 1.0), (9, 1.0), (10, 1.0), (12, 0.5)],
                None,
                Some(0.6),
            ),
            "pain" => set(
                &[(9, -0.5), (10, -0.5), (11, 1.0), (13, 0.35)],
                Some(FaceForm::Press),
                Some(0.0),
            ),
            "wince" => set(
                &[
                    (0, 0.5),
                    (2, 0.2),
                    (1, 0.3),
                    (9, -0.5),
                    (10, -0.5),
                    (11, 1.0),
                ],
                None,
                Some(0.0),
            ),
            "strain" => set(
                &[
                    (1, 0.5),
                    (0, 0.35),
                    (2, 0.3),
                    (9, -0.6),
                    (10, -0.6),
                    (11, 1.0),
                    (14, 0.8),
                ],
                None,
                None,
            ),
            "dizzy" => set(
                &[(9, 0.35), (10, 0.35), (11, 0.5), (14, 0.3)],
                Some(FaceForm::Spiral),
                Some(0.0),
            ),
            "knocked" => set(
                &[(9, -0.3), (10, -0.3), (11, 0.5)],
                Some(FaceForm::Cross),
                Some(0.0),
            ),
            "love" => set(
                &[(9, 0.7), (10, 0.7), (12, 1.0)],
                Some(FaceForm::Heart),
                Some(0.0),
            ),
            "tired" => set(
                &[
                    (0, 0.55),
                    (6, 0.2),
                    (9, -0.15),
                    (10, -0.15),
                    (11, 0.6),
                    (15, 1.0),
                ],
                None,
                Some(0.5),
            ),
            "sick" => set(
                &[
                    (0, 0.4),
                    (1, -0.5),
                    (15, 0.8),
                    (9, 0.4),
                    (10, 0.4),
                    (11, 1.0),
                    (14, 0.5),
                    (7, 0.25),
                ],
                None,
                Some(0.6),
            ),
            "bored" => set(
                &[(0, 0.55), (2, 0.1), (5, 0.45), (9, -0.1), (10, -0.1)],
                None,
                Some(0.5),
            ),
            "cold" => set(
                &[
                    (0, 0.3),
                    (8, 0.6),
                    (9, 0.5),
                    (10, 0.5),
                    (11, 1.0),
                    (12, 0.8),
                ],
                None,
                Some(2.0),
            ),
            "hot" => set(
                &[
                    (0, 0.45),
                    (1, -0.3),
                    (14, 1.0),
                    (12, 0.6),
                    (9, 0.2),
                    (10, 0.2),
                    (11, 0.6),
                ],
                None,
                Some(0.7),
            ),
            "blinded" => set(
                &[
                    (0, 0.4),
                    (2, 0.25),
                    (7, 0.5),
                    (9, 0.3),
                    (10, 0.3),
                    (11, 0.6),
                ],
                None,
                None,
            ),
            "wary" => set(
                &[(0, 0.25), (7, 0.4), (9, -0.3), (10, -0.3), (11, -0.3)],
                None,
                None,
            ),
            "flutter" => {
                e.blink_pace = Some(4.5);
                e.name = "neutral".into();
            }
            "glance_left" => {
                e.name = "neutral".into();
                e.parts[5] = -0.55;
                e.parts[9] = 0.1;
                e.parts[10] = 0.1;
            }
            "glance_right" => {
                e.name = "neutral".into();
                e.parts[5] = 0.55;
                e.parts[9] = 0.1;
                e.parts[10] = 0.1;
            }
            "startle" => {
                e.name.clear();
                e.startle = 1;
                e.blink_pace = Some(0.0);
            }
            "startle_shock" => {
                e.name.clear();
                e.startle = 2;
                e.blink_pace = Some(0.0);
            }
            _ => {}
        }
        e
    }
    pub fn mirrored(mut self) -> Self {
        self.forms.swap(0, 1);
        self.parts[5] = -self.parts[5];
        self.parts.swap(9, 10);
        self.parts.swap(18, 19);
        self
    }
    pub fn scaled(mut self, strength: f32) -> Self {
        if strength == 1.0 {
            return self;
        }
        for p in &mut self.parts {
            *p *= strength;
        }
        self.parts[9] = self.parts[9].clamp(-1.2, 1.6);
        self.parts[10] = self.parts[10].clamp(-1.2, 1.6);
        self
    }
    pub fn from_json(name: &str, v: &Value) -> Self {
        let mut e = Self::named(v["base"].as_str().unwrap_or("neutral"));
        e.name = name.into();
        for (i, key) in CHANNEL_NAMES.iter().enumerate() {
            if let Some(x) = v[key].as_f64() {
                e.parts[i] = finite(x as f32, 0.0).clamp(-4.0, 4.0);
            }
        }
        if let Some(x) = v["lids"].as_f64() {
            e.parts[0] = finite(x as f32, 0.0);
        }
        if let Some(x) = v["brows"].as_f64() {
            e.parts[9] = x as f32;
            e.parts[10] = x as f32;
        }
        if let Some(x) = v["pupils"].as_array() {
            for i in 0..2 {
                if let Some(n) = x.get(i).and_then(Value::as_f64) {
                    e.parts[5 + i] = n as f32;
                }
            }
        }
        if let Some(x) = v["form"].as_str() {
            e.forms = [FaceForm::parse(x); 2];
        }
        for (i, key) in ["right_form", "left_form"].iter().enumerate() {
            if let Some(x) = v[key].as_str() {
                e.forms[i] = FaceForm::parse(x);
            }
        }
        if let Some(x) = v["blinks"].as_f64().or_else(|| v["blink_pace"].as_f64()) {
            e.blink_pace = Some((x as f32).clamp(0.0, 8.0));
        }
        e
    }
    pub fn to_json(&self) -> Value {
        let mut v = json!({"right_form":self.forms[0].name(),"left_form":self.forms[1].name(),"blinks":self.blink_pace});
        for (i, key) in CHANNEL_NAMES.iter().enumerate() {
            v[*key] = json!(self.parts[i]);
        }
        v
    }
    pub fn names() -> &'static [&'static str] {
        &LOOK_NAMES
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FacePersonality {
    pub id: String,
    pub rest: String,
    pub swaps: BTreeMap<String, String>,
    pub blush: f32,
}
impl FacePersonality {
    pub fn preset(id: &str) -> Self {
        let mut p = Self {
            id: id.to_ascii_lowercase(),
            rest: "neutral".into(),
            swaps: BTreeMap::new(),
            blush: 0.0,
        };
        let pairs: &[(&str, &str)] = match p.id.as_str() {
            "brave" => &[
                ("scared", "determined"),
                ("nervous", "determined"),
                ("shocked", "determined"),
                ("worried", "focused"),
                ("sad", "determined"),
            ],
            "fierce" => &[
                ("scared", "furious"),
                ("shocked", "furious"),
                ("nervous", "menacing"),
                ("wary", "menacing"),
                ("worried", "determined"),
                ("sad", "determined"),
                ("surprised", "determined"),
            ],
            "cute" => {
                p.blush = 0.35;
                &[
                    ("curious", "sparkly"),
                    ("surprised", "sparkly"),
                    ("proud", "happy"),
                    ("smug", "wink"),
                    ("furious", "determined"),
                    ("menacing", "sly"),
                ]
            }
            "cheerful" => {
                p.rest = "happy".into();
                &[
                    ("bored", "relaxed"),
                    ("tired", "relaxed"),
                    ("sleepy", "relaxed"),
                    ("sad", "worried"),
                    ("wary", "curious"),
                ]
            }
            "calm" => {
                p.rest = "relaxed".into();
                &[
                    ("scared", "worried"),
                    ("shocked", "surprised"),
                    ("nervous", "wary"),
                    ("furious", "determined"),
                    ("crying", "sad"),
                    ("excited", "happy"),
                    ("joyful", "happy"),
                    ("surprised", "curious"),
                ]
            }
            "grumpy" => {
                p.rest = "bored".into();
                &[
                    ("happy", "smug"),
                    ("joyful", "happy"),
                    ("excited", "curious"),
                    ("sparkly", "curious"),
                    ("relaxed", "bored"),
                    ("proud", "smug"),
                    ("love", "happy"),
                    ("wink", "sly"),
                    ("zen", "shut"),
                ]
            }
            "gloomy" => {
                p.rest = "sad".into();
                &[
                    ("happy", "relaxed"),
                    ("joyful", "happy"),
                    ("excited", "curious"),
                    ("proud", "relaxed"),
                    ("sparkly", "curious"),
                ]
            }
            _ => {
                p.id = "classic".into();
                &[]
            }
        };
        for &(a, b) in pairs {
            p.swaps.insert(a.into(), b.into());
        }
        p
    }
    pub fn from_json(id: &str, v: &Value) -> Self {
        let mut p = Self::preset("classic");
        p.id = id.into();
        p.rest = v["rest"].as_str().unwrap_or("neutral").into();
        p.blush = v["blush"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0) as f32;
        if let Some(o) = v["swaps"].as_object() {
            for (a, b) in o.iter().take(24) {
                if let Some(b) = b.as_str() {
                    p.swaps.insert(a.clone(), b.into());
                }
            }
        }
        p
    }
    pub fn to_json(&self) -> Value {
        json!({"rest":self.rest,"blush":self.blush,"swaps":self.swaps})
    }
    fn apply(&self, e: FaceExpression, s: &FaceSettings) -> Option<FaceExpression> {
        // Procedural looks (aim/scope/startle) have no named personality swap.
        if e.name.is_empty() {
            return Some(e);
        }
        let name = self
            .swaps
            .get(&e.name)
            .map(String::as_str)
            .unwrap_or(if e.name == "neutral" {
                &self.rest
            } else {
                &e.name
            });
        if name.is_empty() {
            return None;
        }
        if name == e.name {
            return Some(e);
        }
        let original = if e.name == "neutral" {
            FaceExpression::default()
        } else {
            s.look(&e.name)
        };
        let mut changed = s.look(name);
        changed.parts[5] += e.parts[5] - original.parts[5];
        changed.parts[6] += e.parts[6] - original.parts[6];
        Some(changed)
    }
}

/// Host input. All time is seconds; speeds are blocks/second, angles degrees.
/// `cues` accepts boolean/quantitative environmental values, e.g.
/// `warden_near=1`, `vehicle=2`, `vehicle_speed=6`, `watched=1`.
/// `forward`/`strafe` are normalized walking directions; `forward_velocity`,
/// `vehicle_speed`, `vertical_speed` and `gliding_speed` use blocks/second.
/// Optional semantic cues `fall_weight` (0..3), `landing_weight` (1..3),
/// `climb_direction` (-1..1), `leaping`, `still_seconds`, and `run_seconds`
/// take precedence over the engine's best-effort movement inference.
/// `events` maps reaction/event IDs to seconds since occurrence, e.g.
/// `release=.1`, `cast=.2`, `hurt=.05`, `landing=.1`, `levelup=0`.
/// `active_reactions` activates any supported reaction directly (0..1 strength)
/// when the host already knows its semantic state. This also supports previews.
#[derive(Clone, Debug)]
pub struct FaceConditions {
    pub time_seconds: f32,
    pub entity_id: u32,
    pub own_player: bool,
    pub hidden: bool,
    pub opacity: f32,
    pub quads: bool,
    pub head_yaw: f32,
    pub head_pitch: f32,
    pub moving: bool,
    pub sprinting: bool,
    pub crouching: bool,
    pub crawling: bool,
    pub on_ground: bool,
    pub vertical_speed: f32,
    pub forward: f32,
    pub strafe: f32,
    pub fall_distance: f32,
    pub swimming: bool,
    pub underwater: bool,
    pub in_water: bool,
    pub in_lava: bool,
    pub flying: bool,
    pub gliding: bool,
    pub gliding_speed: f32,
    pub bank: f32,
    pub climbing: bool,
    pub riding: bool,
    pub sleeping: bool,
    pub dead: bool,
    pub hurt_seconds: f32,
    pub health: f32,
    pub air: f32,
    pub hunger: f32,
    pub attack: f32,
    pub aiming: bool,
    pub blocking: bool,
    pub eating: bool,
    pub drinking: bool,
    /// 0 = avatar right, 1 = left.
    pub main_arm: usize,
    pub night: bool,
    pub rain: bool,
    pub emote: Option<String>,
    pub forced_look: Option<String>,
    pub cues: BTreeMap<String, f32>,
    pub events: BTreeMap<String, f32>,
    pub active_reactions: BTreeMap<String, f32>,
    /// Sampled clip bone positions in model-channel units, not world speeds.
    /// Uses the original `face_*` names; X/Y drive pupils, Y other channels.
    pub face_channels: BTreeMap<String, [f32; 3]>,
}
impl Default for FaceConditions {
    fn default() -> Self {
        Self {
            time_seconds: 0.0,
            entity_id: 0,
            own_player: true,
            hidden: false,
            opacity: 1.0,
            quads: false,
            head_yaw: 0.0,
            head_pitch: 0.0,
            moving: false,
            sprinting: false,
            crouching: false,
            crawling: false,
            on_ground: true,
            vertical_speed: 0.0,
            forward: 0.0,
            strafe: 0.0,
            fall_distance: 0.0,
            swimming: false,
            underwater: false,
            in_water: false,
            in_lava: false,
            flying: false,
            gliding: false,
            gliding_speed: 0.0,
            bank: 0.0,
            climbing: false,
            riding: false,
            sleeping: false,
            dead: false,
            hurt_seconds: f32::INFINITY,
            health: 20.0,
            air: 1.0,
            hunger: 20.0,
            attack: 0.0,
            aiming: false,
            blocking: false,
            eating: false,
            drinking: false,
            main_arm: 0,
            night: false,
            rain: false,
            emote: None,
            forced_look: None,
            cues: BTreeMap::new(),
            events: BTreeMap::new(),
            active_reactions: BTreeMap::new(),
            face_channels: BTreeMap::new(),
        }
    }
}
impl FaceConditions {
    pub fn cue(&self, key: &str) -> f32 {
        finite(self.cues.get(key).copied().unwrap_or(0.0), 0.0)
    }
    pub fn event_age(&self, key: &str) -> f32 {
        self.events.get(key).copied().unwrap_or(f32::INFINITY)
    }
    pub fn activate(&mut self, id: &str, strength: f32) {
        self.active_reactions
            .insert(id.into(), strength.clamp(0.0, 1.0));
    }
    pub fn reaction_names() -> Vec<&'static str> {
        REACTIONS.iter().map(|r| r.0).collect()
    }
}
#[derive(Clone, Debug)]
pub struct FaceFrame {
    pub forms: [FaceForm; 2],
    pub lids: [f32; 2],
    pub lower: f32,
    pub smile: f32,
    pub slant: f32,
    pub wide: f32,
    pub pupil: [f32; 2],
    pub brows: [f32; 2],
    pub tilt: f32,
    pub blush: f32,
    pub tears: f32,
    pub sweat: f32,
    pub bags: f32,
    pub anger: f32,
    pub sparkle: f32,
    pub age_ticks: f32,
    pub reaction: String,
}
impl Default for FaceFrame {
    fn default() -> Self {
        Self {
            forms: [FaceForm::Open; 2],
            lids: [0.0; 2],
            lower: 0.0,
            smile: 0.0,
            slant: 0.0,
            wide: 0.0,
            pupil: [0.0; 2],
            brows: [0.0; 2],
            tilt: 0.0,
            blush: 0.0,
            tears: 0.0,
            sweat: 0.0,
            bags: 0.0,
            anger: 0.0,
            sparkle: 0.0,
            age_ticks: 0.0,
            reaction: "gaze".into(),
        }
    }
}

// Priority order also defines deterministic tie breaking.
const REACTIONS: &[(&str, &str, i32)] = &[
    ("fading", "knocked", 1190),
    ("doze", "shut", 1180),
    ("lava", "startle", 1170),
    ("blindside", "startle_shock", 1165),
    ("struck", "startle", 1160),
    ("dread", "scared", 1150),
    ("brace", "shocked", 1140),
    ("freefall", "shocked", 1130),
    ("thud", "pain", 1120),
    ("scald", "scared", 1110),
    ("spin", "dizzy", 1100),
    ("splash", "pain", 1090),
    ("stumble", "wince", 1080),
    ("smash", "furious", 1070),
    ("slam", "furious", 1060),
    ("breathless", "scared", 1050),
    ("hurl", "furious", 1040),
    ("flash", "shocked", 1030),
    ("lunge", "determined", 1020),
    ("plunge", "surprised", 1010),
    ("cannonball", "joyful", 1000),
    ("recoil", "surprised", 990),
    ("boost", "excited", 980),
    ("spooked", "nervous", 970),
    ("nibble", "surprised", 960),
    ("scope", "neutral", 950),
    ("levelup", "sparkly", 940),
    ("toot", "joyful", 930),
    ("emote", "happy", 920),
    ("windup", "determined", 910),
    ("load", "strain", 900),
    ("aim", "neutral", 890),
    ("release", "smug", 880),
    ("guard", "determined", 870),
    ("reel", "joyful", 860),
    ("munch", "joyful", 850),
    ("sip", "zen", 840),
    ("cast", "sparkly", 830),
    ("adore", "love", 820),
    ("chart", "curious", 815),
    ("study", "focused", 810),
    ("brush", "focused", 805),
    ("ignite", "menacing", 800),
    ("mine", "focused", 795),
    ("strike", "determined", 790),
    ("shear", "happy", 785),
    ("toss", "wink", 780),
    ("pearl", "excited", 775),
    ("brew", "sly", 770),
    ("tinker", "curious", 765),
    ("equip", "proud", 760),
    ("swap", "neutral", 755),
    ("chill", "cold", 700),
    ("scorch", "startle", 695),
    ("shiver", "cold", 690),
    ("dim", "blinded", 685),
    ("woozy", "dizzy", 680),
    ("queasy", "sick", 675),
    ("wounded", "sad", 670),
    ("hungry", "tired", 665),
    ("boosted", "excited", 660),
    ("dolphin", "determined", 600),
    ("submerged", "squint", 595),
    ("paddle", "focused", 590),
    ("surface", "happy", 585),
    ("wade", "worried", 580),
    ("drizzle", "squint", 575),
    ("bank", "happy", 550),
    ("jet", "squint", 545),
    ("glide", "happy", 540),
    ("zoom", "determined", 535),
    ("hover", "relaxed", 530),
    ("float", "nervous", 525),
    ("leap", "excited", 520),
    ("liftoff", "happy", 515),
    ("ascent", "happy", 510),
    ("touchdown", "squint", 505),
    ("scramble", "strain", 500),
    ("descend", "focused", 495),
    ("shimmy", "worried", 490),
    ("skyride", "sparkly", 470),
    ("gallop", "excited", 465),
    ("rail", "surprised", 460),
    ("saddle", "joyful", 455),
    ("voyage", "curious", 450),
    ("sail", "relaxed", 445),
    ("mounted", "happy", 440),
    ("carrying", "strain", 435),
    ("vertigo", "nervous", 420),
    ("tightrope", "worried", 415),
    ("kickoff", "excited", 410),
    ("skid", "surprised", 405),
    ("marathon", "strain", 400),
    ("dash", "determined", 395),
    ("duck", "wary", 390),
    ("sneak", "sneaky", 385),
    ("crouch", "focused", 380),
    ("prone", "focused", 375),
    ("backpedal", "worried", 370),
    ("sidestep", "neutral", 365),
    ("stride", "neutral", 360),
    ("jam", "zen", 300),
    ("cozy", "zen", 295),
    ("noticed", "sly", 290),
    ("ghost", "sly", 285),
    ("beacon", "sparkly", 280),
    ("glow", "curious", 275),
    ("fidget", "sly", 270),
    ("daydream", "sleepy", 265),
    ("still", "neutral", 260),
    ("bloom", "happy", 200),
    ("frosty", "cold", 195),
    ("swelter", "hot", 190),
    ("eerie", "nervous", 185),
    ("night", "sleepy", 180),
    ("hellish", "determined", 175),
    ("void", "worried", 170),
    ("deep", "wary", 165),
    ("gaze", "neutral", 10),
];

#[derive(Clone)]
struct Winner {
    look: FaceExpression,
    priority: i32,
    id: String,
}
pub(super) struct FaceMemory {
    last_time: Option<f32>,
    was_ground: bool,
    was_running: bool,
    was_crouching: bool,
    events: BTreeMap<String, f32>,
    last_fall: f32,
    run_start: f32,
    still_start: f32,
    held: Option<Winner>,
    held_seen: f32,
    from: [f32; 21],
    to: [f32; 21],
    ease_start: f32,
    initialized: bool,
    shown: [FaceForm; 2],
    coming: [FaceForm; 2],
    switch_start: [Option<f32>; 2],
    last_look: Option<(f32, f32, f32)>,
    turned_at: f32,
    idle: f32,
    idle_age: Option<f32>,
    jump_moving: bool,
}
impl Default for FaceMemory {
    fn default() -> Self {
        Self {
            last_time: None,
            was_ground: true,
            was_running: false,
            was_crouching: false,
            events: BTreeMap::new(),
            last_fall: 0.0,
            run_start: 0.0,
            still_start: 0.0,
            held: None,
            held_seen: 0.0,
            from: [0.0; 21],
            to: [0.0; 21],
            ease_start: 0.0,
            initialized: false,
            shown: [FaceForm::Open; 2],
            coming: [FaceForm::Open; 2],
            switch_start: [None; 2],
            last_look: None,
            turned_at: f32::NEG_INFINITY,
            idle: 0.0,
            idle_age: None,
            jump_moving: false,
        }
    }
}
impl FaceMemory {
    fn event(&self, c: &FaceConditions, id: &str, age: f32) -> f32 {
        c.event_age(id).min(
            self.events
                .get(id)
                .map(|t| age - *t)
                .unwrap_or(f32::INFINITY),
        )
    }
    fn moment(c: &FaceConditions, age: f32, salt: u32, show: f32, period: f32) -> bool {
        if c.cue("every_moment") > 0.0 {
            return true;
        }
        let cycle = (age / period).floor() as i32;
        let phase = age - cycle as f32 * period;
        let key = c.entity_id.wrapping_mul(31).wrapping_add(salt);
        let start = signed_mod(hash(key, cycle as u32), (period - show) as i32) as f32;
        phase >= start && phase < start + show
    }
    fn active(&self, id: &str, c: &FaceConditions, t: f32, s: &FaceSettings) -> bool {
        if let Some(&v) = c.active_reactions.get(id) {
            return v > 0.0;
        }
        if c.cue(id) > 0.0
            && !matches!(
                id,
                "night" | "daydream" | "marathon" | "hungry" | "frosty" | "swelter"
            )
        {
            return true;
        }
        let event = |key: &str, ticks: f32| self.event(c, key, t) < ticks / 20.0;
        let flag = |key: &str| c.cue(key) > 0.0;
        let dropping = !c.on_ground
            && !c.climbing
            && !c.in_water
            && !flag("blind")
            && !c.riding
            && c.vertical_speed < 0.0;
        let still = c
            .cues
            .get("still_seconds")
            .copied()
            .unwrap_or(t - self.still_start);
        let fall = c.cues.get("fall_weight").copied().unwrap_or({
            if !dropping || c.fall_distance < 1.5 {
                0.0
            } else if c.fall_distance < 6.0 {
                1.0
            } else {
                2.0
            }
        });
        let landing = c.cues.get("landing_weight").copied().unwrap_or({
            if self.last_fall >= 6.0 {
                3.0
            } else if self.last_fall >= 3.0 {
                2.0
            } else {
                1.0
            }
        });
        let moment = |salt, show, period| Self::moment(c, t * 20.0, salt, show, period);
        match id {
            "fading" => c.dead,
            "doze" => c.sleeping,
            "lava" => c.in_lava && c.hurt_seconds < 0.6,
            "blindside" => flag("hurt_behind") && c.hurt_seconds < 0.6,
            "struck" => c.hurt_seconds < 0.6 || event("hurt", 12.0),
            "dread" => flag("warden_near"),
            "brace" => flag("creeper_near"),
            "freefall" => fall >= 2.0,
            "thud" => event("landing", 14.0) && landing == 3.0,
            "scald" => flag("lava_below"),
            "spin" => flag("riptide"),
            "splash" => event("splash", 8.0),
            "stumble" => event("landing", 8.0) && landing == 2.0,
            "smash" => c.attack > 0.0 && dropping,
            "slam" => flag("slamming"),
            "breathless" => c.underwater && c.air < 0.25,
            "hurl" => event("hurl", 14.0),
            "flash" => flag("lightning") || event("flash", 30.0),
            "lunge" => event("lunge", 12.0),
            "plunge" => fall == 1.0,
            "cannonball" => flag("water_below"),
            "recoil" => c.cue("forward_velocity") < -6.0,
            "boost" => c.gliding && event("boost", 20.0),
            "spooked" => flag("hostile_near"),
            "nibble" => flag("biting"),
            "scope" => flag("spyglass"),
            "levelup" => event("levelup", 40.0),
            "toot" => flag("tooting"),
            "emote" => c.emote.is_some(),
            "windup" => flag("charging_throw"),
            "load" => flag("loading"),
            "aim" => c.aiming,
            "release" => event("release", 12.0),
            "guard" => c.blocking,
            "reel" => event("reel", 14.0),
            "munch" => c.eating,
            "sip" => c.drinking,
            "cast" => event("cast", 16.0),
            "adore" => flag("adoring"),
            "chart" => flag("map_view"),
            "study" => flag("studying"),
            "brush" => flag("brushing"),
            "ignite" => event("ignite", 16.0),
            "mine" => flag("mining"),
            "strike" => c.attack > 0.0,
            "shear" => event("shear", 12.0),
            "toss" => event("toss", 14.0),
            "pearl" => event("pearl", 14.0),
            "brew" => event("brew", 14.0),
            "tinker" => event("interact", 10.0),
            "equip" => event("equip", 24.0),
            "swap" => event("swap", 8.0),
            "chill" => flag("frozen"),
            "scorch" => flag("burning") && c.hurt_seconds < 0.6,
            "shiver" => flag("freezing"),
            "dim" => flag("blind"),
            "woozy" => flag("nauseous"),
            "queasy" => flag("poisoned") || flag("withering"),
            "wounded" => c.health > 0.0 && c.health <= 6.0,
            "hungry" => (c.hunger <= 6.0 || flag("starving")) && moment(1, 80.0, 1200.0),
            "boosted" => flag("boosted"),
            "dolphin" => c.underwater && c.swimming && c.sprinting,
            "submerged" => c.underwater,
            "paddle" => c.swimming,
            "surface" => c.in_water && !c.underwater && !c.swimming,
            "wade" => flag("shallow_water"),
            "drizzle" => c.rain,
            "bank" => c.gliding && c.bank.abs() >= 8.0,
            "jet" => c.gliding && c.gliding_speed >= 10.0,
            "glide" => c.gliding,
            "zoom" => c.flying && flag("fast_flight"),
            "hover" => c.flying,
            "float" => flag("levitating"),
            "leap" => {
                event("liftoff", 10.0)
                    && c.cues
                        .get("leaping")
                        .map_or(self.jump_moving || c.moving || c.sprinting, |v| *v > 0.0)
            }
            "liftoff" => event("liftoff", 4.0),
            "ascent" => {
                !c.on_ground
                    && !c.climbing
                    && !c.in_water
                    && !c.gliding
                    && !c.flying
                    && c.vertical_speed > 2.0
            }
            "touchdown" => event("landing", 5.0),
            "scramble" => {
                c.climbing
                    && c.cues
                        .get("climb_direction")
                        .copied()
                        .unwrap_or(c.vertical_speed)
                        > 0.0
            }
            "descend" => {
                c.climbing
                    && c.cues
                        .get("climb_direction")
                        .copied()
                        .unwrap_or(c.vertical_speed)
                        < 0.0
            }
            "shimmy" => c.climbing,
            "skyride" => c.cue("vehicle") == 5.0,
            "gallop" => c.cue("vehicle") == 2.0 && c.cue("vehicle_speed") >= 5.0,
            "rail" => c.cue("vehicle") == 3.0 && c.cue("vehicle_speed") >= 2.0,
            "saddle" => c.cue("vehicle") == 4.0,
            "voyage" => c.cue("vehicle") == 6.0,
            "sail" => c.cue("vehicle") == 1.0,
            "mounted" => c.riding,
            "carrying" => flag("carrying"),
            "vertigo" => flag("over_edge"),
            "tightrope" => flag("on_fence"),
            "kickoff" => c.sprinting && event("run_start", 10.0),
            "skid" => event("run_stop", 12.0),
            "marathon" => {
                c.sprinting
                    && c.cues
                        .get("run_seconds")
                        .copied()
                        .unwrap_or(t - self.run_start)
                        >= 6.0
                    && moment(2, 80.0, 600.0)
            }
            "dash" => c.sprinting,
            "duck" => c.crouching && event("crouch", 8.0),
            "sneak" => c.crouching && c.moving,
            "crouch" => c.crouching,
            "prone" => c.crawling,
            "backpedal" => c.moving && c.forward < -0.5,
            "sidestep" => c.moving && c.strafe.abs() >= 0.5 && c.forward.abs() < 0.5,
            "stride" => c.moving && !c.sprinting,
            "jam" => flag("music") && still >= 1.0,
            "cozy" => flag("campfire") && still >= 1.0,
            "noticed" => flag("watched"),
            "ghost" => flag("invisible"),
            "beacon" => flag("glowing"),
            "glow" => flag("holding_light") && (c.night || flag("deep")),
            "fidget" => event("fidget", 60.0),
            "daydream" => still >= s.daydream_seconds && moment(6, 160.0, 800.0),
            "still" => still >= 2.0,
            "bloom" => flag("flowery"),
            "frosty" => flag("frosty") && !c.in_water && moment(3, 80.0, 600.0),
            "swelter" => flag("hot") && (!c.night || flag("nether")) && moment(4, 80.0, 600.0),
            "eerie" => flag("eerie"),
            "night" => c.night && moment(5, 80.0, 2400.0),
            "hellish" => flag("nether"),
            "void" => flag("end"),
            "deep" => flag("deep"),
            "gaze" => true,
            _ => false,
        }
    }
    fn expression(
        &self,
        id: &str,
        name: &str,
        c: &FaceConditions,
        s: &FaceSettings,
    ) -> FaceExpression {
        if let Some(name) = s.trigger_looks.get(id) {
            return s.look(name);
        }
        let mut e = if id == "emote" {
            s.look(c.emote.as_deref().unwrap_or("happy"))
        } else if ["scope", "aim", "swap", "stride", "still", "gaze"].contains(&id) {
            FaceExpression::default()
        } else {
            s.look(name)
        };
        match id {
            "scope" | "aim" => {
                e.name.clear();
                let off = 1 - c.main_arm.min(1);
                e.forms[off] = FaceForm::Line;
                e.blink_pace = Some(0.0);
                if id == "scope" {
                    e.parts[4] = 0.35;
                    e.parts[9 + off] = -0.6;
                    e.parts[10 - off] = 0.45;
                } else {
                    e.parts[0] = 0.2;
                    e.parts[2] = 0.15;
                    e.parts[9 + off] = -0.8;
                    e.parts[10 - off] = -0.25;
                }
            }
            "spooked" | "noticed" => {
                let side = c
                    .cue(if id == "spooked" {
                        "hostile_side"
                    } else {
                        "watcher_side"
                    })
                    .clamp(-1.0, 1.0);
                if side != 0.0 {
                    e.parts[5] = 0.45 * side;
                }
            }
            "bank" => e.parts[5] = 0.45 * c.bank.signum(),
            "sidestep" => e.parts[5] = 0.45 * c.strafe.signum(),
            "chart" | "brush" | "descend" | "tightrope" => {
                e.parts[5] = 0.0;
                e.parts[6] = 0.4;
                if id == "brush" {
                    e.parts[11] = 0.3;
                }
            }
            "study" => {
                e.parts[5] = 0.0;
                e.parts[6] = 0.35;
                e.blink_pace = Some(0.5);
            }
            "swap" => {
                e.name.clear();
                e.parts[6] = 0.45;
                e.parts[9] = 0.2;
                e.parts[10] = 0.2;
            }
            "vertigo" => {
                e.parts[5] = 0.0;
                e.parts[6] = 0.45;
            }
            "chill" => {
                e.parts[8] = 1.0;
                e.parts[15] = 0.4;
            }
            "wounded" | "drizzle" => e.blink_pace = Some(2.0),
            "submerged" => e.blink_pace = Some(0.5),
            "mounted" => e.blink_pace = Some(0.75),
            "jam" => e.parts[20] = 1.0,
            "cozy" => e.parts[12] = 0.5,
            "glow" => {
                e.parts[4] = 0.6;
                e.blink_pace = Some(1.0);
            }
            "fidget" => {
                let names = ["sly", "curious", "bored", "wink", "smug", "happy"];
                // Choose once per idle event, not once per global 60-tick bin.
                let event_tick = (c.time_seconds * 20.0).floor() as i32
                    - (c.event_age("fidget") * 20.0).floor() as i32;
                e = s.look(names[signed_mod(hash(c.entity_id, event_tick as u32), 6) as usize]);
            }
            "daydream" => e.parts[15] = 0.0,
            "frosty" => {
                e.parts[8] = 0.0;
                e.blink_pace = Some(1.3);
            }
            "eerie" => e.parts[14] = 0.0,
            "night" => {
                e.parts[0] = 0.2;
                e.parts[15] = 0.0;
                e.blink_pace = Some(1.3);
            }
            "hellish" => {
                e.parts[1] = 0.5;
                e.parts[9] = -0.45;
                e.parts[10] = -0.45;
            }
            "stride" | "still" => {
                e.name.clear();
                e.blink_pace = Some(1.0);
            }
            "gaze" => {
                e.name.clear();
                e.blink_pace = None;
            }
            "lava" | "blindside" | "struck" | "scorch" => e.name.clear(),
            _ => {}
        }
        e
    }
    fn winner(&self, c: &FaceConditions, t: f32, s: &FaceSettings) -> Winner {
        if let Some(name) = &c.forced_look {
            return Winner {
                look: s.look(name).scaled(s.expression_strength),
                priority: 2000,
                id: "forced".into(),
            };
        }
        let mut result = None;
        let mut pace = None;
        for &(id, name, priority) in REACTIONS {
            if !s.reaction_enabled(id) || !self.active(id, c, t, s) {
                continue;
            }
            let e = self.expression(id, name, c, s);
            let e = if id == "emote" || s.trigger_looks.contains_key(id) {
                Some(e)
            } else {
                s.personality.apply(e, s)
            };
            let Some(mut e) = e else {
                continue;
            };
            if pace.is_none() {
                pace = e.blink_pace;
            }
            // Rate-only idle/walk reactions must not mask environmental looks.
            if ["stride", "still", "gaze"].contains(&id) && !s.trigger_looks.contains_key(id) {
                continue;
            }
            if result.is_none() {
                if let Some(&strength) = c.active_reactions.get(id) {
                    e = e.scaled(strength);
                }
                result = Some(Winner {
                    look: e,
                    priority,
                    id: id.into(),
                });
            }
        }
        let mut winner = result.unwrap_or_else(|| Winner {
            look: s.look(&s.personality.rest),
            priority: 0,
            id: "gaze".into(),
        });
        if pace.is_some() {
            winner.look.blink_pace = pace;
        }
        winner.look = winner.look.scaled(s.expression_strength);
        winner
    }
    pub(super) fn frame(&mut self, c: &FaceConditions, s: &FaceSettings) -> FaceFrame {
        let time = finite(c.time_seconds, 0.0).max(0.0);
        if self.last_time.is_some_and(|t| time < t) {
            *self = Self::default();
        }
        let first = self.last_time.is_none();
        if first {
            self.still_start = time;
            self.run_start = time;
        }
        if !first {
            if c.on_ground && !self.was_ground {
                self.events.insert("landing".into(), time);
            }
            if !c.on_ground && self.was_ground && c.vertical_speed > 0.0 {
                self.events.insert("liftoff".into(), time);
                self.jump_moving = c.moving || c.sprinting;
            }
            if c.sprinting && !self.was_running {
                self.events.insert("run_start".into(), time);
                self.run_start = time;
            }
            if !c.sprinting && self.was_running {
                self.events.insert("run_stop".into(), time);
            }
            if c.crouching && !self.was_crouching {
                self.events.insert("crouch".into(), time);
            }
        }
        if !c.on_ground {
            self.last_fall = c.fall_distance;
        }
        if c.moving || c.sprinting || !c.on_ground {
            self.still_start = time;
        }
        self.was_ground = c.on_ground;
        self.was_running = c.sprinting;
        self.was_crouching = c.crouching;
        self.last_time = Some(time);
        let mut target = self.winner(c, time, s);
        if c.forced_look.is_none() {
            if let Some(held) = &self.held {
                if time - self.held_seen < 0.2 && target.priority < held.priority {
                    target = held.clone();
                } else {
                    self.held_seen = time;
                }
            } else {
                self.held_seen = time;
            }
            self.held = Some(target.clone());
        }
        let age = time * 20.0;
        let mut e = target.look;
        if !self.initialized {
            self.from = e.parts;
            self.to = e.parts;
            self.ease_start = time;
            self.initialized = true;
        } else if self.to != e.parts {
            // Retarget from the currently eased value, including when a dynamic
            // target changes each frame. Restarting here is intentional.
            let t = ((time - self.ease_start) / 0.15).clamp(0.0, 1.0);
            self.from = std::array::from_fn(|i| self.from[i] + (self.to[i] - self.from[i]) * t);
            self.to = e.parts;
            self.ease_start = time;
        }
        let ease = ((time - self.ease_start) / 0.15).clamp(0.0, 1.0);
        e.parts = std::array::from_fn(|i| self.from[i] + (self.to[i] - self.from[i]) * ease);
        // Clip channels layer over eased reactions without entering the reaction
        // easing history. Death/sleep/hurt/nearby danger always retain control.
        if target.priority <= 1135 && c.forced_look.is_none() {
            e = over_channels(e, &c.face_channels, s);
        }
        let p = e.parts;
        let gaze = s.reaction_enabled("gaze");
        let up = if gaze {
            (-c.head_pitch / 45.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let down = if gaze {
            (c.head_pitch / 35.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let yaw = if gaze {
            (c.head_yaw / 45.0).clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let idle_weight = if !s.glancing {
            0.0
        } else if c.forced_look.is_some() {
            1.0
        } else {
            self.idle_weight(target.priority <= 300, c.head_yaw, c.head_pitch, age)
        };
        let still = c
            .cues
            .get("still_seconds")
            .copied()
            .unwrap_or(time - self.still_start);
        let idle = idle_weight > 0.0 && still >= 2.0;
        let (spell, glance) = if idle {
            let (spell, glance) = glance_spell(c.entity_id, age);
            (spell * idle_weight, glance * idle_weight)
        } else {
            (0.0, 0.0)
        };
        let drift = if idle {
            0.125
                * (1.0
                    - ((age + signed_mod(hash(c.entity_id, 7), 920) as f32) / 920.0
                        * std::f32::consts::TAU)
                        .cos())
                * idle_weight
        } else {
            0.0
        };
        let mut lid = p[0].max(0.45 * down).max(0.3 * spell) * (1.0 - up);
        let mut wide = p[4].max(0.9 * up);
        let mut pupil = [
            p[5] - 0.55 * yaw
                + 0.45 * glance
                + p[7] * (std::f32::consts::TAU * age / 18.0).sin()
                + p[8] * 0.25 * jitter(c.entity_id, age, 0),
            p[6] - 0.45 * up
                + 0.3 * down
                + 0.12 * spell
                + p[8] * 0.25 * jitter(c.entity_id, age, 1),
        ];
        let brow_gaze = 0.55 * up - 0.35 * down
            + 0.15 * spell
            + drift
            + p[20] * 0.3 * (std::f32::consts::PI * age / 12.0).sin().abs();
        let mut brows = [p[9] + brow_gaze, p[10] + brow_gaze];
        let hurt = c.hurt_seconds.min(self.event(c, "hurt", time)) * 20.0;
        let mut tilt = p[11];
        let mut lower = p[2];
        if e.startle > 0 && hurt < 12.0 {
            let b = if hurt < 4.0 {
                1.3 * (std::f32::consts::FRAC_PI_2 * (hurt / 4.0).clamp(0.0, 1.0)).sin()
            } else if hurt < 6.0 {
                1.3 - 1.65 * (hurt - 4.0) / 2.0
            } else {
                -0.35 * (12.0 - hurt).clamp(0.0, 1.0)
            };
            brows[0] += b;
            brows[1] += b;
            tilt = tilt.max(ramp(hurt, 4.0, 6.0) * (1.0 - ramp(hurt, 11.0, 12.0)));
            if hurt < 4.0 {
                let t = ramp(hurt, 0.0, 1.0);
                lid *= 1.0 - t;
                lower *= 1.0 - t;
                wide = wide.max(t);
                pupil[1] -= 0.35 * t;
                if e.startle == 2 {
                    e.forms = [FaceForm::Blank; 2];
                }
            } else if hurt < 11.0 {
                e.forms = [FaceForm::Press; 2];
            }
        }
        let pace = e.blink_pace.unwrap_or(1.0);
        let beat = if s.blinking && pace > 0.0 {
            blink(c.entity_id, age, s.blink_seconds * 20.0 / pace)
        } else {
            0.0
        };
        let mut lids = [
            lid.max(beat).max(p[18]).clamp(0.0, 1.0),
            lid.max(beat).max(p[19]).clamp(0.0, 1.0),
        ];
        for (i, lid) in lids.iter_mut().enumerate() {
            if e.forms[i] != self.coming[i] {
                if let Some(start) = self.switch_start[i]
                    && age - start >= 1.0
                {
                    self.shown[i] = self.coming[i];
                }
                self.coming[i] = e.forms[i];
                self.switch_start[i] = if e.forms[i] != self.shown[i] {
                    Some(age)
                } else {
                    None
                };
            }
            if let Some(start) = self.switch_start[i] {
                let t = (age - start) / 2.0;
                if t >= 1.0 {
                    self.shown[i] = self.coming[i];
                    self.switch_start[i] = None;
                } else {
                    let form = if t < 0.5 {
                        self.shown[i]
                    } else {
                        self.coming[i]
                    };
                    e.forms[i] = form;
                    if !form.closed() {
                        *lid = lid.max(if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 });
                    }
                }
            }
        }
        let bags = if s.tired_eyes == "rings" { p[15] } else { 0.0 };
        if s.tired_eyes == "lids" {
            lower = lower.max(p[15] * 0.35);
        }
        FaceFrame {
            forms: e.forms,
            lids,
            lower: lower.clamp(0.0, 1.0),
            smile: p[3].clamp(0.0, 1.0),
            slant: p[1].clamp(-1.0, 1.0),
            wide: wide.clamp(0.0, 1.0),
            pupil,
            brows,
            tilt: tilt.clamp(-1.0, 1.0),
            blush: if s.blush {
                p[12].max(s.personality.blush).clamp(0.0, 1.0)
            } else {
                0.0
            },
            tears: p[13].clamp(0.0, 1.0),
            sweat: p[14].clamp(0.0, 1.0),
            bags: bags.clamp(0.0, 1.0),
            anger: p[16].clamp(0.0, 1.0),
            sparkle: p[17].clamp(0.0, 1.0),
            age_ticks: age,
            reaction: target.id,
        }
    }
    fn idle_weight(&mut self, allowed: bool, yaw: f32, pitch: f32, age: f32) -> f32 {
        if let Some((last_yaw, last_pitch, last_age)) = self.last_look
            && age > last_age
            && ((yaw - last_yaw).abs() + (pitch - last_pitch).abs()) / (age - last_age).max(0.05)
                >= 1.5
        {
            self.turned_at = age;
        }
        if self
            .last_look
            .is_none_or(|(_, _, last_age)| last_age != age)
        {
            self.last_look = Some((yaw, pitch, age));
        }
        let target = if allowed && age - self.turned_at >= 12.0 {
            1.0
        } else {
            0.0
        };
        if let Some(last_age) = self.idle_age.filter(|last| age >= *last) {
            let delta = (age - last_age) / 6.0;
            self.idle = if target > self.idle {
                target.min(self.idle + delta)
            } else {
                target.max(self.idle - delta)
            };
        } else {
            self.idle = target;
        }
        self.idle_age = Some(age);
        self.idle
    }
}
fn over_channels(
    mut base: FaceExpression,
    channels: &BTreeMap<String, [f32; 3]>,
    settings: &FaceSettings,
) -> FaceExpression {
    if channels.is_empty() {
        return base;
    }
    let mix = channels
        .get("face_mix")
        .map_or(0.0, |v| finite(v[1], 0.0))
        .clamp(0.0, 1.0);
    let mut offsets = [0.0; 21];
    let mut looks = BTreeMap::new();
    let mut shapes: [BTreeMap<&str, f32>; 2] = std::array::from_fn(|_| BTreeMap::new());
    let mut blink_off = false;
    for (name, position) in channels {
        let x = finite(position[0], 0.0);
        let y = finite(position[1], 0.0);
        match name.as_str() {
            "face_mix" => {}
            "face_blink_off" => blink_off = y > 0.5,
            "face_pupils" => {
                offsets[5] += x;
                offsets[6] -= y;
            }
            _ => {
                if let Some(name) = name.strip_prefix("face_look_") {
                    if y.abs() > 0.0001 {
                        looks.insert(name, y);
                    }
                } else if let Some(name) = name.strip_prefix("face_shape_") {
                    if let Some(name) = name.strip_suffix("_right") {
                        *shapes[0].entry(name).or_default() += y;
                    } else if let Some(name) = name.strip_suffix("_left") {
                        *shapes[1].entry(name).or_default() += y;
                    } else {
                        for eye in &mut shapes {
                            *eye.entry(name).or_default() += y;
                        }
                    }
                } else {
                    let parts: &[usize] = match name.as_str() {
                        "face_lids" => &[0],
                        "face_lid_right" => &[18],
                        "face_lid_left" => &[19],
                        "face_slant" => &[1],
                        "face_lower" => &[2],
                        "face_cheeks" => &[3],
                        "face_wide" => &[4],
                        "face_swing" => &[7],
                        "face_tremble" => &[8],
                        "face_brows" => &[9, 10],
                        "face_brow_right" => &[9],
                        "face_brow_left" => &[10],
                        "face_tilt" => &[11],
                        "face_blush" => &[12],
                        "face_tears" => &[13],
                        "face_sweat" => &[14],
                        "face_bags" => &[15],
                        "face_anger" => &[16],
                        "face_sparkle" => &[17],
                        "face_bob" => &[20],
                        _ => &[],
                    };
                    for &part in parts {
                        offsets[part] += y;
                    }
                }
            }
        }
    }
    // Zero/inactive channels must not remove a preset's custom high brows by
    // needlessly rebuilding/scaling the expression.
    if mix <= 0.01
        && !blink_off
        && looks.is_empty()
        && shapes.iter().all(BTreeMap::is_empty)
        && offsets.iter().all(|x| x.abs() <= 0.0001)
    {
        return base;
    }
    base.parts = std::array::from_fn(|i| (1.0 - mix) * base.parts[i] + offsets[i]);
    let mut strongest: Option<FaceExpression> = None;
    let mut strongest_weight = 0.0;
    for (name, weight) in looks {
        if !settings.custom_looks.contains_key(name)
            && !LOOK_NAMES.contains(&name)
            && ![
                "cross",
                "calm",
                "flutter",
                "narrow",
                "wink_left",
                "wink_right",
                "startled",
                "glum",
                "uneasy",
                "cheery",
                "intent",
                "aghast",
                "hurting",
                "drowsy",
                "laugh",
                "angry",
                "glance_left",
                "glance_right",
            ]
            .contains(&name)
        {
            continue;
        }
        let look = settings.look(name);
        for (part, offset) in base.parts.iter_mut().zip(look.parts) {
            *part += weight * offset;
        }
        if weight > strongest_weight {
            strongest_weight = weight;
            strongest = Some(look);
        }
    }
    for (side, shapes) in shapes.iter().enumerate() {
        let mut selected = None;
        let mut weight = 0.5;
        for (&name, &value) in shapes {
            if [
                "open", "line", "arc", "cup", "press", "cross", "spiral", "heart", "blank",
            ]
            .contains(&name)
                && value > weight
            {
                weight = value;
                selected = Some(FaceForm::parse(name));
            }
        }
        if let Some(shape) = selected {
            base.forms[side] = shape;
        } else if mix > 0.5 {
            base.forms[side] = strongest.as_ref().map_or(FaceForm::Open, |e| e.forms[side]);
        }
    }
    if blink_off {
        base.blink_pace = Some(0.0);
    } else if mix > 0.5 {
        base.blink_pace = strongest.and_then(|look| look.blink_pace);
    }
    base
}
fn ramp(x: f32, a: f32, b: f32) -> f32 {
    ((x - a) / (b - a)).clamp(0.0, 1.0)
}
pub(super) fn hash(id: u32, n: u32) -> u32 {
    let mut x = id
        .wrapping_mul(0x9e3779b1)
        .wrapping_add(n.wrapping_mul(0x85ebca6b));
    x ^= x >> 15;
    x = x.wrapping_mul(739982445);
    x ^= x >> 12;
    x = x.wrapping_mul(695872825);
    x ^ (x >> 15)
}
/// Java's hash is an i32; unsigned remainder changes blink/gaze schedules.
fn signed_mod(value: u32, modulus: i32) -> i32 {
    (value as i32).rem_euclid(modulus.max(1))
}
/// Deterministic five-tick asymmetric blink; one in eight includes a double.
/// `age` and `period` are ticks here, unlike the host-facing condition time.
pub fn blink(id: u32, age: f32, period: f32) -> f32 {
    if !age.is_finite() {
        return 0.0;
    }
    let period = finite(period, 72.0).max(12.5);
    let age = age + signed_mod(hash(id, 0), period as i32) as f32;
    let cycle = (age / period).floor() as i32;
    let mut beat = 0.0_f32;
    for n in cycle - 1..=cycle + 1 {
        let h = hash(id, n as u32);
        let start = (n as f32 + 0.4 * ((h & 65535) as f32 / 65535.0 - 0.5)) * period;
        beat = beat.max(blink_shape(age - start));
        if (h >> 16).is_multiple_of(8) {
            beat = beat.max(blink_shape(age - start - 7.5));
        }
    }
    beat
}
fn blink_shape(t: f32) -> f32 {
    if t <= 0.0 || t >= 5.0 {
        0.0
    } else if t < 1.8 {
        t / 1.8
    } else if t < 3.0 {
        1.0
    } else {
        (5.0 - t) / 2.0
    }
}
fn jitter(id: u32, age: f32, axis: u32) -> f32 {
    let t = age / 2.0;
    let i = t.floor() as i32;
    let key = id ^ if axis == 0 { 1759714724 } else { 458671337 };
    let a = signed_mod(hash(key, (i - 1) as u32), 3) as f32 - 1.0;
    let b = signed_mod(hash(key, i as u32), 3) as f32 - 1.0;
    let t = ((t - t.floor()) / 0.6).min(1.0);
    a + (b - a) * t * t * (3.0 - 2.0 * t)
}
fn glance_spell(id: u32, age: f32) -> (f32, f32) {
    let age = age + signed_mod(hash(id ^ 1540483477, 1), 800) as f32;
    let cycle = (age / 800.0).floor() as i32;
    for n in [cycle, cycle - 1] {
        let h = hash(id ^ 668265263, n as u32);
        let start = n as f32 * 800.0 + 400.0 * ((h & 65535) as f32 / 65535.0 - 0.5);
        let length = 40.0 + 40.0 * (h >> 16) as f32 / 65535.0;
        let t = age - start;
        if t >= 0.0 && t < length {
            let fade = (t / 6.0).min((length - t) / 6.0).min(1.0);
            let phase = t % 62.0;
            let side = if phase < 26.0 {
                -1.0
            } else if phase < 31.0 {
                -1.0 + 2.0 * (phase - 26.0) / 5.0
            } else if phase < 57.0 {
                1.0
            } else {
                1.0 - 2.0 * (phase - 57.0) / 5.0
            };
            return (fade, fade * side);
        }
    }
    (0.0, 0.0)
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    #[test]
    fn every_reaction_is_reachable_and_names_are_unique() {
        let mut ids = std::collections::BTreeSet::new();
        for &(id, _, _) in REACTIONS {
            assert!(ids.insert(id));
            if ["gaze", "still", "stride"].contains(&id) {
                continue;
            }
            let mut c = FaceConditions::default();
            c.activate(id, 1.0);
            let mut memory = FaceMemory::default();
            let frame = memory.frame(&c, &FaceSettings::default());
            assert_eq!(frame.reaction, id);
        }
    }
    #[test]
    fn death_overrides_happiness_and_disabled_reactions_fall_through() {
        let mut c = FaceConditions::default();
        c.dead = true;
        c.eating = true;
        let mut s = FaceSettings::default();
        let mut m = FaceMemory::default();
        assert_eq!(m.frame(&c, &s).reaction, "fading");
        s.reactions.insert("fading".into(), false);
        c.time_seconds = 1.0;
        assert_eq!(m.frame(&c, &s).reaction, "munch");
    }
    #[test]
    fn blinking_is_bounded_deterministic_and_not_synchronized() {
        let a: Vec<_> = (0..2000).map(|i| blink(5, i as f32 * 0.1, 72.0)).collect();
        let b: Vec<_> = (0..2000).map(|i| blink(6, i as f32 * 0.1, 72.0)).collect();
        assert_ne!(a, b);
        assert!(a.iter().all(|&x| (0.0..=1.0).contains(&x)));
        assert!(a.contains(&1.0));
        assert_eq!(blink(5, 88.0, 72.0), blink(5, 88.0, 72.0));
    }
    #[test]
    fn personality_can_skip_fear_and_preserve_directed_gaze() {
        let mut s = FaceSettings::default();
        s.personality = FacePersonality::preset("brave");
        let mut c = FaceConditions::default();
        c.cues.insert("hostile_near".into(), 1.0);
        c.cues.insert("hostile_side".into(), -1.0);
        let mut m = FaceMemory::default();
        let f = m.frame(&c, &s);
        assert!(f.slant > 0.0);
        assert!(f.pupil[0] < 0.0);
        s.personality.swaps.insert("nervous".into(), String::new());
        c.time_seconds = 1.0;
        assert_eq!(m.frame(&c, &s).reaction, "gaze");
    }
    #[test]
    fn signed_hash_remainders_match_blink_and_gaze_contract() {
        assert_eq!(signed_mod(u32::MAX, 72), 71);
        assert_eq!(signed_mod(0x8000_0000, 3), 1);
        assert_eq!(signed_mod(0x7fff_ffff, 3), 1);
    }
    #[test]
    fn walking_rate_does_not_mask_environment_or_custom_trigger_look() {
        let mut c = FaceConditions::default();
        c.moving = true;
        c.cues.insert("flowery".into(), 1.0);
        let mut s = FaceSettings::default();
        let m = FaceMemory::default();
        let winner = m.winner(&c, 0.0, &s);
        assert_eq!(winner.id, "bloom");
        assert_eq!(winner.look.blink_pace, Some(1.0));
        s.trigger_looks.insert("stride".into(), "shut".into());
        let winner = m.winner(&c, 0.0, &s);
        assert_eq!(winner.id, "stride");
        assert_eq!(winner.look.forms, [FaceForm::Line; 2]);
    }
    #[test]
    fn velocity_thresholds_are_blocks_per_second() {
        let s = FaceSettings::default();
        let m = FaceMemory::default();
        let mut c = FaceConditions::default();
        c.cues.insert("vehicle".into(), 2.0);
        c.cues.insert("vehicle_speed".into(), 4.99);
        assert!(!m.active("gallop", &c, 0.0, &s));
        c.cues.insert("vehicle_speed".into(), 5.0);
        assert!(m.active("gallop", &c, 0.0, &s));
        c.cues.insert("vehicle".into(), 3.0);
        c.cues.insert("vehicle_speed".into(), 1.99);
        assert!(!m.active("rail", &c, 0.0, &s));
        c.cues.insert("vehicle_speed".into(), 2.0);
        assert!(m.active("rail", &c, 0.0, &s));
        c.on_ground = false;
        c.vertical_speed = 2.0;
        assert!(!m.active("ascent", &c, 0.0, &s));
        c.vertical_speed = 2.01;
        assert!(m.active("ascent", &c, 0.0, &s));
        c.gliding = true;
        c.gliding_speed = 10.0;
        assert!(m.active("jet", &c, 0.0, &s));
        c.cues.insert("forward_velocity".into(), -6.0);
        assert!(!m.active("recoil", &c, 0.0, &s));
        c.cues.insert("forward_velocity".into(), -6.01);
        assert!(m.active("recoil", &c, 0.0, &s));
    }
    #[test]
    fn semantic_fall_weight_overrides_distance_heuristic() {
        let m = FaceMemory::default();
        let s = FaceSettings::default();
        let mut c = FaceConditions::default();
        c.on_ground = false;
        c.vertical_speed = -20.0;
        c.fall_distance = 2.0;
        c.cues.insert("fall_weight".into(), 2.0);
        assert!(m.active("freefall", &c, 0.0, &s));
        assert!(!m.active("plunge", &c, 0.0, &s));
        c.cues.insert("landing_weight".into(), 3.0);
        c.events.insert("landing".into(), 0.1);
        assert!(m.active("thud", &c, 0.0, &s));
    }
    #[test]
    fn idle_gaze_fades_out_on_turn_and_resumes_after_rest() {
        let mut m = FaceMemory::default();
        assert_eq!(m.idle_weight(true, 0.0, 0.0, 0.0), 1.0);
        assert!((m.idle_weight(true, 12.0, 0.0, 1.0) - 5.0 / 6.0).abs() < 0.0001);
        assert_eq!(m.idle_weight(true, 12.0, 0.0, 7.0), 0.0);
        assert_eq!(m.idle_weight(true, 12.0, 0.0, 13.0), 1.0);
        assert_eq!(m.idle_weight(false, 12.0, 0.0, 19.0), 0.0);
    }
    #[test]
    fn retargeting_preserves_the_current_eased_value() {
        let mut m = FaceMemory::default();
        let mut c = FaceConditions::default();
        c.forced_look = Some("tracking".into());
        let mut s = FaceSettings::default();
        s.blinking = false;
        s.glancing = false;
        s.custom_looks
            .insert("tracking".into(), FaceExpression::default());
        assert_eq!(m.frame(&c, &s).pupil[0], 0.0);
        s.custom_looks.get_mut("tracking").unwrap().parts[5] = 1.0;
        c.time_seconds = 0.05;
        assert_eq!(m.frame(&c, &s).pupil[0], 0.0);
        s.custom_looks.get_mut("tracking").unwrap().parts[5] = 0.5;
        c.time_seconds = 0.1;
        assert!((m.frame(&c, &s).pupil[0] - 1.0 / 3.0).abs() < 0.0001);
        c.time_seconds = 0.25;
        assert!((m.frame(&c, &s).pupil[0] - 0.5).abs() < 0.0001);
    }
    #[test]
    fn clip_channels_use_model_xy_offsets_and_weighted_looks() {
        let channels = BTreeMap::from([
            ("face_mix".into(), [0.0, 1.0, 0.0]),
            ("face_look_happy".into(), [0.0, 0.8, 0.0]),
            ("face_pupils".into(), [0.4, -0.2, 0.0]),
            ("face_brows".into(), [0.0, 0.2, 0.0]),
            ("face_shape_line_left".into(), [0.0, 0.9, 0.0]),
            ("face_blink_off".into(), [0.0, 1.0, 0.0]),
        ]);
        let e = over_channels(
            FaceExpression::named("sad"),
            &channels,
            &FaceSettings::default(),
        );
        assert_eq!(e.parts[0], 0.0);
        assert_eq!(e.parts[3], 0.8);
        assert_eq!(e.parts[5], 0.4);
        assert_eq!(e.parts[6], 0.2);
        assert!((e.parts[9] - 0.68).abs() < 0.0001);
        assert_eq!(e.forms, [FaceForm::Open, FaceForm::Line]);
        assert_eq!(e.blink_pace, Some(0.0));
    }
    #[test]
    fn clip_channels_do_not_override_death_or_enter_reaction_easing() {
        let mut c = FaceConditions::default();
        c.dead = true;
        c.face_channels.insert("face_lids".into(), [0.0, 1.0, 0.0]);
        let mut m = FaceMemory::default();
        let mut s = FaceSettings::default();
        s.blinking = false;
        s.glancing = false;
        assert_eq!(m.frame(&c, &s).lids, [0.0; 2]);
        c.dead = false;
        c.time_seconds = 1.0;
        assert_eq!(m.frame(&c, &s).lids, [1.0; 2]);
        c.face_channels.clear();
        c.time_seconds = 1.01;
        assert_eq!(m.frame(&c, &s).lids, [0.0; 2]);
    }
}
