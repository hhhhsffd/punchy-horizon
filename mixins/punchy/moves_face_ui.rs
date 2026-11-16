// Modal on the native Punchy options page. All coordinates are GUI coordinates,
// after Spatial GUI's inverse mapping; no raw window input or camera ownership.
use super::Game;
use horizon_client::mobs::anim::punchy_moves::engine::moves_expressions::{
    EditorAction, EditorInput, EditorKey, EditorPrimitive, EditorTool, FaceConditions, FaceEditor,
    FaceExpression,
};
use serde_json::{Value, json};
use std::cell::RefCell;

const ID: &str = "punchy";
const ROWS: usize = 13;
thread_local! { static UI: RefCell<Option<Session>> = const { RefCell::new(None) }; }
thread_local! { static POINTER: std::cell::Cell<[f32; 2]> = const { std::cell::Cell::new([0.; 2]) }; }
thread_local! { static OPEN_REQUEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
pub(super) fn shortcut(game: &Game<'_>, key: &str, repeat: bool) -> bool {
    if !game.native_on(ID)
        || game.player.is_none()
        || game.screen != super::Screen::Playing
        || game.screen_over_playing()
        || key != game.native_mod_key(ID, "face_editor", "key.keyboard.f6")
    {
        return false;
    }
    if !repeat {
        OPEN_REQUEST.with(|request| request.set(true));
    }
    true
}
pub(super) fn flush_shortcut(game: &mut Game<'_>) {
    if !OPEN_REQUEST.with(|request| request.replace(false)) {
        return;
    }
    let Some(index) = game.settings.mods.installed.iter().position(|m| m.id == ID) else {
        return;
    };
    if game.player.is_none() || game.screen != super::Screen::Playing {
        return;
    }
    game.capture(false);
    game.open_options_at(false, horizon_client::options_ui::Page::NativeMod(index));
    game.native_settings_refresh(index);
    open(game);
}
pub(super) fn pointer() -> [f32; 2] {
    POINTER.with(|point| point.get())
}
struct Session {
    editor: Option<FaceEditor>,
    message: String,
    tab: usize,
    page: usize,
    scale: f32,
    height: f32,
    down: bool,
    right: bool,
    shape: String,
}
pub(super) fn clear() {
    UI.with(|s| *s.borrow_mut() = None);
}
fn owner(game: &Game<'_>) -> bool {
    use horizon_client::options_ui::Page;
    matches!(game.ui.options.page(), Page::NativeMod(i)
        if game.settings.mods.installed.get(i).is_some_and(|m| m.id == ID))
        && game.screen == super::Screen::Options
}
pub(super) fn active(game: &Game<'_>) -> bool {
    owner(game) && UI.with(|s| s.borrow().is_some())
}
pub(super) fn lifecycle(game: &Game<'_>) {
    if !owner(game) {
        clear();
    }
}
pub(super) fn diagnostic() -> Option<Value> {
    UI.with(|ui| ui.borrow().as_ref().map(|s| json!({
        "tab":s.tab,"page":s.page,"scale":s.scale,"height":s.height,
        "editor":s.editor.as_ref().map(|e|json!({"dirty":e.dirty,
            "marks":e.marks.format(),"look":e.preview_look,"settings":e.engine.settings.to_json()})),
        "message":s.message,
    })))
}
pub(super) fn open(game: &Game<'_>) {
    let (editor, message) = match super::punchy_faces::editor(game) {
        Ok(editor) => (
            Some(editor),
            "Edits apply when saved. Back closes without applying unsaved edits.".into(),
        ),
        Err(error) => (None, error),
    };
    UI.with(|s| {
        *s.borrow_mut() = Some(Session {
            editor,
            message,
            tab: 0,
            page: 0,
            scale: 1.,
            height: 500.,
            down: false,
            right: false,
            shape: "user_shape".into(),
        })
    });
}
fn input(s: &mut Session, command: EditorInput) {
    if let Some(editor) = &mut s.editor {
        editor.input(command);
    }
}
fn command(s: &mut Session, key: EditorKey) {
    input(
        s,
        EditorInput::Key {
            key,
            ctrl: false,
            shift: false,
        },
    );
}
pub(super) fn key(game: &Game<'_>, key: winit::keyboard::KeyCode) -> bool {
    if !active(game) {
        return false;
    }
    use winit::keyboard::KeyCode as K;
    if key == K::Escape {
        clear();
        return true;
    }
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let s = ui.as_mut().unwrap();
        let ctrl = game.has_control_down();
        let shift = game.has_shift_down();
        let action = match key {
            K::Enter => Some(EditorKey::Save),
            K::KeyS if ctrl => Some(EditorKey::Save),
            K::KeyZ if ctrl => Some(if shift {
                EditorKey::Redo
            } else {
                EditorKey::Undo
            }),
            K::KeyY if ctrl => Some(EditorKey::Redo),
            K::Digit1 => Some(EditorKey::Erase),
            K::Digit2 => Some(EditorKey::Eye),
            K::Digit3 => Some(EditorKey::Pupil),
            K::Digit4 => Some(EditorKey::Brow),
            K::Digit5 => Some(EditorKey::Fill),
            K::BracketLeft => Some(EditorKey::PreviewPrevious),
            K::BracketRight => Some(EditorKey::PreviewNext),
            K::KeyR if !ctrl => Some(EditorKey::Reset),
            K::KeyE if ctrl => Some(EditorKey::Export),
            _ => None,
        };
        if let Some(key) = action {
            command(s, key);
        }
        flush(game, s);
    });
    true
}
pub(super) fn mouse(game: &Game<'_>, pressed: bool, right: bool) -> bool {
    if !active(game) {
        return false;
    }
    let mut close = false;
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let s = ui.as_mut().unwrap();
        let (x, y) = (game.cursor.0 / s.scale, game.cursor.1 / s.scale);
        s.down = pressed && s.tab == 0 && y >= 118. && y < s.height - 100.;
        s.right = right;
        if !pressed {
            input(
                s,
                EditorInput::Pointer {
                    x,
                    y: y - 30.,
                    down: false,
                    right_button: right,
                },
            );
            return;
        }
        if y < 28. {
            match (x / 100.).floor() as usize {
                0..=4 => {
                    s.tab = (x / 100.).floor() as usize;
                    s.page = 0;
                }
                5 => {
                    command(s, EditorKey::Save);
                    flush(game, s);
                }
                6 => close = true,
                _ => {}
            }
            return;
        }
        if s.tab == 4 {
            if let Some(editor) = &mut s.editor {
                if (54.0..90.0).contains(&y) {
                    let mut shapes = editor
                        .engine
                        .settings
                        .shapes
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>();
                    if !shapes.contains(&s.shape) {
                        shapes.push(s.shape.clone());
                    }
                    shapes.sort();
                    let i = shapes.iter().position(|id| *id == s.shape).unwrap_or(0);
                    s.shape = shapes[(i + if right { shapes.len() - 1 } else { 1 }) % shapes.len()]
                        .clone();
                } else if (110.0..230.0).contains(&y) && (20.0..180.0).contains(&x) {
                    paint_shape(
                        editor,
                        &s.shape,
                        ((x - 20.) / 20.) as usize,
                        ((y - 110.) / 20.) as usize,
                        right,
                    );
                } else if (250.0..350.0).contains(&y) {
                    let fields = ["closed", "on_white", "mirror"];
                    let row = ((y - 250.) / 26.) as usize;
                    if let Some(field) = fields.get(row) {
                        ensure_shape(editor, &s.shape);
                        Control {
                            label: (*field).into(),
                            path: format!("/shapes/{}/{field}", s.shape),
                            kind: Kind::Toggle,
                        }
                        .adjust(editor, right, false);
                    }
                } else if y >= s.height - 48. {
                    // Create another editable shape, or attach this shape to the preview look.
                    if x < 150. {
                        s.shape = format!("user_shape_{}", editor.engine.settings.shapes.len() + 1);
                        ensure_shape(editor, &s.shape);
                    } else {
                        ensure_shape(editor, &s.shape);
                        let name = editor.preview_look.clone();
                        let mut profile = editor.engine.settings.to_json();
                        let mut look = editor.engine.settings.look(&name).to_json();
                        look["right_form"] = json!(s.shape);
                        look["left_form"] = json!(s.shape);
                        profile["looks"][name] = look;
                        editor.input(EditorInput::Settings(profile));
                    }
                }
            }
        } else if s.tab == 0 {
            // Additional ordinary controls below the editor's existing toolbar.
            if y >= s.height - 48. {
                match (x / 100.).floor() as usize {
                    0 => input(s, EditorInput::Undo),
                    1 => input(s, EditorInput::Redo),
                    2 => command(
                        s,
                        if right {
                            EditorKey::PreviewPrevious
                        } else {
                            EditorKey::PreviewNext
                        },
                    ),
                    3 => command(s, EditorKey::ToggleSides),
                    4 => input(s, EditorInput::SelectTool(EditorTool::Fill)),
                    5 => input(s, EditorInput::ExportMarks),
                    _ => {}
                }
            } else {
                input(
                    s,
                    EditorInput::Pointer {
                        x,
                        y: y - 30.,
                        down: true,
                        right_button: right,
                    },
                );
            }
        } else if y >= s.height - 48. {
            let count = s.editor.as_ref().map_or(0, |e| controls(e, s.tab).len());
            let pages = count.div_ceil(ROWS).max(1);
            s.page = if right || x < 150. {
                (s.page + pages - 1) % pages
            } else {
                (s.page + 1) % pages
            };
        } else if (54.0..54.0 + ROWS as f32 * 26.).contains(&y) {
            if let Some(editor) = &mut s.editor {
                let list = controls(editor, s.tab);
                if let Some(control) = list.get(s.page * ROWS + ((y - 54.) / 26.) as usize) {
                    control.adjust(editor, right, game.has_shift_down());
                }
            }
        }
    });
    UI.with(|ui| {
        if let Some(s) = ui.borrow_mut().as_mut() {
            flush(game, s);
        }
    });
    if close {
        clear();
    }
    true
}

pub(super) fn scroll(game: &Game<'_>, amount: f32) -> bool {
    if !active(game) {
        return false;
    }
    UI.with(|ui| {
        if let Some(s) = ui.borrow_mut().as_mut() {
            if s.tab == 0 {
                command(
                    s,
                    if amount < 0. {
                        EditorKey::PreviewNext
                    } else {
                        EditorKey::PreviewPrevious
                    },
                );
            } else {
                let pages = s
                    .editor
                    .as_ref()
                    .map_or(1, |e| controls(e, s.tab).len().div_ceil(ROWS).max(1));
                s.page = (s.page + if amount < 0. { 1 } else { pages - 1 }) % pages;
            }
        }
    });
    true
}

enum Kind {
    Toggle,
    Number(f64, f64, f64),
    Choice(Vec<String>),
    Color,
    ColorChannel(usize),
}
struct Control {
    label: String,
    path: String,
    kind: Kind,
}
impl Control {
    fn value(&self, e: &FaceEditor) -> String {
        if self.path == "/@preview" {
            return e.preview_look.clone();
        }
        let mut profile = e.engine.settings.to_json();
        if let Some(name) = self
            .path
            .strip_prefix("/looks/")
            .and_then(|p| p.split('/').next())
        {
            if profile["looks"].get(name).is_none() {
                profile["looks"][name] = e.engine.settings.look(name).to_json();
            }
        }
        let fallback = self
            .path
            .strip_prefix("/trigger_priorities/")
            .map(|id| json!(FaceConditions::reaction_priority(id)))
            .unwrap_or(Value::Null);
        let value = profile.pointer(&self.path).unwrap_or(&fallback);
        match (&self.kind, value) {
            (Kind::ColorChannel(channel), _) => rgb(value)[*channel].to_string(),
            (Kind::Toggle, _) => if value.as_bool().unwrap_or(true) {
                "ON"
            } else {
                "OFF"
            }
            .into(),
            (_, Value::String(s)) => s.clone(),
            (Kind::Choice(items), Value::Null) if items.first().is_some_and(|s| s == "auto") => {
                "auto".into()
            }
            (_, Value::Null) => "skin".into(),
            _ => value.to_string(),
        }
    }
    fn adjust(&self, e: &mut FaceEditor, back: bool, fine: bool) {
        if self.path == "/@preview" {
            if let Kind::Choice(items) = &self.kind {
                let i = items.iter().position(|s| *s == e.preview_look).unwrap_or(0);
                e.input(EditorInput::SetLook(
                    items[(i + if back { items.len() - 1 } else { 1 }) % items.len()].clone(),
                ));
            }
            return;
        }
        let mut profile = e.engine.settings.to_json();
        if let Some(name) = self
            .path
            .strip_prefix("/looks/")
            .and_then(|p| p.split('/').next())
        {
            if profile["looks"].get(name).is_none() {
                profile["looks"][name] = e.engine.settings.look(name).to_json();
            }
        }
        let old = profile.pointer(&self.path).cloned().unwrap_or_else(|| {
            self.path
                .strip_prefix("/trigger_priorities/")
                .map(|id| json!(FaceConditions::reaction_priority(id)))
                .unwrap_or(Value::Null)
        });
        let value = match &self.kind {
            Kind::Toggle => json!(!old.as_bool().unwrap_or(true)),
            Kind::Number(step, min, max) => json!(
                (old.as_f64().unwrap_or(*min)
                    + step * if fine { 0.1 } else { 1. } * if back { -1. } else { 1. })
                .clamp(*min, *max)
            ),
            Kind::Choice(items) => {
                let i = items
                    .iter()
                    .position(|s| Some(s.as_str()) == old.as_str())
                    .unwrap_or(0);
                let next = &items[(i + if back { items.len() - 1 } else { 1 }) % items.len()];
                if next == "auto" {
                    Value::Null
                } else {
                    json!(next)
                }
            }
            Kind::Color => {
                // Palette cycling is complemented by exact RGB controls below.
                let choices = [
                    "skin", "#FFFFFF", "#111827", "#FF637D", "#73BFFF", "#79F2B2", "#F9D66E",
                    "#BA91FF",
                ];
                let i = choices
                    .iter()
                    .position(|s| Some(*s) == old.as_str())
                    .unwrap_or(0);
                json!(choices[(i + if back { choices.len() - 1 } else { 1 }) % choices.len()])
            }
            Kind::ColorChannel(channel) => {
                let mut color = rgb(&old);
                color[*channel] = (i32::from(color[*channel])
                    + if back { -1 } else { 1 } * if fine { 1 } else { 8 })
                .clamp(0, 255) as u8;
                json!(format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2]))
            }
        };
        set_path(
            &mut profile,
            self.path.trim_start_matches('/').split('/'),
            value,
        );
        e.input(EditorInput::Settings(profile));
    }
}
fn rgb(value: &Value) -> [u8; 3] {
    let hex = value.as_str().unwrap_or("FFFFFF").trim_start_matches('#');
    let color = u32::from_str_radix(hex, 16).unwrap_or(0xffffff);
    [(color >> 16) as u8, (color >> 8) as u8, color as u8]
}
fn ensure_shape(editor: &mut FaceEditor, id: &str) {
    if !editor.engine.settings.shapes.contains_key(id) {
        let shape = json!({"closed": false, "on_white": false, "mirror": true,
            "color": "line", "grids": {"8x6": ["........", "........", "........", "........", "........", "........"]}});
        let mut profile = editor.engine.settings.to_json();
        profile["shapes"][id] = shape;
        editor.input(EditorInput::Settings(profile));
    }
}
fn paint_shape(editor: &mut FaceEditor, id: &str, x: usize, y: usize, erase: bool) {
    ensure_shape(editor, id);
    let mut profile = editor.engine.settings.to_json();
    let mut rows = profile["shapes"][id]["grids"]["8x6"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![json!("........"); 6]);
    let mut row = rows[y].as_str().unwrap_or("........").as_bytes().to_vec();
    row[x] = if erase || row[x] != b'.' { b'.' } else { b'#' };
    rows[y] = json!(String::from_utf8(row).unwrap());
    profile["shapes"][id]["grids"]["8x6"] = json!(rows);
    editor.input(EditorInput::Settings(profile));
}
fn set_path<'a>(v: &mut Value, mut parts: impl Iterator<Item = &'a str>, value: Value) {
    if let Some(part) = parts.next() {
        if !v.is_object() {
            *v = json!({});
        }
        set_path(
            v.as_object_mut()
                .unwrap()
                .entry(part)
                .or_insert(Value::Null),
            parts,
            value,
        );
    } else {
        *v = value;
    }
}
fn controls(e: &FaceEditor, tab: usize) -> Vec<Control> {
    let mut out = Vec::new();
    let mut add = |label: String, path: String, kind| out.push(Control { label, path, kind });
    let choice = |names: &[&str]| Kind::Choice(names.iter().map(|s| (*s).into()).collect());
    let mut looks = FaceExpression::names()
        .iter()
        .map(|s| (*s).into())
        .collect::<Vec<String>>();
    looks.extend(e.engine.settings.custom_looks.keys().cloned());
    looks.sort();
    looks.dedup();
    if tab == 3 {
        add(
            "Edit / preview look".into(),
            "/@preview".into(),
            Kind::Choice(looks),
        );
        let name = &e.preview_look;
        let mut shapes = [
            "open", "line", "arc", "cup", "press", "cross", "spiral", "heart", "blank",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
        shapes.extend(e.engine.settings.shapes.keys().cloned());
        shapes.sort();
        shapes.dedup();
        for key in ["right_form", "left_form"] {
            add(
                key.replace('_', " "),
                format!("/looks/{name}/{key}"),
                Kind::Choice(shapes.clone()),
            );
        }
        for key in [
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
        ] {
            add(
                key.replace('_', " "),
                format!("/looks/{name}/{key}"),
                Kind::Number(0.1, -4., 4.),
            );
        }
        add(
            "Blink pace".into(),
            format!("/looks/{name}/blinks"),
            Kind::Number(0.1, 0., 8.),
        );
        return out;
    }
    if tab == 2 {
        looks.insert(0, "auto".into());
        for id in FaceConditions::reaction_names() {
            add(
                format!("{id}: enabled"),
                format!("/reactions/{id}"),
                Kind::Toggle,
            );
            add(
                format!("{id}: look"),
                format!("/trigger_looks/{id}"),
                Kind::Choice(looks.clone()),
            );
            add(
                format!("{id}: priority"),
                format!("/trigger_priorities/{id}"),
                Kind::Number(10., 0., 1990.),
            );
        }
        return out;
    }
    for (key, label) in [
        ("faces", "Facial animation"),
        ("on_other_players", "Other players"),
        ("hat_layer", "Detect hat eyes"),
        ("over_hat", "Draw over hat"),
        ("blinking", "Blinking"),
        ("glances", "Glancing"),
        ("brows", "Brows"),
        ("blush", "Blush"),
        ("glow_pulse", "Pulse glow"),
        ("glow_night_only", "Glow at night only"),
    ] {
        add(label.into(), format!("/{key}"), Kind::Toggle);
    }
    for (key, step, min, max) in [
        ("blink_seconds", 0.25, 1.5, 8.),
        ("daydream_seconds", 5., 10., 120.),
        ("expression_strength", 0.05, 0.25, 1.5),
        ("glow_strength", 0.1, 0.1, 2.),
    ] {
        add(
            key.replace('_', " "),
            format!("/{key}"),
            Kind::Number(step, min, max),
        );
    }
    add(
        "Personality".into(),
        "/personality".into(),
        choice(&[
            "classic", "brave", "fierce", "cute", "cheerful", "calm", "grumpy", "gloomy",
        ]),
    );
    let personality = &e.engine.settings.personality.id;
    add(
        "Personality resting look".into(),
        format!("/personalities/{personality}/rest"),
        Kind::Choice(looks.clone()),
    );
    add(
        "Personality resting blush".into(),
        format!("/personalities/{personality}/blush"),
        Kind::Number(0.05, 0., 1.),
    );
    add(
        "Sparkle".into(),
        "/sparkle".into(),
        choice(&["off", "bar", "dot", "skin"]),
    );
    add(
        "Tired eyes".into(),
        "/tired_eyes".into(),
        choice(&["rings", "lids", "off"]),
    );
    for key in [
        "line_color",
        "left_lid",
        "right_lid",
        "blush_color",
        "ring_color",
        "sparkle_color",
    ] {
        add(key.replace('_', " "), format!("/{key}"), Kind::Color);
        for (channel, name) in ["R", "G", "B"].iter().enumerate() {
            add(
                format!("{} {name}", key.replace('_', " ")),
                format!("/{key}"),
                Kind::ColorChannel(channel),
            );
        }
    }
    for (prefix, label) in [("", "Left eye"), ("/right_eye", "Right eye")] {
        for part in ["whites", "pupils", "brows", "sparkles"] {
            add(
                format!("{label}: glow {part}"),
                format!("{prefix}/glow/{part}"),
                Kind::Toggle,
            );
        }
        for effect in ["bloom", "glare", "trail", "tracer"] {
            add(
                format!("{label}: {effect}"),
                format!("{prefix}/glow_effects/{effect}"),
                Kind::Toggle,
            );
            add(
                format!("{label}: {effect} strength"),
                format!("{prefix}/glow_effect_strengths/{effect}"),
                Kind::Number(0.1, 0.1, 2.),
            );
        }
        for key in ["glow_color", "trail_color"] {
            add(
                format!("{label}: {}", key.replace('_', " ")),
                format!("{prefix}/{key}"),
                Kind::Color,
            );
            for (channel, name) in ["R", "G", "B"].iter().enumerate() {
                add(
                    format!("{label}: {} {name}", key.replace('_', " ")),
                    format!("{prefix}/{key}"),
                    Kind::ColorChannel(channel),
                );
            }
        }
    }
    out
}
fn save_action(game: &Game<'_>, action: EditorAction) -> Result<String, String> {
    match action {
        EditorAction::Save { settings, marks } => {
            game.native_set_setting(ID, "moves_face_profile", &settings.to_string());
            game.native_set_setting(ID, "moves_face_marks", &marks);
            // Existing quick controls must reflect the saved profile instead of overriding it.
            for (key, field) in [
                ("moves_faces_enabled", "faces"),
                ("moves_face_blinking", "blinking"),
                ("moves_face_glancing", "glances"),
                ("moves_face_brows", "brows"),
                ("moves_face_blush", "blush"),
                ("moves_face_others", "on_other_players"),
                ("moves_face_blink_seconds", "blink_seconds"),
                ("moves_face_strength", "expression_strength"),
            ] {
                game.native_set_setting(ID, key, &settings[field].to_string());
            }
            Ok("Saved facial settings and marks".into())
        }
        EditorAction::ExportSkin {
            width,
            height,
            rgba,
        } => {
            let path = export_path("png")?;
            let file = std::fs::File::create_new(&path).map_err(|e| e.to_string())?;
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())?;
            Ok(format!("Exported {}", path.display()))
        }
        EditorAction::ExportMarks(marks) => {
            let path = export_path("txt")?;
            use std::io::Write;
            let mut file = std::fs::File::create_new(&path).map_err(|e| e.to_string())?;
            file.write_all(marks.as_bytes())
                .map_err(|e| e.to_string())?;
            Ok(format!("Exported {}", path.display()))
        }
        EditorAction::Error(e) => Err(e),
        EditorAction::Close => Ok(String::new()),
    }
}
fn flush(game: &Game<'_>, s: &mut Session) {
    if let Some(editor) = &mut s.editor {
        for action in editor.take_actions() {
            s.message = match save_action(game, action) {
                Ok(message) => message,
                Err(error) => format!("Error: {error}"),
            };
        }
    }
}
fn export_path(ext: &str) -> Result<std::path::PathBuf, String> {
    let dir = horizon_client::logging::player_dir().join("punchy/exports");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    Ok(dir.join(format!("face-{time}.{ext}")))
}
pub(super) fn render(game: &Game<'_>, batch: &mut horizon_client::hud::HudBatch) {
    // Diagnostics sample inside the captured options layer, before Spatial GUI
    // restores the window cursor. This also lets the runtime driver exercise
    // the same projected pointer path as physical mouse input.
    POINTER.with(|point| point.set([game.cursor.0, game.cursor.1]));
    if !owner(game) {
        clear();
        return;
    }
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let Some(s) = ui.as_mut() else {
            return;
        };
        s.scale = (batch.width / 800.)
            .min(batch.height / 500.)
            .min(1.)
            .max(0.1);
        let width = batch.width / s.scale;
        s.height = batch.height / s.scale;
        let scale = s.scale;
        let fill = |batch: &mut horizon_client::hud::HudBatch, b: [f32; 4], c: [u8; 4]| {
            let argb = u32::from_be_bytes([c[3], c[0], c[1], c[2]]);
            game.native_fill(batch, b.map(|n| n * scale), argb);
        };
        // Scale glyphs as well as their origins so the entire modal fits even
        // at high GUI scales; hit-testing uses this same virtual canvas.
        let text = |batch: &mut horizon_client::hud::HudBatch, p: [f32; 2], t: &str, c: u32| {
            let start = batch.vertices.len();
            game.native_text(batch, t, p, c, true);
            for vertex in &mut batch.vertices[start..] {
                vertex.pos[0] *= scale;
                vertex.pos[1] *= scale;
            }
        };
        // Spatial GUI strips a single fullscreen vanilla backdrop from its
        // captured layer. Two opaque panels keep this modal's own background
        // intact, hiding the settings page beneath it on both flat and curved UI.
        fill(batch, [0., 0., width * 0.5, s.height], [15, 18, 25, 255]);
        fill(
            batch,
            [width * 0.5, 0., width * 0.5, s.height],
            [15, 18, 25, 255],
        );
        let mut preview = None;
        if let Some(editor) = &mut s.editor {
            if s.down
                && s.tab == 0
                && game.cursor.1 / scale > 92.
                && game.cursor.1 / scale < s.height - 100.
            {
                editor.input(EditorInput::Pointer {
                    x: game.cursor.0 / scale,
                    y: game.cursor.1 / scale - 30.,
                    down: true,
                    right_button: s.right,
                });
            }
            let time = game.player_model.as_ref().map_or(0., |m| {
                (m.age as f32 + game.clock.camera_fraction() as f32) / 20.
            });
            let frame = editor.frame(568., s.height - 100., time);
            for action in frame.actions {
                match save_action(game, action) {
                    Ok(m) => s.message = m,
                    Err(e) => s.message = format!("Error: {e}"),
                }
            }
            if s.tab == 0 {
                for primitive in frame.primitives {
                    match primitive {
                        EditorPrimitive::Rect { mut bounds, color } => {
                            bounds[1] += 30.;
                            fill(batch, bounds, color);
                        }
                        EditorPrimitive::Text {
                            mut position,
                            text: t,
                            color,
                        } => {
                            position[1] += 30.;
                            text(
                                batch,
                                position,
                                &t,
                                u32::from_be_bytes([0, color[0], color[1], color[2]]),
                            );
                        }
                    }
                }
                for (i, label) in [
                    "Undo",
                    "Redo",
                    "Preview +/-",
                    "Sides",
                    "Fill",
                    "Export marks",
                ]
                .iter()
                .enumerate()
                {
                    text(
                        batch,
                        [i as f32 * 100. + 8., s.height - 40.],
                        label,
                        0x9bcfff,
                    );
                }
            } else if s.tab == 4 {
                text(
                    batch,
                    [20., 64.],
                    &format!("Shape: {} (click to select)", s.shape),
                    0xe6edf7,
                );
                let profile = editor.engine.settings.to_json();
                for y in 0..6 {
                    let row = profile["shapes"][&s.shape]["grids"]["8x6"][y]
                        .as_str()
                        .unwrap_or("........")
                        .as_bytes();
                    for x in 0..8 {
                        fill(
                            batch,
                            [20. + x as f32 * 20., 110. + y as f32 * 20., 19., 19.],
                            if row.get(x).is_some_and(|&c| c != b'.') {
                                [225, 235, 255, 255]
                            } else {
                                [40, 50, 65, 255]
                            },
                        );
                    }
                }
                for (row, field) in ["closed", "on_white", "mirror"].iter().enumerate() {
                    text(
                        batch,
                        [20., 256. + row as f32 * 26.],
                        &format!(
                            "{field}: {}",
                            profile["shapes"][&s.shape][field]
                                .as_bool()
                                .unwrap_or(*field == "mirror")
                        ),
                        0xc5d6ec,
                    );
                }
                text(
                    batch,
                    [20., 360.],
                    "Click cells to draw. Right click erases. Ctrl+Z/Y undoes/redoes.",
                    0xaab8cd,
                );
                text(
                    batch,
                    [20., s.height - 40.],
                    "New shape         Use for preview look's eyes",
                    0x9bcfff,
                );
            } else {
                let list = controls(editor, s.tab);
                s.page = s.page.min(list.len().saturating_sub(1) / ROWS);
                for (row, control) in list.iter().skip(s.page * ROWS).take(ROWS).enumerate() {
                    let y = 54. + row as f32 * 26.;
                    fill(batch, [12., y, 570., 24.], [32, 40, 56, 255]);
                    text(
                        batch,
                        [20., y + 6.],
                        &format!("{}: {}", control.label, control.value(editor)),
                        0xe6edf7,
                    );
                }
                text(
                    batch,
                    [16., s.height - 40.],
                    &format!(
                        "< Previous    Page {}/{}    Next >",
                        s.page + 1,
                        list.len().div_ceil(ROWS)
                    ),
                    0x9bcfff,
                );
                text(
                    batch,
                    [16., s.height - 64.],
                    "Left click increases / right click decreases. Shift: fine adjustment.",
                    0xaab8cd,
                );
            }
            preview = Some(frame.preview);
        }
        for (i, label) in [
            "Paint",
            "Style / glow",
            "Reactions",
            "Looks",
            "Shapes",
            "Save",
            "Back",
        ]
        .iter()
        .enumerate()
        {
            fill(
                batch,
                [i as f32 * 100., 0., 98., 27.],
                if s.tab == i {
                    [50, 85, 120, 255]
                } else {
                    [32, 40, 56, 255]
                },
            );
            text(batch, [i as f32 * 100. + 8., 8.], label, 0xffffff);
        }
        if let Some(preview) = preview {
            let cell = ((width - 620.) / 8.).clamp(8., 24.);
            text(batch, [600., 52.], "Live preview", 0xffffff);
            for y in 0..8 {
                for x in 0..8 {
                    let skin_scale = preview.width as usize / 64;
                    let i = (((8 + y) * skin_scale * preview.width as usize)
                        + (8 + x) * skin_scale)
                        * 4;
                    let base: [u8; 4] = preview.rgba[i..i + 4].try_into().unwrap();
                    let hi = (((8 + y) * skin_scale * preview.width as usize)
                        + (40 + x) * skin_scale)
                        * 4;
                    let hat: [u8; 4] = preview.rgba[hi..hi + 4].try_into().unwrap();
                    let a = hat[3] as f32 / 255.;
                    let c = std::array::from_fn(|k| {
                        if k == 3 {
                            255
                        } else {
                            (base[k] as f32 * (1. - a) + hat[k] as f32 * a).round() as u8
                        }
                    });
                    fill(
                        batch,
                        [600. + x as f32 * cell, 74. + y as f32 * cell, cell, cell],
                        c,
                    );
                }
            }
        }
        text(batch, [12., s.height - 18.], &s.message, 0xc5d6ec);
    });
}
