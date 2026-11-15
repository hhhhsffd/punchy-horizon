//! Native editor presentation model. Hosts translate rectangles/text to their
//! GUI renderer and forward pointer/key events; no game types are required.
use super::{
    FaceConditions, FaceEngine, FaceExpression, FaceMarks, FaceOutput, FaceSettings, Rgba,
    export_skin_metadata,
};
use serde_json::{Value, json};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorTool {
    Erase,
    Eye,
    Pupil,
    Brow,
    Fill,
}
impl EditorTool {
    fn mark(self) -> u8 {
        match self {
            Self::Erase => 0,
            Self::Eye => 1,
            Self::Pupil => 2,
            Self::Brow => 3,
            Self::Fill => 1,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorKey {
    Escape,
    Enter,
    Undo,
    Redo,
    Erase,
    Eye,
    Pupil,
    Brow,
    Fill,
    PreviewNext,
    PreviewPrevious,
    Export,
    Save,
    Reset,
    ToggleSides,
}
#[derive(Clone, Debug)]
pub enum EditorInput {
    Key {
        key: EditorKey,
        ctrl: bool,
        shift: bool,
    },
    /// GUI coordinates. A stroke ends when down becomes false; right button
    /// erases regardless of selected tool.
    Pointer {
        x: f32,
        y: f32,
        down: bool,
        right_button: bool,
    },
    SelectTool(EditorTool),
    SetLook(String),
    ToggleMirror,
    /// Shallow merge into the persisted settings JSON; e.g.
    /// `{ "blink_seconds": 5, "personality": "brave" }`.
    Settings(Value),
    ImportMarks(String),
    Undo,
    Redo,
    AutoDetect,
    Clear,
    ExportSkin,
    ExportMarks,
    Save,
    Close,
}
#[derive(Clone, Debug)]
pub enum EditorAction {
    Close,
    /// Caller chooses the filename and persists these bytes as a PNG.
    ExportSkin {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    ExportMarks(String),
    Save {
        settings: Value,
        marks: String,
    },
    Error(String),
}
#[derive(Clone, Debug)]
pub enum EditorPrimitive {
    Rect {
        bounds: [f32; 4],
        color: Rgba,
    },
    Text {
        position: [f32; 2],
        text: String,
        color: Rgba,
    },
}
#[derive(Clone, Debug)]
pub struct EditorFrame {
    pub primitives: Vec<EditorPrimitive>,
    pub preview: FaceOutput,
    pub selected_tool: EditorTool,
    pub preview_look: String,
    pub dirty: bool,
    /// Changes and file operations are returned once, never performed silently.
    pub actions: Vec<EditorAction>,
}
#[derive(Clone)]
struct Snapshot {
    marks: FaceMarks,
    settings: Value,
}

/// Owns a preview engine. `input` edits only this editor's copy; apply the
/// `Save` action to the live player engine and native preference store.
pub struct FaceEditor {
    pub engine: FaceEngine,
    pub marks: FaceMarks,
    pub tool: EditorTool,
    pub mirror: bool,
    pub show_sides: bool,
    pub preview_look: String,
    pub dirty: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    actions: Vec<EditorAction>,
    viewport: [f32; 3],
    pointer_down: bool,
    previous_cell: Option<(i32, i32)>,
    stroke_changed: bool,
    saved: Snapshot,
}
impl FaceEditor {
    pub fn new(
        rgba: &[u8],
        width: u32,
        height: u32,
        settings: FaceSettings,
    ) -> Result<Self, String> {
        let engine = FaceEngine::new(rgba, width, height, settings)?;
        let marks = engine.rig.marks();
        let saved = Snapshot {
            marks: marks.clone(),
            settings: engine.settings.to_json(),
        };
        Ok(Self {
            engine,
            marks,
            tool: EditorTool::Eye,
            mirror: false,
            show_sides: true,
            preview_look: "neutral".into(),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            actions: Vec::new(),
            viewport: [16.0, 88.0, 12.0],
            pointer_down: false,
            previous_cell: None,
            stroke_changed: false,
            saved,
        })
    }
    pub fn from_engine(engine: &FaceEngine) -> Result<Self, String> {
        let (w, h) = engine.dimensions();
        let mut editor = Self::new(engine.original_rgba(), w, h, engine.settings.clone())?;
        editor.marks = engine.rig.marks();
        editor.engine.set_marks(Some(&editor.marks));
        editor.saved = editor.snapshot();
        Ok(editor)
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            marks: self.marks.clone(),
            settings: self.engine.settings.to_json(),
        }
    }
    fn remember(&mut self) {
        self.undo.push(self.snapshot());
        if self.undo.len() > 128 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    fn apply(&mut self, snapshot: Snapshot) {
        self.marks = snapshot.marks;
        self.engine.settings = FaceSettings::from_json(&snapshot.settings);
        self.engine.set_marks(Some(&self.marks));
        self.update_dirty();
    }
    fn update_dirty(&mut self) {
        self.dirty =
            self.marks != self.saved.marks || self.engine.settings.to_json() != self.saved.settings;
    }
    fn rebuild(&mut self) {
        self.engine.set_marks(Some(&self.marks));
        self.update_dirty();
    }
    pub fn undo(&mut self) {
        if let Some(old) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.apply(old);
        }
    }
    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.apply(next);
        }
    }
    /// Programmatic editor command; coordinates cover the continuous 24x8
    /// front/side strip. Useful for accessibility, scripted tools and native UI.
    pub fn paint(&mut self, x: i32, y: i32, kind: u8) {
        self.remember();
        self.paint_cell(x, y, kind);
        self.rebuild();
    }
    fn paint_cell(&mut self, x: i32, y: i32, kind: u8) {
        self.marks.set(x, y, kind);
        if self.mirror {
            self.marks.set(7 - x, y, kind);
        }
    }
    fn flood(&mut self, x: i32, y: i32, kind: u8) {
        if !super::detect::inside(x, y) {
            return;
        }
        let old = self.marks.get(x, y);
        if old == kind {
            return;
        }
        let mut queue = VecDeque::from([(x, y)]);
        while let Some((x, y)) = queue.pop_front() {
            if !super::detect::inside(x, y) || self.marks.get(x, y) != old {
                continue;
            }
            self.paint_cell(x, y, kind);
            for next in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                queue.push_back(next);
            }
        }
    }
    fn line(&mut self, from: (i32, i32), to: (i32, i32), kind: u8) {
        let (mut x, mut y) = from;
        let dx = (to.0 - x).abs();
        let dy = -(to.1 - y).abs();
        let sx = if x < to.0 { 1 } else { -1 };
        let sy = if y < to.1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            self.paint_cell(x, y, kind);
            if (x, y) == to {
                break;
            }
            let e = 2 * error;
            if e >= dy {
                error += dy;
                x += sx;
            }
            if e <= dx {
                error += dx;
                y += sy;
            }
        }
    }
    pub fn input(&mut self, input: EditorInput) {
        match input {
            EditorInput::SelectTool(tool) => self.tool = tool,
            EditorInput::SetLook(look) => {
                self.preview_look = look;
                self.engine.reset_animation();
            }
            EditorInput::ToggleMirror => self.mirror = !self.mirror,
            EditorInput::Settings(patch) => {
                if let Some(patch) = patch.as_object() {
                    self.remember();
                    let mut settings = self.engine.settings.to_json();
                    settings.as_object_mut().unwrap().extend(patch.clone());
                    self.engine.settings = FaceSettings::from_json(&settings);
                    self.rebuild();
                }
            }
            EditorInput::ImportMarks(text) => {
                self.remember();
                self.marks = FaceMarks::parse(&text);
                self.rebuild();
            }
            EditorInput::Undo => self.undo(),
            EditorInput::Redo => self.redo(),
            EditorInput::AutoDetect => {
                self.remember();
                self.engine.redetect();
                self.marks = self.engine.rig.marks();
                self.rebuild();
            }
            EditorInput::Clear => {
                self.remember();
                self.marks = FaceMarks::default();
                self.rebuild();
            }
            EditorInput::ExportSkin => {
                let (width, height) = self.engine.dimensions();
                let mut rgba = self.engine.original_rgba().to_vec();
                let mut metadata = self.engine.metadata();
                metadata.marks = self.marks.clone();
                match export_skin_metadata(&mut rgba, width, height, &metadata) {
                    Ok(()) => self.actions.push(EditorAction::ExportSkin {
                        width,
                        height,
                        rgba,
                    }),
                    Err(e) => self.actions.push(EditorAction::Error(e)),
                }
            }
            EditorInput::ExportMarks => self
                .actions
                .push(EditorAction::ExportMarks(self.marks.format())),
            EditorInput::Save => {
                self.actions.push(EditorAction::Save {
                    settings: self.engine.settings.to_json(),
                    marks: self.marks.format(),
                });
                self.saved = self.snapshot();
                self.dirty = false;
            }
            EditorInput::Close => self.actions.push(EditorAction::Close),
            EditorInput::Key { key, ctrl, shift } => {
                let _ = ctrl;
                match key {
                    EditorKey::Escape => self.input(EditorInput::Close),
                    EditorKey::Enter | EditorKey::Save => self.input(EditorInput::Save),
                    EditorKey::Undo => self.input(if shift {
                        EditorInput::Redo
                    } else {
                        EditorInput::Undo
                    }),
                    EditorKey::Redo => self.input(EditorInput::Redo),
                    EditorKey::Erase => self.tool = EditorTool::Erase,
                    EditorKey::Eye => self.tool = EditorTool::Eye,
                    EditorKey::Pupil => self.tool = EditorTool::Pupil,
                    EditorKey::Brow => self.tool = EditorTool::Brow,
                    EditorKey::Fill => self.tool = EditorTool::Fill,
                    EditorKey::PreviewNext | EditorKey::PreviewPrevious => {
                        let names = FaceExpression::names();
                        let current = names
                            .iter()
                            .position(|&n| n == self.preview_look)
                            .unwrap_or(0);
                        let step = if key == EditorKey::PreviewPrevious {
                            names.len() - 1
                        } else {
                            1
                        };
                        self.preview_look = names[(current + step) % names.len()].into();
                        self.engine.reset_animation();
                    }
                    EditorKey::Export => self.input(EditorInput::ExportSkin),
                    EditorKey::Reset => self.input(EditorInput::AutoDetect),
                    EditorKey::ToggleSides => self.show_sides = !self.show_sides,
                }
            }
            EditorInput::Pointer {
                x,
                y,
                down,
                right_button,
            } => {
                if !down {
                    self.pointer_down = false;
                    self.previous_cell = None;
                    if self.stroke_changed {
                        self.rebuild();
                        self.stroke_changed = false;
                    }
                    return;
                }
                // Toolbar tools, auto detect, mirror, preview, save and export
                // are ordinary clickable native controls, not keyboard-only.
                if !self.pointer_down && (34.0..62.0).contains(&y) {
                    let button = ((x - 16.0) / 64.0).floor() as i32;
                    match button {
                        0 => self.tool = EditorTool::Erase,
                        1 => self.tool = EditorTool::Eye,
                        2 => self.tool = EditorTool::Pupil,
                        3 => self.tool = EditorTool::Brow,
                        4 => self.input(EditorInput::AutoDetect),
                        5 => self.mirror = !self.mirror,
                        6 => self.input(EditorInput::Save),
                        7 => self.input(EditorInput::ExportSkin),
                        _ => {}
                    }
                    self.pointer_down = true;
                    return;
                }
                let [left, top, cell] = self.viewport;
                let col = ((x - left) / cell).floor() as i32;
                let row = ((y - top) / cell).floor() as i32;
                let (start, count) = if self.show_sides { (-8, 24) } else { (0, 8) };
                if col < 0 || col >= count || !(0..8).contains(&row) {
                    self.previous_cell = None;
                    self.pointer_down = true;
                    return;
                }
                let point = (col + start, row);
                if self.previous_cell == Some(point) {
                    self.pointer_down = true;
                    return;
                }
                if !self.pointer_down {
                    self.remember();
                }
                let kind = if right_button { 0 } else { self.tool.mark() };
                if self.tool == EditorTool::Fill && !right_button {
                    self.flood(point.0, point.1, kind);
                } else if let Some(last) = self.previous_cell {
                    self.line(last, point, kind);
                } else {
                    self.paint_cell(point.0, point.1, kind);
                }
                self.previous_cell = Some(point);
                self.pointer_down = true;
                self.stroke_changed = true;
                self.rebuild();
            }
        }
    }
    pub fn frame(&mut self, width: f32, height: f32, time_seconds: f32) -> EditorFrame {
        let panels = if self.show_sides { 24.0 } else { 8.0 };
        let cell = ((width - 32.0) / panels)
            .min((height - 170.0) / 8.0)
            .clamp(4.0, 36.0);
        self.viewport = [16.0, 88.0, cell];
        let mut primitives = vec![
            EditorPrimitive::Rect {
                bounds: [0.0, 0.0, width, height],
                color: [15, 18, 25, 245],
            },
            EditorPrimitive::Text {
                position: [16.0, 12.0],
                text: format!("Face editor{}", if self.dirty { " *" } else { "" }),
                color: [240, 240, 248, 255],
            },
        ];
        for (i, label) in [
            "Erase", "Eye", "Pupil", "Brow", "Detect", "Mirror", "Save", "Export",
        ]
        .iter()
        .enumerate()
        {
            primitives.push(EditorPrimitive::Rect {
                bounds: [16.0 + i as f32 * 64.0, 34.0, 60.0, 28.0],
                color: [40, 46, 60, 255],
            });
            primitives.push(EditorPrimitive::Text {
                position: [20.0 + i as f32 * 64.0, 42.0],
                text: (*label).into(),
                color: [235, 235, 245, 255],
            });
        }
        if self.show_sides {
            for (i, label) in ["Right side", "Front", "Left side"].iter().enumerate() {
                primitives.push(EditorPrimitive::Text {
                    position: [16.0 + i as f32 * 8.0 * cell, 72.0],
                    text: (*label).into(),
                    color: [180, 190, 215, 255],
                });
            }
        }
        let start = if self.show_sides { -8 } else { 0 };
        let count = if self.show_sides { 24 } else { 8 };
        for y in 0..8 {
            for col in 0..count {
                let x = start + col;
                let px = 16.0 + col as f32 * cell;
                let py = 88.0 + y as f32 * cell;
                let base = self.engine.grid.shown(x, y, self.engine.settings.hat_layer);
                primitives.push(EditorPrimitive::Rect {
                    bounds: [px, py, cell - 1.0, cell - 1.0],
                    color: base,
                });
                let mark = self.marks.get(x, y);
                if mark > 0 {
                    let color = match mark {
                        1 => [245, 245, 255, 230],
                        2 => [70, 135, 255, 240],
                        _ => [255, 175, 65, 240],
                    };
                    primitives.push(EditorPrimitive::Rect {
                        bounds: [px + cell * 0.15, py + cell * 0.15, cell * 0.7, cell * 0.7],
                        color,
                    });
                    primitives.push(EditorPrimitive::Text {
                        position: [px + cell * 0.3, py + cell * 0.25],
                        text: match mark {
                            1 => "E",
                            2 => "P",
                            _ => "B",
                        }
                        .into(),
                        color: [10, 10, 20, 255],
                    });
                }
            }
        }
        let footer = 88.0 + 8.0 * cell + 12.0;
        primitives.push(EditorPrimitive::Text {
            position: [16.0, footer],
            text: format!(
                "Preview: {}  |  Mirror: {}  |  Undo {} / Redo {}",
                self.preview_look,
                self.mirror,
                self.undo.len(),
                self.redo.len()
            ),
            color: [215, 220, 235, 255],
        });
        primitives.push(EditorPrimitive::Text {
            position: [16.0, footer + 20.0],
            text: "1 Erase  2 Eye  3 Pupil  4 Brow   [ ] Preview   Ctrl+Z/Y Undo/Redo".into(),
            color: [145, 155, 180, 255],
        });
        let conditions = FaceConditions {
            time_seconds,
            forced_look: Some(self.preview_look.clone()),
            ..FaceConditions::default()
        };
        let preview = self.engine.render(&conditions);
        EditorFrame {
            primitives,
            preview,
            selected_tool: self.tool,
            preview_look: self.preview_look.clone(),
            dirty: self.dirty,
            actions: std::mem::take(&mut self.actions),
        }
    }
    /// Machine-readable editor controls for native widgets (sliders, color
    /// pickers, reaction toggles, custom personality and per-eye glow panels).
    pub fn controls(&self) -> Value {
        json!({"settings":self.engine.settings.to_json(),
            "looks":FaceExpression::names(),"reactions":FaceConditions::reaction_names(),
            "tools":["erase","eye","pupil","brow","fill"],
            "ranges":{"blink_seconds":[1.5,8.0],"daydream_seconds":[10,120],
                "expression_strength":[0.25,1.5],"glow_strength":[0.1,2.0]},
            "personalities":["classic","brave","fierce","cute","cheerful","calm","grumpy","gloomy"],
            "tired_eyes":["rings","lids","off"],"sparkles":["off","bar","dot","skin"]})
    }
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    #[test]
    fn editor_strokes_have_undo_redo_and_save_actions() {
        let rgba = [190, 145, 110, 255].repeat(64 * 64);
        let mut editor = FaceEditor::new(&rgba, 64, 64, FaceSettings::default()).unwrap();
        editor.paint(1, 4, 1);
        assert_eq!(editor.marks.get(1, 4), 1);
        assert!(editor.dirty);
        editor.undo();
        assert_eq!(editor.marks.get(1, 4), 0);
        editor.redo();
        assert_eq!(editor.marks.get(1, 4), 1);
        editor.input(EditorInput::Save);
        let frame = editor.frame(800.0, 600.0, 1.0);
        assert!(matches!(
            frame.actions.first(),
            Some(EditorAction::Save { .. })
        ));
        assert!(!editor.dirty);
    }
    #[test]
    fn mirror_brush_wraps_sides() {
        let rgba = [190, 145, 110, 255].repeat(64 * 64);
        let mut editor = FaceEditor::new(&rgba, 64, 64, FaceSettings::default()).unwrap();
        editor.mirror = true;
        editor.paint(-1, 4, 2);
        assert_eq!(editor.marks.get(8, 4), 2);
    }
}
