//! Skin reduction, connected-cell facial rig extraction and palette sampling.
use super::{FaceMarks, Rgba, chroma, distance, luminance, over};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct SkinGrid {
    pub pixels: Vec<Rgba>,
    pub height: usize,
    pub scale: usize,
}
impl SkinGrid {
    pub fn new(rgba: &[u8], width: u32, height: u32) -> Result<Self, String> {
        if !(64..=1024).contains(&width)
            || !width.is_multiple_of(64)
            || (height != width && height != width / 2)
        {
            return Err("skin must be 64..1024 pixels wide, a multiple of 64, and square or legacy half-height".into());
        }
        let size = width as usize * height as usize * 4;
        if rgba.len() < size {
            return Err("skin RGBA buffer is truncated".into());
        }
        let scale = width as usize / 64;
        let logical_height = height as usize / scale;
        let mut pixels = Vec::with_capacity(64 * logical_height);
        let span = if scale <= 2 { scale } else { scale / 2 };
        let inset = (scale - span) / 2;
        for y in 0..logical_height {
            for x in 0..64 {
                let mut samples = Vec::with_capacity(span * span);
                for dy in 0..span {
                    for dx in 0..span {
                        let i =
                            ((y * scale + inset + dy) * width as usize + x * scale + inset + dx)
                                * 4;
                        samples.push([rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]);
                    }
                }
                pixels.push(representative(&samples));
            }
        }
        Ok(Self {
            pixels,
            height: logical_height,
            scale,
        })
    }
    pub fn at(&self, x: usize, y: usize) -> Rgba {
        self.pixels.get(y * 64 + x).copied().unwrap_or([0; 4])
    }
    /// Strip coordinates: right side -8..0, front 0..8, left side 8..16.
    pub fn shown(&self, x: i32, y: i32, hat: bool) -> Rgba {
        if !inside(x, y) {
            return [0; 4];
        }
        let mut base = self.at((8 + x) as usize, (8 + y) as usize);
        base[3] = 255;
        if hat {
            over(self.at((40 + x) as usize, (8 + y) as usize), base)
        } else {
            base
        }
    }
    pub fn on_hat(&self, x: i32, y: i32, hat: bool) -> bool {
        hat && inside(x, y) && self.at((40 + x) as usize, (8 + y) as usize)[3] >= 128
    }
}
fn representative(samples: &[Rgba]) -> Rgba {
    let mut best = samples[0];
    let mut best_count = 0;
    for &c in samples {
        let n = samples.iter().filter(|&&a| a == c).count();
        if n > best_count {
            best = c;
            best_count = n;
        }
    }
    if best_count * 2 >= samples.len() {
        return best;
    }
    let solid = samples.iter().filter(|c| c[3] >= 128).count() * 2 >= samples.len();
    let mut clusters: Vec<(Rgba, [u32; 4], u32)> = Vec::new();
    for &c in samples.iter().filter(|c| (c[3] >= 128) == solid) {
        let i = clusters
            .iter()
            .position(|(center, _, _)| distance(*center, c) <= 24.0)
            .unwrap_or_else(|| {
                clusters.push((c, [0; 4], 0));
                clusters.len() - 1
            });
        for (sum, component) in clusters[i].1.iter_mut().zip(c) {
            *sum += u32::from(component);
        }
        clusters[i].2 += 1;
    }
    if let Some((_, sum, count)) = clusters.iter().max_by_key(|(_, _, count)| *count) {
        for k in 0..4 {
            best[k] = ((sum[k] + count / 2) / count) as u8;
        }
    }
    best
}

#[derive(Clone, Debug)]
pub struct Eye {
    pub col: i32,
    pub row: i32,
    pub width: i32,
    pub height: i32,
    pub hat: bool,
    pub pupil_col: i32,
    pub pupil_width: i32,
    pub white: Option<[i32; 2]>,
    pub lid: Rgba,
    pub ink: Rgba,
    pub iris: Rgba,
    pub sclera: Rgba,
    pub cheek: Rgba,
    /// Avatar right = 0; avatar left = 1. Cyclopean eyes use 0.
    pub side: usize,
}
impl Eye {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.col && x < self.col + self.width && y >= self.row && y < self.row + self.height
    }
}
#[derive(Clone, Debug)]
pub struct Brow {
    pub col: i32,
    pub row: i32,
    pub width: i32,
    pub hat: bool,
    pub skin: Rgba,
    pub color: Rgba,
    pub side: usize,
}
#[derive(Clone, Debug)]
pub struct FaceRig {
    pub eyes: Vec<Eye>,
    pub brows: Vec<Brow>,
    pub skin: Rgba,
    pub confidence: f32,
}
impl Default for FaceRig {
    fn default() -> Self {
        Self {
            eyes: Vec::new(),
            brows: Vec::new(),
            skin: [198, 155, 125, 255],
            confidence: 0.0,
        }
    }
}
impl FaceRig {
    pub fn marks(&self) -> FaceMarks {
        let mut marks = FaceMarks::default();
        for e in &self.eyes {
            for y in e.row..e.row + e.height {
                for x in e.col..e.col + e.width {
                    marks.set(
                        x,
                        y,
                        if x >= e.pupil_col && x < e.pupil_col + e.pupil_width {
                            2
                        } else {
                            1
                        },
                    );
                }
            }
        }
        for b in &self.brows {
            for x in b.col..b.col + b.width {
                marks.set(x, b.row, 3);
            }
        }
        marks
    }
}

pub(super) fn inside(x: i32, y: i32) -> bool {
    (-8..16).contains(&x) && (0..8).contains(&y)
}
pub(super) fn index(x: i32, y: i32) -> usize {
    (y * 24 + x + 8) as usize
}
fn coords(i: usize) -> (i32, i32) {
    (i as i32 % 24 - 8, i as i32 / 24)
}
fn neighbors(i: usize) -> impl Iterator<Item = usize> {
    let (x, y) = coords(i);
    [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
        .into_iter()
        .filter(|&(x, y)| inside(x, y))
        .map(|(x, y)| index(x, y))
}
fn hue(c: Rgba) -> f32 {
    let r = c[0] as f32;
    let g = c[1] as f32;
    let b = c[2] as f32;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max == min {
        return 0.0;
    }
    let h = if max == r {
        (g - b) / (max - min)
    } else if max == g {
        (b - r) / (max - min) + 2.0
    } else {
        (r - g) / (max - min) + 4.0
    };
    (h * 60.0).rem_euclid(360.0)
}
fn hue_delta(a: Rgba, b: Rgba) -> f32 {
    let d = (hue(a) - hue(b)).abs();
    d.min(360.0 - d)
}
fn skin_tone(seen: &[Rgba; 192]) -> Rgba {
    let mut best = seen[index(3, 5)];
    let mut score = -1.0;
    // Central/lower face has stronger support than hair, ears and side panels.
    for y in 1..8 {
        for x in 0..8 {
            let c = seen[index(x, y)];
            let mut support = 0.0;
            for sy in 1..8 {
                for sx in 0..8 {
                    let d = distance(c, seen[index(sx, sy)]);
                    if d < 40.0 {
                        let center = 1.0 + (3.5 - (sx as f32 - 3.5).abs()) * 0.6;
                        let lower = if sy >= 3 { 2.0 } else { 0.7 };
                        support += (40.0 - d) * center * lower;
                    }
                }
            }
            if luminance(c) < 32.0 {
                support *= 0.6;
            }
            if chroma(c) >= 20.0 {
                support *= 1.0 + luminance(c) / 512.0;
            }
            if support > score {
                best = c;
                score = support;
            }
        }
    }
    let mut sum = [0.0; 3];
    let mut total = 0.0;
    for y in 1..8 {
        for x in 0..8 {
            let c = seen[index(x, y)];
            let w = (36.0 - distance(c, best)).max(0.0);
            for k in 0..3 {
                sum[k] += c[k] as f32 * w;
            }
            total += w;
        }
    }
    if total > 0.0 {
        for k in 0..3 {
            best[k] = (sum[k] / total).round() as u8;
        }
    }
    best
}
fn skin_like(c: Rgba, tone: Rgba) -> bool {
    distance(c, tone) < 46.0
        && (chroma(c) < 25.0 || chroma(tone) < 12.0 || hue_delta(c, tone) < 60.0)
}
fn white_like(c: Rgba, tone: Rgba) -> bool {
    let l = luminance(c);
    let t = luminance(tone);
    let ch = chroma(c);
    let pale = (ch <= 40.0 && l >= 100.0_f32.max(t - 45.0))
        || (ch <= 16.0 && l >= 100.0_f32.max(t - 80.0))
        || (ch <= 64.0 && l >= 185.0 && (hue_delta(c, tone) >= 40.0 || ch * 2.0 <= chroma(tone)))
        || (ch <= 70.0 && l >= t + 50.0 && l >= 150.0);
    pale && (distance(c, tone) >= 50.0
        || ch * 2.0 <= chroma(tone) + 8.0
            && l >= t - 20.0
            && (chroma(tone) >= 12.0 || l >= t + 25.0))
}
fn components(mask: &[bool; 192], split_middle: bool) -> Vec<Vec<usize>> {
    let mut visited = [false; 192];
    let mut groups = Vec::new();
    for start in 0..192 {
        if !mask[start] || visited[start] {
            continue;
        }
        visited[start] = true;
        let mut queue = VecDeque::from([start]);
        let mut cells = Vec::new();
        while let Some(i) = queue.pop_front() {
            cells.push(i);
            for j in neighbors(i) {
                let (x, _) = coords(i);
                let (nx, _) = coords(j);
                if split_middle && (x < 4) != (nx < 4) {
                    continue;
                }
                if mask[j] && !visited[j] {
                    visited[j] = true;
                    queue.push_back(j);
                }
            }
        }
        groups.push(cells);
    }
    groups
}
fn bounds(cells: &[usize]) -> [i32; 4] {
    let mut minx = 16;
    let mut miny = 8;
    let mut maxx = -9;
    let mut maxy = -1;
    for &i in cells {
        let (x, y) = coords(i);
        minx = minx.min(x);
        maxx = maxx.max(x);
        miny = miny.min(y);
        maxy = maxy.max(y);
    }
    [minx, miny, maxx - minx + 1, maxy - miny + 1]
}
fn palette_eye(
    grid: &SkinGrid,
    hat: bool,
    b: [i32; 4],
    tone: Rgba,
    pupil: Option<(i32, i32)>,
) -> Eye {
    let [x, y, w, h] = b;
    let mut brightest = (f32::MIN, [x, y]);
    let mut darkest = (f32::MAX, tone);
    let mut hats = 0;
    let mut pupil_min = 16;
    let mut pupil_max = -9;
    let mut iris = tone;
    let mut iris_score = -1.0;
    for row in y..y + h {
        for col in x..x + w {
            let c = grid.shown(col, row, hat);
            let lum = luminance(c);
            if lum > brightest.0 {
                brightest = (lum, [col, row]);
            }
            if lum < darkest.0 {
                darkest = (lum, c);
            }
            hats += i32::from(grid.on_hat(col, row, hat));
        }
    }
    let sclera = grid.shown(brightest.1[0], brightest.1[1], hat);
    let has_white = white_like(sclera, tone) || brightest.0 - darkest.0 > 70.0;
    for row in y..y + h {
        for col in x..x + w {
            let c = grid.shown(col, row, hat);
            let pupil_color = if has_white {
                luminance(sclera) - luminance(c) >= 25.0 && distance(sclera, c) >= 45.0
                    || chroma(c) >= 30.0
                        && distance(sclera, c) >= 38.0
                        && luminance(c) <= luminance(sclera) + 8.0
            } else {
                true
            };
            if pupil_color {
                pupil_min = pupil_min.min(col);
                pupil_max = pupil_max.max(col);
                let score = chroma(c) * 1.5 - luminance(c) * 0.1;
                if score > iris_score {
                    iris = c;
                    iris_score = score;
                }
            }
        }
    }
    if let Some((col, width)) = pupil {
        pupil_min = col;
        pupil_max = col + width - 1;
    }
    let mut lid = tone;
    let mut lid_score = f32::MAX;
    for sy in 0..8 {
        for sx in -8..16 {
            if sy >= y && sy < y + h && sx >= x && sx < x + w {
                continue;
            }
            let c = grid.shown(sx, sy, hat);
            if !skin_like(c, tone) {
                continue;
            }
            let d = (sx - (x + w / 2)).abs() as f32
                + (sy - y).abs() as f32 * 1.5
                + if sy >= y { 2.0 } else { 0.0 };
            if d < lid_score {
                lid = c;
                lid_score = d;
            }
        }
    }
    let cheek = if y + h < 8 && skin_like(grid.shown(x + w / 2, y + h, hat), tone) {
        grid.shown(x + w / 2, y + h, hat)
    } else {
        lid
    };
    let mut ink = darkest.1;
    let k = if darkest.0 <= 60.0 {
        0.8
    } else {
        60.0 / darkest.0.max(1.0)
    };
    for v in &mut ink[..3] {
        *v = (*v as f32 * k).round() as u8;
    }
    Eye {
        col: x,
        row: y,
        width: w,
        height: h,
        hat: hats * 2 > w * h,
        pupil_col: pupil_min,
        pupil_width: (pupil_max - pupil_min + 1).max(0),
        white: has_white.then_some(brightest.1),
        lid,
        ink,
        iris,
        sclera,
        cheek,
        side: usize::from(x * 2 + w > 8),
    }
}

#[derive(Clone)]
struct Candidate {
    b: [i32; 4],
    score: f32,
    cells: usize,
    whites: usize,
}
pub(super) fn detect(grid: &SkinGrid, hat: bool, marks: Option<&FaceMarks>) -> FaceRig {
    let mut seen = [[0; 4]; 192];
    for (i, c) in seen.iter_mut().enumerate() {
        let (x, y) = coords(i);
        *c = grid.shown(x, y, hat);
    }
    let tone = skin_tone(&seen);
    if let Some(marks) = marks.filter(|m| m.cells.iter().any(|&x| x != 0)) {
        return marked(grid, hat, marks, tone);
    }
    let skin: [bool; 192] = std::array::from_fn(|i| skin_like(seen[i], tone));
    let white: [bool; 192] = std::array::from_fn(|i| white_like(seen[i], tone));
    // Reject connected hair/helmet from the top, rather than assuming every
    // dark texel is an eye. Color continuity prevents hair swallowing sclera.
    let mut cover = [false; 192];
    let mut queue = VecDeque::new();
    for x in -8..16 {
        let i = index(x, 0);
        if !skin[i] {
            cover[i] = true;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        for j in neighbors(i) {
            if !cover[j] && !skin[j] && !white[j] && distance(seen[i], seen[j]) <= 38.0 {
                cover[j] = true;
                queue.push_back(j);
            }
        }
    }
    let feature: [bool; 192] = std::array::from_fn(|i| {
        let (_, y) = coords(i);
        y > 0
            && y < 7
            && !cover[i]
            && !skin[i]
            && (white[i]
                || luminance(seen[i]) < luminance(tone) * 0.72
                || hue_delta(seen[i], tone) > 30.0)
    });
    let mut candidates = Vec::new();
    for cells in components(&feature, true) {
        let b = bounds(&cells);
        let [x, y, w, h] = b;
        if w > 6 || h > 4 || y < 1 || y + h > 7 || w * h > 20 {
            continue;
        }
        let whites = cells.iter().filter(|&&i| white[i]).count();
        let side_distance = (x as f32 + w as f32 * 0.5 - 3.5).abs();
        let mut score = if whites > 0 { 3.0 } else { 1.0 };
        score += if (3..=5).contains(&y) { 1.0 } else { 0.0 };
        score += if w <= 3 && h <= 2 { 0.5 } else { -0.5 };
        score -= if side_distance > 8.0 { 1.0 } else { 0.0 };
        if whites > 0 && whites < cells.len() {
            score += 1.0;
        }
        candidates.push(Candidate {
            b,
            score,
            cells: cells.len(),
            whites,
        });
    }
    // Bright sclera can touch across the nose: separately search a middle eye.
    for cells in components(&feature, false) {
        let b = bounds(&cells);
        if b[0] < 4 && b[0] + b[2] > 4 && b[2] <= 6 && b[3] <= 4 && b[1] >= 2 && b[1] + b[3] <= 7 {
            let whites = cells.iter().filter(|&&i| white[i]).count();
            if whites >= 2 && whites < cells.len() {
                candidates.push(Candidate {
                    b,
                    score: 3.5,
                    cells: cells.len(),
                    whites,
                });
            }
        }
    }
    let mut choice: Vec<usize> = Vec::new();
    let mut top = 3.8;
    for (i, a) in candidates.iter().enumerate() {
        if a.b[0] < 4 && a.b[0] + a.b[2] > 4 && a.score > top {
            top = a.score;
            choice = vec![i];
        }
        for (j, b) in candidates.iter().enumerate().skip(i + 1) {
            let [ax, ay, aw, ah] = a.b;
            let [bx, by, bw, bh] = b.b;
            if (ax * 2 + aw <= 8) == (bx * 2 + bw <= 8) {
                continue;
            }
            let row = (ay - by).abs();
            let height = (ah - bh).abs();
            let mirror = (ax + bx + aw - 8).abs();
            if row > 1 || height > 1 || mirror > 2 {
                continue;
            }
            let mut score = a.score + b.score + 2.5
                - row as f32
                - height as f32 * 0.7
                - mirror as f32 * 0.7
                - (aw - bw).abs() as f32 * 0.7;
            if a.whites == 0 && b.whites == 0 && (a.cells == 1 || b.cells == 1) {
                score -= 1.0;
            }
            if score > top {
                top = score;
                choice = vec![i, j];
            }
        }
    }
    let mut rig = FaceRig {
        skin: tone,
        confidence: if choice.len() == 2 {
            ((top - 4.0) / 7.0).clamp(0.0, 1.0)
        } else {
            0.4
        },
        ..FaceRig::default()
    };
    for &i in &choice {
        rig.eyes
            .push(palette_eye(grid, hat, candidates[i].b, tone, None));
    }
    rig.eyes.sort_by_key(|e| e.col);
    // A thin, isolated contrasting row immediately above an eye is a brow.
    for e in &rig.eyes {
        let mut best: Option<Brow> = None;
        for y in (1..e.row).rev().take(3) {
            let mut start = None;
            let mut end = 0;
            for x in e.col.max(-8)..(e.col + e.width).min(16) {
                let i = index(x, y);
                if !skin[i] && !white[i] && !cover[i] && luminance(seen[i]) < luminance(tone) - 28.0
                {
                    start.get_or_insert(x);
                    end = x;
                }
            }
            if let Some(x) = start
                && end - x + 1 >= 2
            {
                best = Some(Brow {
                    col: x,
                    row: y,
                    width: end - x + 1,
                    hat: grid.on_hat(x, y, hat),
                    skin: e.lid,
                    color: grid.shown(x, y, hat),
                    side: e.side,
                });
                break;
            }
        }
        if let Some(b) = best {
            rig.brows.push(b);
        }
    }
    rig
}
fn marked(grid: &SkinGrid, hat: bool, marks: &FaceMarks, tone: Rgba) -> FaceRig {
    let mask: [bool; 192] = std::array::from_fn(|i| marks.cells[i] == 1 || marks.cells[i] == 2);
    let mut groups = components(&mask, false);
    // Two adjacent eye whites are separate eyes when each half has a pupil.
    let mut split = Vec::new();
    for cells in groups.drain(..) {
        let left = cells
            .iter()
            .any(|&i| coords(i).0 < 4 && marks.cells[i] == 2);
        let right = cells
            .iter()
            .any(|&i| coords(i).0 >= 4 && marks.cells[i] == 2);
        let middle_pupil = (0..8).any(|y| marks.get(3, y) == 2 && marks.get(4, y) == 2);
        if left && right && !middle_pupil {
            split.push(
                cells
                    .iter()
                    .copied()
                    .filter(|&i| coords(i).0 < 4)
                    .collect::<Vec<_>>(),
            );
            split.push(
                cells
                    .iter()
                    .copied()
                    .filter(|&i| coords(i).0 >= 4)
                    .collect::<Vec<_>>(),
            );
        } else {
            split.push(cells);
        }
    }
    let mut rig = FaceRig {
        skin: tone,
        confidence: 1.0,
        ..FaceRig::default()
    };
    for cells in split {
        if cells.is_empty() {
            continue;
        }
        let mut min = 16;
        let mut max = -9;
        for &i in &cells {
            if marks.cells[i] == 2 {
                min = min.min(coords(i).0);
                max = max.max(coords(i).0);
            }
        }
        let b = bounds(&cells);
        rig.eyes.push(palette_eye(
            grid,
            hat,
            b,
            tone,
            Some((min, (max - min + 1).max(0))),
        ));
    }
    let mask: [bool; 192] = std::array::from_fn(|i| marks.cells[i] == 3);
    for cells in components(&mask, false) {
        let [x, y, w, _] = bounds(&cells);
        let side = usize::from(x * 2 + w > 8);
        let skin = rig
            .eyes
            .iter()
            .find(|e| e.side == side)
            .map(|e| e.lid)
            .unwrap_or(tone);
        rig.brows.push(Brow {
            col: x,
            row: y,
            width: w,
            hat: grid.on_hat(x, y, hat),
            skin,
            color: grid.shown(x, y, hat),
            side,
        });
    }
    rig.eyes.sort_by_key(|e| e.col);
    rig
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    fn skin(scale: usize) -> Vec<u8> {
        let mut rgba = vec![0; 64 * 64 * scale * scale * 4];
        for y in 0..8 {
            for x in -8..16 {
                let color = if y == 4 && [1, 6].contains(&x) {
                    [245, 245, 245, 255]
                } else if y == 4 && [2, 5].contains(&x) {
                    [40, 55, 100, 255]
                } else {
                    [185, 135, 100, 255]
                };
                for dy in 0..scale {
                    for dx in 0..scale {
                        let i = (((y + 8) as usize * scale + dy) * 64 * scale
                            + (x + 8) as usize * scale
                            + dx)
                            * 4;
                        rgba[i..i + 4].copy_from_slice(&color);
                    }
                }
            }
        }
        rgba
    }
    #[test]
    fn symmetric_eyes_detect_at_every_supported_scale() {
        for scale in [1, 2, 4, 8, 16] {
            let rgba = skin(scale);
            let g = SkinGrid::new(&rgba, 64 * scale as u32, 64 * scale as u32).unwrap();
            let r = detect(&g, true, None);
            assert_eq!(r.eyes.len(), 2);
            assert_eq!(r.eyes[0].row, 4);
            assert_eq!(r.eyes[0].width, 2);
            assert!(r.eyes.iter().all(|e| e.white.is_some()));
        }
    }
    #[test]
    fn marks_support_wrap_across_front_side_seam() {
        let g = SkinGrid::new(&skin(1), 64, 64).unwrap();
        let mut marks = FaceMarks::default();
        marks.set(-1, 4, 1);
        marks.set(0, 4, 2);
        marks.set(7, 4, 2);
        marks.set(8, 4, 1);
        let r = detect(&g, false, Some(&marks));
        assert_eq!(r.eyes.len(), 2);
        assert_eq!(r.eyes[0].col, -1);
        assert_eq!(r.eyes[1].col, 7);
    }
    #[test]
    fn uniform_skin_has_no_false_eyes() {
        let rgba = [185, 135, 100, 255].repeat(64 * 64);
        let g = SkinGrid::new(&rgba, 64, 64).unwrap();
        assert!(detect(&g, false, None).eyes.is_empty());
    }
}
