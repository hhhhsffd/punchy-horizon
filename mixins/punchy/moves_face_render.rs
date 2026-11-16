//! RGBA skin-space rasterizer. Geometry is generated procedurally: no artwork
//! or pre-rendered expression masks are embedded.
use super::detect::{Brow, Eye, inside};
use super::motion::{FaceConditions, FaceForm};
use super::{
    FaceEngine, FaceOutput, FaceQuad, GLOW_BROWS, GLOW_PUPILS, GLOW_SPARKLES, GLOW_WHITES,
    GlowAnchor, Rgba, chroma, distance, luminance, mix_color,
};

struct Canvas<'a> {
    engine: &'a FaceEngine,
    out: &'a mut FaceOutput,
    opacity: f32,
    geometry: bool,
    depth: f32,
}
impl Canvas<'_> {
    // Keep fractional geometry separate from the compatibility skin raster.
    // Split at head seams so a quad can never bridge two perpendicular planes.
    fn piece(
        &mut self,
        bounds: [f32; 4],
        hat: bool,
        color: Rgba,
        alpha: f32,
        emission: Option<Rgba>,
    ) {
        if !self.geometry {
            return;
        }
        let [left, top, right, bottom] = bounds;
        let top = top.max(0.);
        let bottom = bottom.min(8.);
        if bottom <= top {
            return;
        }
        let mut color = color;
        color[3] = (color[3] as f32 * alpha * self.opacity)
            .clamp(0., 255.)
            .round() as u8;
        if color[3] == 0 {
            return;
        }
        for (lo, hi) in [(-8_f32, 0_f32), (0., 8.), (8., 16.)] {
            let l = left.max(lo);
            let r = right.min(hi);
            if r <= l {
                continue;
            }
            let middle = (l + r) * 0.5;
            let (a, normal) = surface_in_face(l, top, hat, middle);
            let (b, _) = surface_in_face(r, top, hat, middle);
            let (c, _) = surface_in_face(r, bottom, hat, middle);
            let (d, _) = surface_in_face(l, bottom, hat, middle);
            let positions =
                [a, d, c, b].map(|p| std::array::from_fn(|i| p[i] + normal[i] * self.depth));
            let mut emission = emission.unwrap_or([0; 4]);
            emission[3] = (emission[3] as f32 * alpha * self.opacity)
                .clamp(0., 255.)
                .round() as u8;
            self.out.quads.push(FaceQuad {
                positions,
                normal,
                color,
                emission,
            });
        }
    }
    fn original(&self, x: f32, y: f32, hat: bool) -> Rgba {
        if !inside(x.floor() as i32, y.floor() as i32) {
            return [0; 4];
        }
        let scale = self.engine.grid.scale as f32;
        let px = ((if hat { 40.0 } else { 8.0 }) + x) * scale;
        let py = (8.0 + y) * scale;
        let i = (py.floor() as usize * self.engine.width as usize + px.floor() as usize) * 4;
        self.engine.source[i..i + 4].try_into().unwrap_or([0; 4])
    }
    fn backing(&mut self, x: f32, y: f32, w: f32, h: f32, hat: bool, color: Rgba) {
        // The body skin contains only stable backings. Animated pixels belong
        // to the fractional mesh; drawing them twice leaves raster fringes.
        self.rect(x, y, w, h, hat, color, 1., None);
        let scale = self.engine.grid.scale as f32;
        for py in (y * scale).floor() as i32..((y + h) * scale).ceil() as i32 {
            for px in (x * scale).floor() as i32..((x + w) * scale).ceil() as i32 {
                if px < -8 * scale as i32
                    || px >= 16 * scale as i32
                    || py < 0
                    || py >= 8 * scale as i32
                {
                    continue;
                }
                let u = (if hat { 40 } else { 8 }) * scale as i32 + px;
                let v = 8 * scale as i32 + py;
                let i = (v as usize * self.engine.width as usize + u as usize) * 4;
                self.out.geometry_skin[i..i + 4].copy_from_slice(&color);
            }
        }
    }
    fn paint(&mut self, x: i32, y: i32, hat: bool, c: Rgba, coverage: f32, emission: Option<Rgba>) {
        let scale = self.engine.grid.scale as i32;
        if x < -8 * scale || x >= 16 * scale || y < 0 || y >= 8 * scale {
            return;
        }
        let u = (if hat { 40 } else { 8 }) * scale + x;
        let v = 8 * scale + y;
        let i = (v as usize * self.engine.width as usize + u as usize) * 4;
        let old: Rgba = self.out.rgba[i..i + 4].try_into().unwrap();
        let coverage = (coverage * self.opacity).clamp(0.0, 1.0);
        if coverage <= 0.0 {
            return;
        }
        // Replacements retain skin-layer alpha; an animated hat eye is opaque.
        let new = mix_color(old, c, coverage);
        self.out.rgba[i..i + 4].copy_from_slice(&new);
        if new != self.engine.source[i..i + 4] {
            self.out.overlay[i..i + 4].copy_from_slice(&new);
            self.out.overlay[i + 3] = 255;
        }
        if let Some(mut e) = emission {
            e[3] = (e[3] as f32 * coverage).round() as u8;
            self.out.emission[i..i + 4].copy_from_slice(&e);
        } else {
            self.out.emission[i..i + 4].fill(0);
        }
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit animation and world inputs keep the pure solver independent of the game"
    )]
    fn rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        hat: bool,
        c: Rgba,
        alpha: f32,
        emission: Option<Rgba>,
    ) {
        self.piece([x, y, x + w, y + h], hat, c, alpha, emission);
        let scale = self.engine.grid.scale as f32;
        let left = (x * scale).floor() as i32;
        let right = ((x + w) * scale).ceil() as i32;
        let top = (y * scale).floor() as i32;
        let bottom = ((y + h) * scale).ceil() as i32;
        for py in top..bottom {
            for px in left..right {
                let coverage =
                    (((px + 1) as f32).min((x + w) * scale) - (px as f32).max(x * scale)).max(0.0)
                        * (((py + 1) as f32).min((y + h) * scale) - (py as f32).max(y * scale))
                            .max(0.0);
                self.paint(px, py, hat, c, coverage * alpha, emission);
            }
        }
    }
    fn allowed_mark(&self, x: f32, y: f32) -> bool {
        let col = x.floor() as i32;
        let row = y.floor() as i32;
        if !inside(col, row) {
            return false;
        }
        if self.engine.rig.eyes.iter().any(|e| e.contains(col, row)) {
            return false;
        }
        let c = self
            .engine
            .grid
            .shown(col, row, self.engine.settings.hat_layer);
        // Decorative marks belong on exposed facial skin, not hair/accessories.
        distance(c, self.engine.rig.skin) < 85.0
            || luminance(c) >= luminance(self.engine.rig.skin) - 50.0
                && chroma(c) < chroma(self.engine.rig.skin) + 30.0
    }
    fn mark(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba, alpha: f32) {
        if self.allowed_mark(x + w * 0.5, y + h * 0.5) {
            let hat = self.engine.settings.over_hat
                || self.engine.grid.on_hat(
                    x.floor() as i32,
                    y.floor() as i32,
                    self.engine.settings.hat_layer,
                );
            self.rect(x, y, w, h, hat, c, alpha, None);
        }
    }
}

fn emissive(
    engine: &FaceEngine,
    c: &FaceConditions,
    side: usize,
    part: u8,
    base: Rgba,
) -> Option<Rgba> {
    let light = &engine.settings.lights[side.min(1)];
    if light.parts & part == 0 || engine.settings.glow_night_only && !c.night {
        return None;
    }
    let mut color = light.color.unwrap_or(base);
    let pulse = if engine.settings.glow_pulse {
        0.75 + 0.25 * (c.time_seconds * 2.0).sin()
    } else {
        1.0
    };
    let strength = engine.settings.glow_strength * pulse;
    // HDR strength remains unquantized on anchors; this is the UNORM mask.
    for channel in &mut color[..3] {
        *channel = (*channel as f32 * strength).min(255.0).round() as u8;
    }
    color[3] = 255;
    Some(color)
}

pub(super) fn draw(engine: &FaceEngine, conditions: &FaceConditions, out: &mut FaceOutput) {
    let mut canvas = Canvas {
        engine,
        out,
        opacity: super::finite(conditions.opacity, 1.0).clamp(0.0, 1.0),
        geometry: conditions.quads,
        depth: 0.,
    };
    for e in &engine.rig.eyes {
        draw_eye(&mut canvas, conditions, e);
    }
    if engine.settings.brows {
        for b in &engine.rig.brows {
            draw_brow(&mut canvas, conditions, b);
        }
    }
    draw_marks(&mut canvas);
    for e in &engine.rig.eyes {
        let light = &engine.settings.lights[e.side];
        if light.parts == 0 || engine.settings.glow_night_only && !conditions.night {
            continue;
        }
        let x = e.col as f32 + e.width as f32 * 0.5;
        let y = e.row as f32 + e.height as f32 * 0.5;
        let (position, _) = surface(x, y, e.hat || engine.settings.over_hat);
        // A shut iris must not leave bloom, trails or a light floating over its
        // opaque eyelid. Sample the final emission mask in that eye's region.
        let scale = engine.grid.scale as i32;
        let mut visible = 0_u32;
        let mut area = 0_u32;
        let rise = if light.parts & GLOW_BROWS != 0 { 3 } else { 1 };
        for row in (e.row - rise).max(0) * scale..(e.row + e.height + 1).min(8) * scale {
            for col in (e.col - 1).max(-8) * scale..(e.col + e.width + 1).min(16) * scale {
                let u = (if e.hat || engine.settings.over_hat {
                    40
                } else {
                    8
                }) * scale
                    + col;
                let v = 8 * scale + row;
                let i = (v as usize * engine.width as usize + u as usize) * 4;
                visible += u32::from(canvas.out.emission[i + 3]);
                area += 1;
            }
        }
        let coverage = (visible as f32 / (255. * area.max(1) as f32) * 4.).min(1.);
        let pulse = if engine.settings.glow_pulse {
            0.75 + 0.25 * (conditions.time_seconds * 2.0).sin()
        } else {
            1.0
        };
        canvas.out.glow_anchors.push(GlowAnchor {
            position,
            color: light.color.unwrap_or(e.iris),
            strength: engine.settings.glow_strength * pulse * coverage,
            effects: light.effects,
            effect_strengths: light.strengths,
            trail_color: light.trail_color.or(light.color).unwrap_or(e.iris),
        });
    }
}
fn surface(x: f32, y: f32, hat: bool) -> ([f32; 3], [f32; 3]) {
    surface_in_face(x, y, hat, x)
}
fn surface_in_face(x: f32, y: f32, hat: bool, face_x: f32) -> ([f32; 3], [f32; 3]) {
    let depth = if hat { 4.51 } else { 4.01 };
    let expand = if hat { 1.125 } else { 1. };
    let y = (y - 4.) * expand - 4.;
    if face_x < 0.0 {
        ([-depth, y, (-4.0 - x) * expand], [-1.0, 0.0, 0.0])
    } else if face_x >= 8.0 {
        ([depth, y, (x - 12.0) * expand], [1.0, 0.0, 0.0])
    } else {
        ([(x - 4.0) * expand, y, -depth], [0.0, 0.0, -1.0])
    }
}

fn draw_eye(canvas: &mut Canvas<'_>, c: &FaceConditions, e: &Eye) {
    canvas.depth = 0.;
    let engine = canvas.engine;
    let f = canvas.out.frame.clone();
    let s = &engine.settings;
    let side = e.side;
    let form = if eye_side(e) == 2 {
        middle_form(f.forms)
    } else {
        f.forms[side]
    };
    let eye_lid = if eye_side(e) == 2 {
        f.lids[0].max(f.lids[1])
    } else {
        f.lids[side]
    };
    let custom = s
        .shapes
        .get(f.shape_names[side].as_deref().unwrap_or(form.name()));
    let hat = e.hat || s.over_hat;
    let lid = s.lid_colors[side].unwrap_or(e.lid);
    let ink = s.line_color.unwrap_or(e.ink);
    let scale = canvas.engine.grid.scale as f32;
    let x = e.col as f32;
    let y = e.row as f32;
    let w = e.width as f32;
    let h = e.height as f32;
    let widen = if e.white.is_some() && (e.pupil_width > 0 || form == FaceForm::Blank) {
        snap_half(f.wide)
    } else {
        0.0
    };
    let top = (y - widen).max(0.0);
    let bottom = (y + h).min(8.0);
    let actual_h = bottom - top;
    let draw_shape = custom.is_some() || !matches!(form, FaceForm::Open | FaceForm::Blank);
    let shape_h = if let Some(shape) = custom {
        actual_h.max(shape.minimum_rows() as f32 * 0.5)
    } else if matches!(
        form,
        FaceForm::Heart | FaceForm::Spiral | FaceForm::Cross | FaceForm::Press
    ) {
        actual_h.max(1.5)
    } else {
        actual_h.max(1.0)
    };
    let shape_top = (y + h - shape_h).max(0.0);
    let mut box_top = top.min(shape_top);
    let mut box_bottom = bottom.max(shape_top + shape_h);
    if !draw_shape {
        box_top = top;
        box_bottom = bottom;
    }
    // Erase the old eye before rendering a moved pupil or replacement symbol.
    canvas.backing(x, box_top, w, box_bottom - box_top, hat, lid);
    let white_emission = emissive(canvas.engine, c, side, GLOW_WHITES, e.sclera);
    let pupil_emission = emissive(canvas.engine, c, side, GLOW_PUPILS, e.iris);
    let sparkle_emission = emissive(canvas.engine, c, side, GLOW_SPARKLES, s.sparkle_color);
    let pupil_width = e.pupil_width.max(0) as f32;
    let shift_x = if e.white.is_some() {
        pupil_shift(engine, e, f.pupil[0])
    } else {
        settled(f.pupil[0].clamp(-0.5, 0.5))
    };
    let shift_y = settled(if e.white.is_some() {
        f.pupil[1]
    } else {
        f.pupil[1].clamp(-0.5, 0.5)
    });
    let left_pupil = e.pupil_col as f32 + shift_x;
    let right_pupil = left_pupil + pupil_width.min(w);
    let white = e.white.is_some();
    let translating = !white && form == FaceForm::Open && custom.is_none();
    let scan_left = if translating { x.min(x + shift_x) } else { x };
    let scan_right = if translating {
        (x + w).max(x + w + shift_x)
    } else {
        x + w
    };
    let scan_top = if translating {
        box_top.min(box_top + shift_y).max(0.)
    } else {
        box_top
    };
    let scan_bottom = if translating {
        box_bottom.max(box_bottom + shift_y).min(8.)
    } else {
        box_bottom
    };
    if translating {
        canvas.backing(
            scan_left,
            scan_top,
            scan_right - scan_left,
            scan_bottom - scan_top,
            hat,
            lid,
        );
    }
    let px0 = (scan_left * scale).floor() as i32;
    let px1 = (scan_right * scale).ceil() as i32;
    let py0 = (scan_top * scale).floor() as i32;
    let py1 = (scan_bottom * scale).ceil() as i32;
    // Four sub-samples provide fractional lids even on a 64px skin. HD skins
    // retain native pupil detail rather than expanding the downsampled palette.
    for py in py0..py1 {
        for px in px0..px1 {
            let mut sums = [0.0_f32; 4];
            let mut emitted = [0.0_f32; 4];
            let mut coverage = 0.0;
            for oy in [0.25, 0.75] {
                for ox in [0.25, 0.75] {
                    let fx = (px as f32 + ox) / scale;
                    let fy = (py as f32 + oy) / scale;
                    if fx < scan_left || fx >= scan_right || fy < scan_top || fy >= scan_bottom {
                        continue;
                    }
                    let u = (fx - x) / w;
                    let v = (fy - shape_top) / shape_h;
                    let mut color = lid;
                    let mut em = None;
                    let mut depth = 0.020;
                    if draw_shape {
                        if e.white.is_some()
                            && (custom.is_some_and(|shape| shape.on_white)
                                || custom.is_none() && form == FaceForm::Spiral)
                        {
                            color = e.sclera;
                            em = white_emission;
                        }
                        let custom_color = custom.and_then(|shape| {
                            shape.sample(
                                u,
                                v,
                                side,
                                (w * 2.).round().max(1.) as usize,
                                (shape_h * 2.).round().max(1.) as usize,
                            )
                        });
                        if custom_color.is_some()
                            || custom.is_none()
                                && shape_on(form, u, v, eye_side(e), f.age_ticks, w, shape_h)
                        {
                            color = if form == FaceForm::Heart {
                                let pulse = (f.age_ticks / 10.0).floor() as i32 % 2 == 0;
                                if pulse {
                                    [255, 63, 110, 255]
                                } else {
                                    [255, 111, 146, 255]
                                }
                            } else {
                                ink
                            };
                            if let Some(chosen) = custom_color {
                                color = if chosen[3] == 0 { ink } else { chosen };
                            }
                            let closed = custom.map_or_else(|| form.closed(), |shape| shape.closed);
                            em = if !closed
                                && (custom_color.is_some_and(|c| c[3] > 0)
                                    || custom.is_none() && form == FaceForm::Heart)
                            {
                                pupil_emission
                            } else {
                                None
                            };
                            depth = 0.023;
                        }
                        if !custom.map_or_else(|| form.closed(), |shape| shape.closed)
                            && fy < shape_top + eye_lid * shape_h
                        {
                            color = lid;
                            em = None;
                            depth = 0.026;
                        }
                    } else {
                        let (upper_mask, lower_mask) = eye_masks(
                            e,
                            fx - x - if translating { shift_x } else { 0. },
                            actual_h,
                            eye_lid,
                            if s.brows { f.slant } else { 0.0 },
                            f.lower,
                            f.smile,
                        );
                        let upper = top + upper_mask + if translating { shift_y } else { 0. };
                        let lower = bottom - lower_mask + if translating { shift_y } else { 0. };
                        if fy >= upper
                            && fy < lower
                            && (!translating || fx >= x + shift_x && fx < x + w + shift_x)
                        {
                            if form == FaceForm::Blank {
                                color = e.sclera;
                                em = white_emission;
                                depth = 0.010;
                            } else if white {
                                color = e.sclera;
                                em = white_emission;
                                depth = 0.010;
                                if fx >= left_pupil
                                    && fx < right_pupil
                                    && fy >= y + shift_y
                                    && fy < y + h + shift_y
                                {
                                    let sx = (fx - shift_x).clamp(x, x + w - 0.001);
                                    let sy = (fy - shift_y).clamp(y, y + h - 0.001);
                                    let source = canvas.original(sx, sy, e.hat);
                                    color = if distance(source, e.sclera) > 24.0 {
                                        source
                                    } else {
                                        e.iris
                                    };
                                    color[3] = 255;
                                    em = pupil_emission;
                                    depth = 0.012;
                                }
                            } else {
                                let sx = (fx - shift_x).clamp(x, x + w - 0.001);
                                let sy = (fy - shift_y).clamp(y, y + h - 0.001);
                                color = canvas.original(sx, sy, e.hat);
                                color[3] = 255;
                                em = pupil_emission;
                                depth = 0.012;
                            }
                            // Retain skin-authored glints in the iris, or add selected
                            // procedural sparkle on the upper outer pupil corner.
                            let spark = s.sparkle != "off" || f.sparkle > 0.1;
                            let spark_size = if s.sparkle == "bar" { 0.45 } else { 0.25 };
                            let spark_x = if side == 0 {
                                left_pupil + 0.15
                            } else {
                                right_pupil - 0.15 - spark_size
                            };
                            if spark
                                && s.sparkle != "skin"
                                && fx >= spark_x
                                && fx < spark_x + spark_size
                                && fy >= top + 0.12
                                && fy < top + 0.12 + 0.25
                            {
                                color = s.sparkle_color;
                                em = sparkle_emission;
                                depth = 0.013;
                            }
                            if s.sparkle == "skin" {
                                let glint = canvas.original(fx - shift_x, fy - shift_y, true);
                                if glint[3] >= 128 && luminance(glint) > luminance(color) + 48.0 {
                                    color = glint;
                                    em = sparkle_emission;
                                    depth = 0.013;
                                }
                            }
                        }
                    }
                    canvas.depth = depth;
                    let d = 0.25 / scale;
                    canvas.piece(
                        [
                            (fx - d).max(scan_left),
                            (fy - d).max(scan_top),
                            (fx + d).min(scan_right),
                            (fy + d).min(scan_bottom),
                        ],
                        hat,
                        color,
                        1.,
                        em,
                    );
                    for k in 0..4 {
                        sums[k] += color[k] as f32;
                    }
                    if let Some(em) = em {
                        for k in 0..4 {
                            emitted[k] += em[k] as f32;
                        }
                    }
                    coverage += 1.0;
                }
            }
            if coverage > 0.0 {
                let color = std::array::from_fn(|k| (sums[k] / coverage).round() as u8);
                let emission = if emitted[3] > 0.0 {
                    Some(std::array::from_fn(|k| {
                        (emitted[k] / coverage).round() as u8
                    }))
                } else {
                    None
                };
                canvas.paint(px, py, hat, color, coverage / 4.0, emission);
            }
        }
    }
    // Hat glints follow their iris, while opaque hair/accessories keep their
    // original occlusion. Remove only the detected glint from the static atlas.
    if !e.hat && s.hat_layer {
        for row in e.row..e.row + e.height {
            for col in e.col..e.col + e.width {
                let hat_pixel = canvas
                    .engine
                    .grid
                    .at((40 + col) as usize, (8 + row) as usize);
                let base = canvas
                    .engine
                    .grid
                    .at((8 + col) as usize, (8 + row) as usize);
                if hat_pixel[3] >= 128
                    && luminance(hat_pixel) >= 140.0
                    && luminance(hat_pixel) - luminance(base) >= 60.0
                {
                    let scale = engine.grid.scale;
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let i = (((8 + row as usize) * scale + dy) * engine.width as usize
                                + (40 + col) as usize * scale
                                + dx)
                                * 4;
                            canvas.out.geometry_skin[i..i + 4].fill(0);
                            canvas.out.rgba[i..i + 4].fill(0);
                        }
                    }
                    if !draw_shape && form != FaceForm::Blank {
                        let gx = col as f32 + shift_x;
                        let gy = row as f32 + shift_y;
                        let (upper, lower) = eye_masks(
                            e,
                            gx - x,
                            actual_h,
                            eye_lid,
                            if s.brows { f.slant } else { 0. },
                            f.lower,
                            f.smile,
                        );
                        let left = gx.max(if translating { x + shift_x } else { x });
                        let right =
                            (gx + 1.).min(if translating { x + w + shift_x } else { x + w });
                        let upper = gy.max(top + upper + if translating { shift_y } else { 0. });
                        let lower =
                            (gy + 1.).min(bottom - lower + if translating { shift_y } else { 0. });
                        if right > left && lower > upper {
                            canvas.depth = 0.013;
                            canvas.rect(
                                left,
                                upper,
                                right - left,
                                lower - upper,
                                true,
                                hat_pixel,
                                1.,
                                sparkle_emission,
                            );
                        }
                    }
                }
            }
        }
    }
}
fn middle_form(forms: [FaceForm; 2]) -> FaceForm {
    match forms {
        [a, b] if a == b => a,
        [FaceForm::Open, FaceForm::Line] | [FaceForm::Line, FaceForm::Open] => FaceForm::Open,
        [FaceForm::Open, b] => b,
        [a, _] => a,
    }
}
fn settled(value: f32) -> f32 {
    if value.abs() < 0.15 { 0.0 } else { value }
}
fn snap_half(value: f32) -> f32 {
    // Match half-texel model geometry, including Java's negative tie rounding.
    (value * 2.0 + 0.5).floor() * 0.5
}
fn eye_side(e: &Eye) -> usize {
    if e.col <= 3 && e.col + e.width > 4 {
        2
    } else {
        e.side
    }
}
fn eye_pupil_shift(e: &Eye, value: f32) -> f32 {
    let side = eye_side(e);
    let left = (e.pupil_col - e.col) as f32;
    let right = (e.col + e.width - e.pupil_col - e.pupil_width) as f32;
    value.clamp(
        -(left + if side != 1 { 0.3 } else { 0.0 }),
        right + if side != 0 { 0.3 } else { 0.0 },
    )
}
fn pupil_shift(engine: &FaceEngine, e: &Eye, value: f32) -> f32 {
    let mut shift = eye_pupil_shift(e, value);
    let side = eye_side(e);
    if ((side == 0 && shift > 0.0) || (side == 1 && shift < 0.0))
        && let Some(other) = engine
            .rig
            .eyes
            .iter()
            .find(|other| eye_side(other) == 1 - side)
    {
        shift = shift.signum() * shift.abs().min(eye_pupil_shift(other, value).abs());
    }
    settled(shift)
}
fn eye_masks(
    e: &Eye,
    x: f32,
    height: f32,
    lid: f32,
    slant: f32,
    lower: f32,
    smile: f32,
) -> (f32, f32) {
    let columns = e.width * 2;
    let column = (x * 2.0).floor() as i32;
    let stair = |column: i32, count: i32| {
        let half = (count / 2).max(1);
        ((column - (half - 1)) as f32 / half as f32).max(0.0)
    };
    let (column, count) = if eye_side(e) == 2 {
        (column.min(columns - 1 - column), (columns + 1) / 2)
    } else {
        (
            if e.side == 0 {
                column
            } else {
                columns - 1 - column
            },
            columns,
        )
    };
    let slope = slant.abs()
        * stair(
            if slant >= 0.0 {
                column
            } else {
                count - 1 - column
            },
            count,
        );
    let upper_raw = height.min(lid * height + slope * 0.5 * e.height as f32);
    let lower_raw = (lower * e.height as f32 + smile * 0.5).max(0.0);
    let mut upper = snap_half(upper_raw).min(height);
    let mut lower = snap_half(lower_raw).clamp(0.0, (height - upper).max(0.0));
    if upper + lower > height - 0.01 && upper_raw + lower_raw < height - 0.01 {
        if lower >= 0.5 {
            lower -= 0.5;
        } else {
            upper = (upper - 0.5).max(0.0);
        }
    }
    (upper, lower)
}
fn shape_on(
    form: FaceForm,
    u: f32,
    v: f32,
    side: usize,
    age: f32,
    width: f32,
    height: f32,
) -> bool {
    if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
        return false;
    }
    // Symbols are cells on the half-texel grid. Analytical smooth curves
    // disappear on short eyes and have different silhouette/phase semantics.
    let cols = (width * 2.).round().max(1.) as i32;
    let rows = (height * 2.).round().max(1.) as i32;
    let x = (u * cols as f32).floor() as i32;
    let x = if side == 1 { cols - 1 - x } else { x };
    let y = (v * rows as f32).floor() as i32;
    match form {
        FaceForm::Line => y == rows / 2,
        FaceForm::Arc | FaceForm::Cup => {
            if cols < 4 {
                return y
                    == if form == FaceForm::Cup {
                        rows - 1
                    } else {
                        (rows / 2 - 1).max(0)
                    };
            }
            let levels = if cols >= 6 && rows >= 3 { 3 } else { 2 };
            let row = y - (rows - levels) / 2;
            let row = if form == FaceForm::Cup {
                levels - 1 - row
            } else {
                row
            };
            if row < 0 || row >= levels {
                return false;
            }
            let inset = levels - 1 - row;
            if row == 0 {
                x >= inset && x < cols - inset
            } else {
                x == inset || x == cols - 1 - inset
            }
        }
        FaceForm::Press => {
            let row = y - (rows - 3) / 2;
            if !(0..3).contains(&row) {
                return false;
            }
            let (x, cols) = if side == 2 {
                (x.min(cols - 1 - x), (cols / 2).max(1))
            } else {
                (x, cols)
            };
            let half = (cols / 2).max(1);
            if row == 1 { x >= cols - half } else { x < half }
        }
        FaceForm::Cross => {
            let Some((w, h)) = [(8, 8), (6, 6), (4, 4), (4, 3)]
                .into_iter()
                .find(|&(w, h)| w <= cols && h <= rows)
            else {
                return y == rows / 2;
            };
            let x = x - (cols - w) / 2;
            let y = y - (rows - h) / 2;
            if x < 0 || x >= w || y < 0 || y >= h {
                return false;
            }
            let inset = y.min(h - 1 - y);
            let thickness = if w == 8 { 2 } else { 1 };
            (inset..inset + thickness).contains(&x)
                || (w - inset - thickness..w - inset).contains(&x)
        }
        FaceForm::Heart => {
            let Some((w, h)) = [
                (8, 6),
                (8, 5),
                (8, 4),
                (6, 5),
                (6, 4),
                (6, 3),
                (4, 4),
                (4, 3),
            ]
            .into_iter()
            .find(|&(w, h)| w <= cols && h <= rows) else {
                return y >= (rows - 2).max(0);
            };
            let x = x - (cols - w) / 2;
            let y = y - (rows - h) / 2;
            if x < 0 || x >= w || y < 0 || y >= h {
                return false;
            }
            if y == 0 {
                return match w {
                    8 => (1..3).contains(&x) || (5..7).contains(&x),
                    6 => !(2..4).contains(&x),
                    _ => x == 0 || x == 3,
                };
            }
            let tail = if w == 8 && h == 6 {
                3
            } else if w >= 6 {
                2
            } else {
                1
            };
            let inset = if y < h - tail { 0 } else { w / 2 - (h - y) };
            x >= inset && x < w - inset
        }
        FaceForm::Spiral => {
            let count = if cols == 1 {
                rows
            } else if rows == 1 {
                cols
            } else {
                2 * (cols + rows) - 4
            };
            let start = (age / 1.5).floor() as i32;
            let index = if rows == 1 {
                Some(x)
            } else if cols == 1 {
                Some(y)
            } else if y == 0 {
                Some(x)
            } else if x == cols - 1 {
                Some(cols + y - 1)
            } else if y == rows - 1 {
                Some(2 * cols + rows - 3 - x)
            } else if x == 0 {
                Some(count - y)
            } else {
                None
            };
            if let Some(index) = index
                && (index - start).rem_euclid(count) >= (count / 3).max(1)
            {
                return true;
            }
            if cols >= 4 && rows >= 4 {
                let index = (start - 1).rem_euclid(count);
                let p = if index < cols {
                    (index, 0)
                } else if index < cols + rows - 1 {
                    (cols - 1, index - cols + 1)
                } else if index < 2 * cols + rows - 2 {
                    (2 * cols + rows - 3 - index, rows - 1)
                } else {
                    (0, count - index)
                };
                (x, y) == (p.0.clamp(1, cols - 2), p.1.clamp(1, rows - 2))
            } else {
                false
            }
        }
        _ => false,
    }
}
fn draw_brow(canvas: &mut Canvas<'_>, c: &FaceConditions, b: &Brow) {
    canvas.depth = 0.;
    let engine = canvas.engine;
    let f = canvas.out.frame.clone();
    let s = &engine.settings;
    let hat = b.hat || s.over_hat;
    let ink = s.line_color.unwrap_or(b.color);
    canvas.backing(b.col as f32, b.row as f32, b.width as f32, 1.0, hat, b.skin);
    let emission = emissive(canvas.engine, c, b.side, GLOW_BROWS, ink);
    canvas.depth = 0.019;
    let rise = f.brows[b.side] * 0.8;
    for i in 0..b.width {
        let t = if b.width > 1 {
            i as f32 / (b.width - 1) as f32 - 0.5
        } else {
            0.0
        };
        let sign = if b.side == 0 { 1.0 } else { -1.0 };
        let y = b.row as f32 - rise + t * f.tilt * sign * 0.8;
        canvas.rect((b.col + i) as f32, y, 1.0, 0.75, hat, ink, 1.0, emission);
    }
}
fn draw_marks(canvas: &mut Canvas<'_>) {
    canvas.depth = 0.006;
    let f = canvas.out.frame.clone();
    let engine = canvas.engine;
    let settings = &engine.settings;
    let blush_color = settings.blush_color.unwrap_or([255, 79, 126, 255]);
    let ring_color = settings.ring_color.unwrap_or([74, 56, 102, 255]);
    let eyes = canvas.engine.rig.eyes.clone();
    for e in &eyes {
        let y = (e.row + e.height) as f32;
        let center = e.col as f32 + e.width as f32 * 0.5;
        if f.blush > 0.0 {
            let tint = mix_color(e.cheek, blush_color, 0.55);
            canvas.mark(center - 0.8, y + 0.4, 1.6, 0.65, tint, f.blush * 0.75);
        }
        if f.bags > 0.0 {
            let tint = mix_color(e.cheek, ring_color, 0.8);
            canvas.mark(
                e.col as f32,
                y + 0.15,
                e.width as f32,
                0.45,
                tint,
                f.bags * 0.65,
            );
        }
        if f.tears > 0.0 {
            let outer = if e.side == 0 {
                e.col as f32 + 0.05
            } else {
                (e.col + e.width) as f32 - 0.4
            };
            let phase = (f.age_ticks / 12.0 + e.side as f32 * 0.35).fract();
            let length = (1.0 + phase * 2.0).min(8.0 - y);
            if length > 0.0 {
                canvas.mark(
                    outer,
                    y - 0.1,
                    0.42,
                    length,
                    [111, 196, 255, 255],
                    f.tears * 0.8,
                );
                canvas.mark(
                    outer,
                    y + length - 0.4,
                    0.2,
                    0.3,
                    [202, 238, 255, 255],
                    f.tears,
                );
            }
        }
        if f.sweat > 0.0 {
            let outer = if e.side == 0 {
                e.col as f32 - 0.8
            } else {
                (e.col + e.width) as f32 + 0.3
            };
            let phase = (f.age_ticks / 24.0 + e.side as f32 * 0.5).fract();
            canvas.mark(
                outer,
                (e.row as f32 - 0.4 + phase).max(0.0),
                0.45,
                0.8,
                [111, 196, 255, 255],
                f.sweat * (1.0 - phase) * 0.9,
            );
        }
        if f.anger > 0.0 && e.side == 1 {
            let x = (e.col + e.width) as f32;
            let y = (e.row as f32 - 1.2).max(0.0);
            let color = [229, 49, 58, 255];
            canvas.mark(x, y, 0.8, 0.2, color, f.anger);
            canvas.mark(x + 0.3, y - 0.3, 0.2, 0.8, color, f.anger);
            canvas.mark(x + 0.7, y + 0.5, 0.8, 0.2, color, f.anger);
            canvas.mark(x + 1.0, y + 0.2, 0.2, 0.8, color, f.anger);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::super::{FaceMarks, FaceSettings};
    use super::*;
    fn marked_engine() -> FaceEngine {
        let mut rgba = [180, 130, 95, 255].repeat(64 * 64);
        for y in 0..64 {
            for x in 32..64 {
                rgba[(y * 64 + x) * 4 + 3] = 0;
            }
        }
        for x in [9, 14] {
            let i = (12 * 64 + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[245, 245, 245, 255]);
        }
        for x in [10, 13] {
            let i = (12 * 64 + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[40, 70, 100, 255]);
        }
        let mut e = FaceEngine::new(&rgba, 64, 64, FaceSettings::default()).unwrap();
        let mut marks = FaceMarks::default();
        marks.set(1, 4, 1);
        marks.set(2, 4, 2);
        marks.set(5, 4, 2);
        marks.set(6, 4, 1);
        e.set_marks(Some(&marks));
        e
    }
    #[test]
    fn shut_expression_modifies_only_head_texture_and_emission_is_separate() {
        let mut e = marked_engine();
        e.settings.blinking = false;
        e.settings.lights[0].parts = GLOW_PUPILS;
        let mut c = FaceConditions::default();
        c.forced_look = Some("shut".into());
        e.render(&c);
        c.time_seconds = 0.2;
        let output = e.render(&c);
        assert!(output.detected);
        assert_ne!(output.rgba, e.original_rgba());
        for y in 16..64 {
            assert_eq!(
                &output.rgba[y * 64 * 4..(y + 1) * 64 * 4],
                &e.original_rgba()[y * 64 * 4..(y + 1) * 64 * 4]
            );
        }
    }
    #[test]
    fn every_procedural_shape_is_nonempty_and_finite() {
        for form in [
            FaceForm::Line,
            FaceForm::Arc,
            FaceForm::Cup,
            FaceForm::Press,
            FaceForm::Cross,
            FaceForm::Spiral,
            FaceForm::Heart,
        ] {
            assert!((0..256).any(|i| shape_on(
                form,
                (i % 16) as f32 / 16.0 + 0.03125,
                (i / 16) as f32 / 16.0 + 0.03125,
                0,
                0.0,
                4.0,
                3.0
            )));
        }
    }
    #[test]
    fn side_surface_normals_and_seams_are_consistent() {
        let (a, n) = surface(-1.0, 4.0, false);
        assert_eq!(n, [-1.0, 0.0, 0.0]);
        assert!(a[0] < -4.0);
        let (a, n) = surface(9.0, 4.0, true);
        assert_eq!(n, [1.0, 0.0, 0.0]);
        assert!(a[0] > 4.5);
    }
    #[test]
    fn blink_closes_the_entire_eye_without_an_invented_ink_line() {
        let engine = marked_engine();
        for eye in &engine.rig.eyes {
            for x in [0.25, 0.75] {
                let (upper, lower) = eye_masks(eye, x, eye.height as f32, 1.0, 0.0, 0.0, 0.0);
                assert_eq!(upper, eye.height as f32);
                assert_eq!(lower, 0.0);
            }
        }
        assert_eq!(snap_half(0.24), 0.0);
        assert_eq!(snap_half(0.25), 0.5);
        assert_eq!(settled(0.14), 0.0);
        assert_eq!(settled(0.15), 0.15);
    }
    #[test]
    fn eyes_without_whites_can_move_outside_their_original_box() {
        let mut engine = marked_engine();
        engine.settings.brows = false;
        engine.settings.sparkle = "off".into();
        engine.rig.eyes.truncate(1);
        engine.rig.eyes[0].white = None;
        let eye = &engine.rig.eyes[0];
        let x = (8 + eye.col + eye.width) as usize;
        let y = (8 + eye.row) as usize;
        let i = (y * 64 + x) * 4;
        let original = engine.original_rgba()[i..i + 4].to_vec();
        let conditions = FaceConditions::default();
        let mut output = engine.render(&conditions);
        output.rgba = engine.original_rgba().to_vec();
        output.overlay.fill(0);
        output.frame = super::super::FaceFrame {
            pupil: [0.5, 0.],
            ..Default::default()
        };
        draw(&engine, &conditions, &mut output);
        assert_ne!(&output.rgba[i..i + 4], original.as_slice());
    }
    #[test]
    fn face_mesh_has_outward_winding_and_keeps_animation_out_of_backing_skin() {
        let mut engine = marked_engine();
        let output = engine.render(&FaceConditions {
            quads: true,
            ..Default::default()
        });
        assert!(!output.quads.is_empty());
        for q in &output.quads {
            let p = q.positions.map(glam::Vec3::from_array);
            assert!(
                (p[1] - p[0])
                    .cross(p[2] - p[0])
                    .dot(glam::Vec3::from_array(q.normal))
                    > 0.
            );
        }
        assert_ne!(output.geometry_skin, output.rgba);
        assert_eq!(
            &output.geometry_skin[16 * 64 * 4..],
            &engine.original_rgba()[16 * 64 * 4..]
        );
    }
    #[test]
    fn closing_a_colored_shape_removes_pupil_glow_but_keeps_a_fading_anchor() {
        let mut engine = marked_engine();
        engine.settings.lights[0].parts = GLOW_PUPILS;
        engine.rig.eyes.truncate(1);
        let conditions = FaceConditions {
            quads: true,
            ..Default::default()
        };
        let mut output = engine.render(&conditions);
        output.quads.clear();
        output.emission.fill(0);
        output.glow_anchors.clear();
        output.frame.forms = [FaceForm::Heart; 2];
        output.frame.lids = [1.; 2];
        draw(&engine, &conditions, &mut output);
        assert!(output.quads.iter().all(|q| q.emission[3] == 0));
        assert_eq!(output.glow_anchors.len(), 1);
        assert_eq!(output.glow_anchors[0].strength, 0.);
    }
}
