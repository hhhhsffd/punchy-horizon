//! Native, asset-free facial animation for Minecraft RGBA skins.
//!
//! Integration: retain one `FaceEngine` per skin/player, call `render` with time
//! in **seconds**, and upload `FaceOutput::rgba` as the player's skin. Upload
//! `emission` as an optional emissive texture. Always retain the original skin:
//! feeding an already animated image back into detection loses its eye palette.
//! `overlay` is an alternative sparse replacement texture (not alpha blended
//! over `rgba`). Quads use head-local model pixels, Y down, front at Z = -4.
//! No JVM, Minecraft renderer, filesystem or bundled texture dependency.

use serde_json::{Value, json};
use std::collections::BTreeMap;

#[path = "moves_face_detect.rs"]
mod detect;
#[path = "moves_face_editor.rs"]
mod editor;
#[path = "moves_face_metadata.rs"]
mod metadata;
#[path = "moves_face_motion.rs"]
mod motion;
#[path = "moves_face_render.rs"]
mod render;

pub use detect::{Brow, Eye, FaceRig, SkinGrid};
pub use editor::{
    EditorAction, EditorFrame, EditorInput, EditorKey, EditorPrimitive, EditorTool, FaceEditor,
};
pub use metadata::{FaceMarks, SkinMetadata, export_skin_metadata, import_skin_metadata};
pub use motion::{FaceConditions, FaceExpression, FaceForm, FaceFrame, FacePersonality, blink};

pub type Rgba = [u8; 4];
pub const GLOW_WHITES: u8 = 1;
pub const GLOW_PUPILS: u8 = 2;
pub const GLOW_BROWS: u8 = 4;
pub const GLOW_SPARKLES: u8 = 8;
pub const EFFECT_BLOOM: u8 = 1;
pub const EFFECT_GLARE: u8 = 2;
pub const EFFECT_TRAIL: u8 = 4;
pub const EFFECT_TRACER: u8 = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct EyeLight {
    pub parts: u8,
    pub effects: u8,
    /// Bloom, glare, trail and tracer strength, respectively.
    pub strengths: [f32; 4],
    pub color: Option<Rgba>,
    pub trail_color: Option<Rgba>,
}
impl Default for EyeLight {
    fn default() -> Self {
        Self {
            parts: 0,
            effects: 0,
            strengths: [1.0; 4],
            color: None,
            trail_color: None,
        }
    }
}

/// Serializable preferences. Color `None` means sample the original skin.
/// Eye order is avatar right (image left), then avatar left.
#[derive(Clone, Debug)]
pub struct FaceSettings {
    pub enabled: bool,
    pub on_other_players: bool,
    pub over_hat: bool,
    pub hat_layer: bool,
    pub brows: bool,
    pub blinking: bool,
    pub glancing: bool,
    pub blink_seconds: f32,
    pub daydream_seconds: f32,
    pub expression_strength: f32,
    /// `off`, `bar`, `dot`, or `skin`.
    pub sparkle: String,
    pub sparkle_color: Rgba,
    pub line_color: Option<Rgba>,
    pub lid_colors: [Option<Rgba>; 2],
    pub blush: bool,
    pub blush_color: Option<Rgba>,
    /// `rings`, `lids`, or `off`.
    pub tired_eyes: String,
    pub ring_color: Option<Rgba>,
    pub lights: [EyeLight; 2],
    pub glow_strength: f32,
    pub glow_pulse: bool,
    pub glow_night_only: bool,
    pub personality: FacePersonality,
    pub reactions: BTreeMap<String, bool>,
    pub trigger_looks: BTreeMap<String, String>,
    pub custom_looks: BTreeMap<String, FaceExpression>,
}
impl Default for FaceSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            on_other_players: false,
            over_hat: false,
            hat_layer: true,
            brows: true,
            blinking: true,
            glancing: true,
            blink_seconds: 3.6,
            daydream_seconds: 30.0,
            expression_strength: 1.0,
            sparkle: "off".into(),
            sparkle_color: [255; 4],
            line_color: None,
            lid_colors: [None; 2],
            blush: true,
            blush_color: None,
            tired_eyes: "rings".into(),
            ring_color: None,
            lights: [EyeLight::default(), EyeLight::default()],
            glow_strength: 1.0,
            glow_pulse: false,
            glow_night_only: false,
            personality: FacePersonality::preset("classic"),
            reactions: BTreeMap::new(),
            trigger_looks: BTreeMap::new(),
            custom_looks: BTreeMap::new(),
        }
    }
}
impl FaceSettings {
    pub fn reaction_enabled(&self, id: &str) -> bool {
        self.reactions.get(id).copied().unwrap_or(true)
    }
    pub fn look(&self, name: &str) -> FaceExpression {
        self.custom_looks
            .get(name)
            .cloned()
            .unwrap_or_else(|| FaceExpression::named(name))
    }
    pub fn normalize(&mut self) {
        self.blink_seconds = finite(self.blink_seconds, 3.6).clamp(1.5, 8.0);
        self.daydream_seconds = finite(self.daydream_seconds, 30.0).clamp(10.0, 120.0);
        self.expression_strength = finite(self.expression_strength, 1.0).clamp(0.25, 1.5);
        self.glow_strength = finite(self.glow_strength, 1.0).clamp(0.1, 2.0);
        if !["off", "bar", "dot", "skin"].contains(&self.sparkle.as_str()) {
            self.sparkle = "off".into();
        }
        if !["rings", "lids", "off"].contains(&self.tired_eyes.as_str()) {
            self.tired_eyes = "rings".into();
        }
        for eye in &mut self.lights {
            eye.parts &= 15;
            eye.effects &= 15;
            for strength in &mut eye.strengths {
                *strength = finite(*strength, 1.0).clamp(0.1, 2.0);
            }
        }
    }
    /// Reads the original Expressions `face.json` field names, tolerating
    /// unknown or malformed individual fields instead of rejecting the file.
    pub fn from_json(v: &Value) -> Self {
        let mut s = Self::default();
        macro_rules! boolean {
            ($key:literal, $field:ident) => {
                if let Some(x) = v[$key].as_bool() {
                    s.$field = x;
                }
            };
        }
        macro_rules! number {
            ($key:literal, $field:ident) => {
                if let Some(x) = v[$key].as_f64() {
                    s.$field = x as f32;
                }
            };
        }
        boolean!("faces", enabled);
        boolean!("on_other_players", on_other_players);
        boolean!("over_hat", over_hat);
        boolean!("hat_layer", hat_layer);
        boolean!("brows", brows);
        boolean!("blinking", blinking);
        boolean!("glances", glancing);
        boolean!("blush", blush);
        boolean!("glow_pulse", glow_pulse);
        boolean!("glow_night_only", glow_night_only);
        number!("blink_seconds", blink_seconds);
        number!("daydream_seconds", daydream_seconds);
        number!("expression_strength", expression_strength);
        number!("glow_strength", glow_strength);
        if let Some(x) = v["sparkle"].as_str() {
            s.sparkle = x.into();
        }
        if let Some(x) = v["tired_eyes"].as_str() {
            s.tired_eyes = x.into();
        }
        s.sparkle_color = parse_color(&v["sparkle_color"]).unwrap_or([255; 4]);
        s.line_color = parse_color(&v["line_color"]);
        s.blush_color = parse_color(&v["blush_color"]);
        s.ring_color = parse_color(&v["ring_color"]);
        s.lid_colors[1] = parse_color(&v["left_lid"]);
        s.lid_colors[0] = parse_color(&v["right_lid"]).or(s.lid_colors[1]);
        s.lights[1] = read_light(v);
        s.lights[0] = if v["eyes_apart"].as_bool() == Some(true) {
            read_light(&v["right_eye"])
        } else {
            s.lights[1].clone()
        };
        if let Some(name) = v["personality"].as_str() {
            s.personality = if v["personalities"][name].is_object() {
                FacePersonality::from_json(name, &v["personalities"][name])
            } else {
                FacePersonality::preset(name)
            };
        } else if v["personality"].is_object() {
            s.personality = FacePersonality::from_json("custom", &v["personality"]);
        }
        if let Some(o) = v["reactions"].as_object() {
            for (k, x) in o {
                if let Some(b) = x.as_bool() {
                    s.reactions.insert(k.clone(), b);
                }
            }
        }
        if let Some(o) = v["trigger_looks"].as_object() {
            for (k, x) in o {
                if let Some(n) = x.as_str() {
                    s.trigger_looks.insert(k.clone(), n.into());
                }
            }
        }
        if let Some(o) = v["looks"].as_object() {
            for (k, x) in o {
                s.custom_looks
                    .insert(k.clone(), FaceExpression::from_json(k, x));
            }
        }
        s.normalize();
        s
    }
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "faces": self.enabled, "on_other_players": self.on_other_players,
            "over_hat": self.over_hat, "hat_layer": self.hat_layer,
            "brows": self.brows, "blinking": self.blinking, "glances": self.glancing,
            "blink_seconds": self.blink_seconds, "daydream_seconds": self.daydream_seconds,
            "expression_strength": self.expression_strength, "sparkle": self.sparkle,
            "sparkle_color": color_json(Some(self.sparkle_color)),
            "line_color": color_json(self.line_color),
            "left_lid": color_json(self.lid_colors[1]), "right_lid": color_json(self.lid_colors[0]),
            "blush": self.blush, "blush_color": color_json(self.blush_color),
            "tired_eyes": self.tired_eyes, "ring_color": color_json(self.ring_color),
            "glow_strength": self.glow_strength, "glow_pulse": self.glow_pulse,
            "glow_night_only": self.glow_night_only, "eyes_apart": true,
            "right_eye": light_json(&self.lights[0]),
            "personality": self.personality.id,
            "personalities": {self.personality.id.clone(): self.personality.to_json()},
            "reactions": self.reactions, "trigger_looks": self.trigger_looks,
            "looks": self.custom_looks.iter().map(|(k,v)| (k.clone(),v.to_json())).collect::<BTreeMap<_,_>>()
        });
        if let (Some(a), Some(b)) = (v.as_object_mut(), light_json(&self.lights[1]).as_object()) {
            a.extend(b.clone());
        }
        v
    }
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        Ok(Self::from_json(&v))
    }
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let data = serde_json::to_vec_pretty(&self.to_json()).map_err(|e| e.to_string())?;
        std::fs::write(path, data).map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct FaceQuad {
    pub positions: [[f32; 3]; 4],
    pub normal: [f32; 3],
    pub color: Rgba,
    pub emission: Rgba,
}
#[derive(Clone, Debug)]
pub struct GlowAnchor {
    pub position: [f32; 3],
    pub color: Rgba,
    pub strength: f32,
    pub effects: u8,
    pub effect_strengths: [f32; 4],
    pub trail_color: Rgba,
}
#[derive(Clone, Debug)]
pub struct FaceOutput {
    pub width: u32,
    pub height: u32,
    /// Complete original skin with this frame's facial replacements.
    pub rgba: Vec<u8>,
    /// Sparse replacement pixels: alpha 0 means unchanged.
    pub overlay: Vec<u8>,
    /// Linear intensity is additionally available through `glow_anchors`.
    pub emission: Vec<u8>,
    /// Optional per-pixel face geometry. Empty unless `conditions.quads`.
    pub quads: Vec<FaceQuad>,
    pub glow_anchors: Vec<GlowAnchor>,
    pub frame: FaceFrame,
    pub detected: bool,
}

pub struct FaceEngine {
    pub settings: FaceSettings,
    pub rig: FaceRig,
    /// Valid embedded marks permit remote animation without enabling automatic
    /// detection on all other players.
    pub marked: bool,
    pub embedded_settings: Option<FaceSettings>,
    source: Vec<u8>,
    grid: SkinGrid,
    width: u32,
    height: u32,
    memory: motion::FaceMemory,
    marks: Option<FaceMarks>,
    detected_hat_layer: bool,
}
impl FaceEngine {
    pub fn new(
        rgba: &[u8],
        width: u32,
        height: u32,
        mut settings: FaceSettings,
    ) -> Result<Self, String> {
        settings.normalize();
        let grid = SkinGrid::new(rgba, width, height)?;
        let embedded = metadata::read_grid(&grid);
        let embedded_settings = embedded.as_ref().and_then(|m| m.settings.clone());
        if settings.to_json() == FaceSettings::default().to_json()
            && let Some(style) = &embedded_settings
        {
            settings = style.clone();
        }
        let marks = embedded.as_ref().map(|m| m.marks.clone());
        let rig = detect::detect(&grid, settings.hat_layer, marks.as_ref());
        let detected_hat_layer = settings.hat_layer;
        Ok(Self {
            settings,
            rig,
            source: rgba[..width as usize * height as usize * 4].to_vec(),
            marked: embedded.is_some(),
            embedded_settings,
            grid,
            width,
            height,
            memory: motion::FaceMemory::default(),
            marks,
            detected_hat_layer,
        })
    }
    /// Explicit user marks always take precedence over automatic detection.
    pub fn set_marks(&mut self, marks: Option<&FaceMarks>) {
        self.marked = marks.is_some();
        self.marks = marks.cloned();
        self.rig = detect::detect(&self.grid, self.settings.hat_layer, self.marks.as_ref());
        self.detected_hat_layer = self.settings.hat_layer;
    }
    /// Apply live preferences without losing the original pixels or explicit
    /// embedded/editor marks. Hat-layer changes require fresh palette sampling.
    pub fn set_settings(&mut self, mut settings: FaceSettings) {
        settings.normalize();
        let redetect = settings.hat_layer != self.detected_hat_layer;
        self.settings = settings;
        if redetect {
            self.rig = detect::detect(&self.grid, self.settings.hat_layer, self.marks.as_ref());
            self.detected_hat_layer = self.settings.hat_layer;
        }
    }
    /// Discard explicit marks and return to automatic eye/brow detection.
    pub fn redetect(&mut self) {
        self.marks = None;
        self.marked = false;
        self.rig = detect::detect(&self.grid, self.settings.hat_layer, None);
        self.detected_hat_layer = self.settings.hat_layer;
    }
    pub fn reset_animation(&mut self) {
        self.memory = motion::FaceMemory::default();
    }
    pub fn original_rgba(&self) -> &[u8] {
        &self.source
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn metadata(&self) -> SkinMetadata {
        SkinMetadata {
            marks: self.rig.marks(),
            settings: Some(self.settings.clone()),
        }
    }
    pub fn render(&mut self, conditions: &FaceConditions) -> FaceOutput {
        let frame = self.memory.frame(conditions, &self.settings);
        let mut output = FaceOutput {
            width: self.width,
            height: self.height,
            rgba: self.source.clone(),
            overlay: vec![0; self.source.len()],
            emission: vec![0; self.source.len()],
            quads: Vec::new(),
            glow_anchors: Vec::new(),
            frame,
            detected: !self.rig.eyes.is_empty(),
        };
        if self.settings.enabled
            && (conditions.own_player || self.settings.on_other_players || self.marked)
            && !conditions.hidden
            && conditions.opacity > 0.0
        {
            render::draw(self, conditions, &mut output);
        }
        output
    }
}

pub(super) fn finite(x: f32, fallback: f32) -> f32 {
    if x.is_finite() { x } else { fallback }
}
pub(super) fn luminance(c: Rgba) -> f32 {
    (c[0] as f32 * 299.0 + c[1] as f32 * 587.0 + c[2] as f32 * 114.0) / 1000.0
}
pub(super) fn distance(a: Rgba, b: Rgba) -> f32 {
    (0..3)
        .map(|i| (a[i] as f32 - b[i] as f32).powi(2))
        .sum::<f32>()
        .sqrt()
}
pub(super) fn chroma(c: Rgba) -> f32 {
    *c[..3].iter().max().unwrap() as f32 - *c[..3].iter().min().unwrap() as f32
}
pub(super) fn over(a: Rgba, b: Rgba) -> Rgba {
    let mut c = [0, 0, 0, 255];
    for i in 0..3 {
        c[i] = ((a[i] as u32 * a[3] as u32 + b[i] as u32 * (255 - a[3] as u32) + 127) / 255) as u8;
    }
    c
}
pub(super) fn mix_color(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let mut c = [0, 0, 0, 255];
    for i in 0..4 {
        c[i] = (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t.clamp(0.0, 1.0)).round() as u8;
    }
    c
}
pub(super) fn parse_color(v: &Value) -> Option<Rgba> {
    let n = if let Some(s) = v.as_str() {
        if s == "auto" {
            return None;
        }
        u32::from_str_radix(s.trim_start_matches('#'), 16).ok()?
    } else {
        v.as_u64()? as u32
    };
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8, 255])
}
pub(super) fn color_json(c: Option<Rgba>) -> Value {
    c.map(|c| json!(format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])))
        .unwrap_or(json!("auto"))
}
fn read_light(v: &Value) -> EyeLight {
    let mut l = EyeLight::default();
    for (i, name) in ["whites", "pupils", "brows", "sparkles"].iter().enumerate() {
        if v["glow"][name].as_bool() == Some(true) {
            l.parts |= 1 << i;
        }
    }
    for (i, name) in ["bloom", "glare", "trail", "tracer"].iter().enumerate() {
        if v["glow_effects"][name].as_bool() == Some(true) {
            l.effects |= 1 << i;
        }
        if let Some(x) = v["glow_effect_strengths"][name].as_f64() {
            l.strengths[i] = x as f32;
        }
    }
    l.color = parse_color(&v["glow_color"]);
    l.trail_color = parse_color(&v["trail_color"]);
    l
}
fn light_json(l: &EyeLight) -> Value {
    let mut glow = serde_json::Map::new();
    let mut effects = serde_json::Map::new();
    let mut strengths = serde_json::Map::new();
    for (i, n) in ["whites", "pupils", "brows", "sparkles"].iter().enumerate() {
        glow.insert((*n).into(), json!(l.parts & (1 << i) != 0));
    }
    for (i, n) in ["bloom", "glare", "trail", "tracer"].iter().enumerate() {
        effects.insert((*n).into(), json!(l.effects & (1 << i) != 0));
        strengths.insert((*n).into(), json!(l.strengths[i]));
    }
    json!({"glow": glow, "glow_effects": effects, "glow_effect_strengths": strengths,
        "glow_color": color_json(l.color), "trail_color": color_json(l.trail_color)})
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    #[test]
    fn settings_json_round_trip_and_validation() {
        let mut s = FaceSettings::from_json(&json!({"blink_seconds": 90, "expression_strength": 0,
            "glow": {"pupils": true}, "personality": "cute", "line_color": "#123456"}));
        assert_eq!(s.blink_seconds, 8.0);
        assert_eq!(s.expression_strength, 0.25);
        assert_eq!(s.lights[0].parts, GLOW_PUPILS);
        s.lid_colors[0] = Some([11, 22, 33, 255]);
        let decoded = FaceSettings::from_json(&s.to_json());
        assert_eq!(decoded.lid_colors, s.lid_colors);
        assert_eq!(decoded.personality.id, "cute");
    }
    #[test]
    fn invalid_skin_is_rejected_before_allocation() {
        assert!(FaceEngine::new(&[], 1024, 1024, FaceSettings::default()).is_err());
        assert!(SkinGrid::new(&[], u32::MAX, u32::MAX).is_err());
    }
    #[test]
    fn live_settings_preserve_source_and_embedded_or_manual_marks() {
        let mut rgba = [180, 130, 95, 255].repeat(64 * 64);
        let mut marks = FaceMarks::default();
        marks.set(1, 4, 1);
        marks.set(2, 4, 2);
        export_skin_metadata(
            &mut rgba,
            64,
            64,
            &SkinMetadata {
                marks: marks.clone(),
                settings: None,
            },
        )
        .unwrap();
        let mut engine = FaceEngine::new(&rgba, 64, 64, FaceSettings::default()).unwrap();
        let mut settings = engine.settings.clone();
        settings.hat_layer = false;
        settings.blink_seconds = f32::NAN;
        engine.set_settings(settings);
        assert_eq!(engine.marks.as_ref(), Some(&marks));
        assert!(engine.marked);
        assert!(!engine.detected_hat_layer);
        assert_eq!(engine.settings.blink_seconds, 3.6);
        assert_eq!(engine.original_rgba(), rgba);
        marks.set(6, 4, 2);
        engine.set_marks(Some(&marks));
        let mut settings = engine.settings.clone();
        settings.hat_layer = true;
        engine.set_settings(settings);
        assert_eq!(engine.marks.as_ref(), Some(&marks));
        assert_eq!(engine.original_rgba(), rgba);
    }
}
