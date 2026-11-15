//! Expressions-compatible rig/style metadata in unused skin pixels (56..64,
//! 16..32), including side marks, per-eye lights and personality overrides.
use super::detect::{index, inside};
use super::motion::LOOK_NAMES;
use super::{EyeLight, FacePersonality, FaceSettings, Rgba, SkinGrid};

const SIGNATURE: &[u8; 24] = b"punchy moves face rig v1";
const MARK_COLORS: [Rgba; 4] = [
    [0, 0, 0, 255],
    [255, 255, 255, 255],
    [32, 64, 192, 255],
    [128, 64, 32, 255],
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceMarks {
    /// Eight rows of 24 cells: side right, front, side left. 0=none, 1=eye
    /// white, 2=pupil, 3=brow. Coordinates are x=-8..15, y=0..7.
    pub cells: [u8; 192],
}
impl Default for FaceMarks {
    fn default() -> Self {
        Self { cells: [0; 192] }
    }
}
impl FaceMarks {
    pub fn set(&mut self, x: i32, y: i32, kind: u8) {
        if inside(x, y) {
            self.cells[index(x, y)] = kind.min(3);
        }
    }
    pub fn get(&self, x: i32, y: i32) -> u8 {
        if inside(x, y) {
            self.cells[index(x, y)]
        } else {
            0
        }
    }
    pub fn has_sides(&self) -> bool {
        (0..8).any(|y| (-8..16).any(|x| !(0..8).contains(&x) && self.get(x, y) != 0))
    }
    /// Original text format: front, optional right side, optional left side,
    /// separated by whitespace. Symbols `.epb` represent the four mark kinds.
    pub fn parse(text: &str) -> Self {
        let mut m = Self::default();
        for (y, line) in text.lines().take(8).enumerate() {
            for (panel, word) in line.split_whitespace().take(3).enumerate() {
                let start = [0, -8, 8][panel];
                for (x, c) in word.chars().take(8).enumerate() {
                    let kind = match c {
                        'e' => 1,
                        'p' => 2,
                        'b' => 3,
                        _ => 0,
                    };
                    m.set(start + x as i32, y as i32, kind);
                }
            }
        }
        m
    }
    pub fn format(&self) -> String {
        let mut s = String::new();
        for y in 0..8 {
            let panels = if self.has_sides() { 3 } else { 1 };
            for p in 0..panels {
                if p > 0 {
                    s.push(' ');
                }
                for x in 0..8 {
                    s.push(b".epb"[self.get([0, -8, 8][p] + x, y).min(3) as usize] as char);
                }
            }
            s.push('\n');
        }
        s
    }
}
#[derive(Clone, Debug)]
pub struct SkinMetadata {
    pub marks: FaceMarks,
    pub settings: Option<FaceSettings>,
}

pub fn import_skin_metadata(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Option<SkinMetadata>, String> {
    Ok(read_grid(&SkinGrid::new(rgba, width, height)?))
}
/// Only the documented unused 8x16 logical-pixel patch is changed. Rejects
/// legacy skins: their unused area is not safely exportable by this format.
pub fn export_skin_metadata(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    metadata: &SkinMetadata,
) -> Result<(), String> {
    let grid = SkinGrid::new(rgba, width, height)?;
    if width != height {
        return Err("metadata export requires a square skin".into());
    }
    let scale = grid.scale;
    for y in 16..32 {
        for x in 56..64 {
            write_block(rgba, width as usize, scale, x, y, MARK_COLORS[0]);
        }
    }
    for (i, chunk) in SIGNATURE.as_chunks::<3>().0.iter().enumerate() {
        write_block(
            rgba,
            width as usize,
            scale,
            56 + i,
            16,
            [chunk[0], chunk[1], chunk[2], 255],
        );
    }
    for y in 0..8 {
        for x in 0..8 {
            write_block(
                rgba,
                width as usize,
                scale,
                56 + x,
                17 + y,
                MARK_COLORS[metadata.marks.get(x as i32, y as i32).min(3) as usize],
            );
        }
    }
    write_block(
        rgba,
        width as usize,
        scale,
        56,
        25,
        rgb(mark_checksum(&metadata.marks)),
    );
    if metadata.settings.is_some() || metadata.marks.has_sides() {
        let mut data = encode_style(
            metadata
                .settings
                .as_ref()
                .unwrap_or(&FaceSettings::default()),
        );
        if metadata.marks.has_sides() {
            if data.len() == 39 {
                data.push(0);
            }
            let start = data.len();
            data.resize(start + 32, 0);
            for y in 0..8 {
                for x in 0..16 {
                    let col = if x < 8 { x - 8 } else { x };
                    let i = (y * 16 + x) as usize;
                    data[start + i / 4] |= metadata.marks.get(col, y) << (i % 4 * 2);
                }
            }
        }
        if data.len() > 138 {
            return Err("skin metadata exceeds 138-byte style capacity".into());
        }
        let mut stream = [0; 144];
        stream[0] = b'F';
        stream[1] = b'S';
        stream[2] = data.len() as u8;
        stream[3..3 + data.len()].copy_from_slice(&data);
        let check = rgb(byte_checksum(&data));
        stream[3 + data.len()..6 + data.len()].copy_from_slice(&check[..3]);
        for i in 0..48 {
            write_block(
                rgba,
                width as usize,
                scale,
                56 + i % 8,
                26 + i / 8,
                [stream[i * 3], stream[i * 3 + 1], stream[i * 3 + 2], 255],
            );
        }
    }
    Ok(())
}
fn write_block(out: &mut [u8], width: usize, scale: usize, x: usize, y: usize, c: Rgba) {
    for dy in 0..scale {
        for dx in 0..scale {
            let i = ((y * scale + dy) * width + x * scale + dx) * 4;
            out[i..i + 4].copy_from_slice(&c);
        }
    }
}
pub(super) fn read_grid(grid: &SkinGrid) -> Option<SkinMetadata> {
    for (i, b) in SIGNATURE.as_chunks::<3>().0.iter().enumerate() {
        if grid.at(56 + i, 16)[..3] != *b {
            return None;
        }
    }
    let mut marks = FaceMarks::default();
    for y in 0..8 {
        for x in 0..8 {
            let c = grid.at(56 + x, 17 + y);
            let kind = MARK_COLORS
                .iter()
                .position(|p| (0..3).all(|i| (c[i] as i32 - p[i] as i32).abs() <= 24))?;
            marks.set(x as i32, y as i32, kind as u8);
        }
    }
    if color(grid.at(56, 25)) != mark_checksum(&marks) {
        return None;
    }
    let mut stream = [0; 144];
    for i in 0..48 {
        stream[i * 3..i * 3 + 3].copy_from_slice(&grid.at(56 + i % 8, 26 + i / 8)[..3]);
    }
    let n = stream[2] as usize;
    let data = if stream[0] == b'F'
        && stream[1] == b'S'
        && n > 0
        && n <= 138
        && byte_checksum(&stream[3..3 + n])
            == color([stream[3 + n], stream[4 + n], stream[5 + n], 255])
    {
        Some(&stream[3..3 + n])
    } else {
        None
    };
    let settings = data.and_then(decode_style);
    if let Some(data) = data.filter(|d| d.len() > 39) {
        let style_size = (40 + data[39] as usize).min(data.len());
        if data.len() >= style_size + 32 {
            for y in 0..8 {
                for x in 0..16 {
                    let i = (y * 16 + x) as usize;
                    marks.set(
                        if x < 8 { x - 8 } else { x },
                        y,
                        (data[style_size + i / 4] >> (i % 4 * 2)) & 3,
                    );
                }
            }
        }
    }
    Some(SkinMetadata { marks, settings })
}
fn rgb(n: u32) -> Rgba {
    [(n >> 16) as u8, (n >> 8) as u8, n as u8, 255]
}
fn color(c: Rgba) -> u32 {
    (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32
}
fn mark_checksum(m: &FaceMarks) -> u32 {
    let mut n = 3087675_u32;
    for y in 0..8 {
        for x in 0..8 {
            n = (n.wrapping_mul(31) + m.get(x, y).min(3) as u32 + 7) & 0xffffff;
        }
    }
    n
}
fn byte_checksum(data: &[u8]) -> u32 {
    data.iter().fold(5052867_u32, |n, &b| {
        (n.wrapping_mul(131) + b as u32 + 3) & 0xffffff
    })
}
fn strength(x: f32) -> u8 {
    ((super::finite(x, 1.0).clamp(0.1, 2.0) - 0.1) / 1.9 * 255.0).round() as u8
}
fn unstrength(x: u8) -> f32 {
    ((0.1 + x as f32 / 255.0 * 1.9) * 20.0).round() / 20.0
}
fn put_color(data: &mut [u8], at: usize, c: Option<Rgba>) {
    data[at..at + 3].copy_from_slice(&c.unwrap_or([0; 4])[..3]);
}
fn get_color(data: &[u8], at: usize) -> Rgba {
    [data[at], data[at + 1], data[at + 2], 255]
}
fn put_light(data: &mut [u8], at: usize, l: &EyeLight) {
    data[at] = (l.parts & 15) | ((l.effects & 15) << 4);
    for i in 0..4 {
        data[at + 1 + i] = strength(l.strengths[i]);
    }
    put_color(data, at + 5, l.color);
    put_color(data, at + 8, l.trail_color);
}
fn get_light(data: &[u8], at: usize, auto: u8, color_bit: u8, trail_bit: u8) -> EyeLight {
    EyeLight {
        parts: data[at] & 15,
        effects: data[at] >> 4,
        strengths: std::array::from_fn(|i| unstrength(data[at + 1 + i])),
        color: if auto & color_bit == 0 {
            Some(get_color(data, at + 5))
        } else {
            None
        },
        trail_color: if auto & trail_bit == 0 {
            Some(get_color(data, at + 8))
        } else {
            None
        },
    }
}
fn encode_style(s: &FaceSettings) -> Vec<u8> {
    let mut data = vec![0; 39];
    data[0] = 1;
    data[1] = u8::from(s.glow_pulse)
        | u8::from(s.glow_night_only) << 1
        | u8::from(s.over_hat) << 2
        | u8::from(!s.brows) << 3
        | u8::from(!s.hat_layer) << 4;
    data[2] = strength(s.glow_strength);
    data[3] = ["off", "bar", "dot", "skin"]
        .iter()
        .position(|&x| x == s.sparkle)
        .unwrap_or(0) as u8;
    put_color(&mut data, 4, Some(s.sparkle_color));
    data[7] = u8::from(s.line_color.is_none())
        | u8::from(s.lid_colors[0].is_none()) << 1
        | u8::from(s.lid_colors[1].is_none()) << 2
        | u8::from(s.lights[0].color.is_none()) << 3
        | u8::from(s.lights[0].trail_color.is_none()) << 4
        | u8::from(s.lights[1].color.is_none()) << 5
        | u8::from(s.lights[1].trail_color.is_none()) << 6;
    put_color(&mut data, 8, s.line_color);
    put_color(&mut data, 11, s.lid_colors[0]);
    put_color(&mut data, 14, s.lid_colors[1]);
    put_light(&mut data, 17, &s.lights[0]);
    put_light(&mut data, 28, &s.lights[1]);
    let p = &s.personality;
    let mut swaps = Vec::new();
    for (a, b) in p.swaps.iter().take(24) {
        if let Some(a) = LOOK_NAMES.iter().position(|&n| n == a) {
            let b = if b.is_empty() {
                Some(255)
            } else {
                LOOK_NAMES.iter().position(|&n| n == b).map(|n| n as u8)
            };
            if let Some(b) = b {
                swaps.push((a as u8, b));
            }
        }
    }
    let manner = !s.blush
        || s.blush_color.is_some()
        || s.ring_color.is_some()
        || s.tired_eyes != "rings"
        || s.expression_strength != 1.0
        || p.rest != "neutral"
        || p.blush > 0.0
        || !swaps.is_empty();
    if manner {
        data.resize(52 + swaps.len() * 2, 0);
        data[39] = (12 + swaps.len() * 2) as u8;
        data[40] = 1;
        let rings = ["rings", "lids", "off"]
            .iter()
            .position(|&x| x == s.tired_eyes)
            .unwrap_or(0) as u8;
        data[41] = u8::from(!s.blush)
            | u8::from(s.blush_color.is_none()) << 1
            | u8::from(s.ring_color.is_none()) << 2
            | rings << 3;
        put_color(&mut data, 42, s.blush_color);
        put_color(&mut data, 45, s.ring_color);
        data[48] = (s.expression_strength.clamp(0.25, 1.5) * 100.0).round() as u8;
        data[49] = LOOK_NAMES.iter().position(|&n| n == p.rest).unwrap_or(0) as u8;
        data[50] = (p.blush.clamp(0.0, 1.0) * 100.0).round() as u8;
        data[51] = swaps.len() as u8;
        for (i, (a, b)) in swaps.into_iter().enumerate() {
            data[52 + i * 2] = a;
            data[53 + i * 2] = b;
        }
    }
    data
}
fn decode_style(data: &[u8]) -> Option<FaceSettings> {
    if data.len() < 39 || data[0] != 1 {
        return None;
    }
    let mut s = FaceSettings::default();
    let flags = data[1];
    let auto = data[7];
    s.glow_pulse = flags & 1 != 0;
    s.glow_night_only = flags & 2 != 0;
    s.over_hat = flags & 4 != 0;
    s.brows = flags & 8 == 0;
    s.hat_layer = flags & 16 == 0;
    s.glow_strength = unstrength(data[2]);
    s.sparkle = ["off", "bar", "dot", "skin"]
        .get(data[3] as usize)
        .unwrap_or(&"off")
        .to_string();
    s.sparkle_color = get_color(data, 4);
    s.line_color = (auto & 1 == 0).then(|| get_color(data, 8));
    s.lid_colors = [
        (auto & 2 == 0).then(|| get_color(data, 11)),
        (auto & 4 == 0).then(|| get_color(data, 14)),
    ];
    s.lights = [
        get_light(data, 17, auto, 8, 16),
        get_light(data, 28, auto, 32, 64),
    ];
    if data.len() >= 49 && data[39] >= 9 && 40 + data[39] as usize <= data.len() && data[40] == 1 {
        let f = data[41];
        s.blush = f & 1 == 0;
        s.blush_color = (f & 2 == 0).then(|| get_color(data, 42));
        s.ring_color = (f & 4 == 0).then(|| get_color(data, 45));
        s.tired_eyes = ["rings", "lids", "off"]
            .get(((f >> 3) & 3) as usize)
            .unwrap_or(&"rings")
            .to_string();
        s.expression_strength = data[48] as f32 / 100.0;
        if data.len() >= 52 {
            let mut p = FacePersonality::preset("classic");
            p.id = "skin".into();
            p.rest = LOOK_NAMES
                .get(data[49] as usize)
                .unwrap_or(&"neutral")
                .to_string();
            p.blush = (data[50] as f32 / 100.0).clamp(0.0, 1.0);
            let size = (40 + data[39] as usize).min(data.len());
            for i in 0..(data[51] as usize).min(24) {
                let at = 52 + i * 2;
                if at + 1 >= size {
                    break;
                }
                if let Some(&a) = LOOK_NAMES.get(data[at] as usize) {
                    if data[at + 1] == 255 {
                        p.swaps.insert(a.into(), String::new());
                    } else if let Some(&b) = LOOK_NAMES.get(data[at + 1] as usize) {
                        p.swaps.insert(a.into(), b.into());
                    }
                }
            }
            for id in [
                "classic", "brave", "fierce", "cute", "cheerful", "calm", "grumpy", "gloomy",
            ] {
                let preset = FacePersonality::preset(id);
                if preset.rest == p.rest && preset.swaps == p.swaps && preset.blush == p.blush {
                    p = preset;
                    break;
                }
            }
            s.personality = p;
        }
    }
    s.normalize();
    Some(s)
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    #[test]
    fn marks_text_roundtrip_has_side_order_and_unknown_symbol_safety() {
        let mut m = FaceMarks::default();
        m.set(-1, 3, 2);
        m.set(5, 4, 1);
        m.set(8, 2, 3);
        assert_eq!(FaceMarks::parse(&m.format()), m);
        assert_eq!(FaceMarks::parse("?e......").get(0, 0), 0);
    }
    #[test]
    fn metadata_roundtrip_and_unused_pixel_boundary() {
        for scale in [1, 2, 16] {
            let width = 64 * scale;
            let mut skin = vec![77; width * width * 4];
            let original = skin.clone();
            let mut marks = FaceMarks::default();
            marks.set(1, 4, 1);
            marks.set(2, 4, 2);
            marks.set(-1, 4, 2);
            let mut settings = FaceSettings::default();
            settings.personality = FacePersonality::preset("cute");
            settings.lights[0].parts = 3;
            settings.lights[1].color = Some([55, 66, 77, 255]);
            let data = SkinMetadata {
                marks: marks.clone(),
                settings: Some(settings),
            };
            export_skin_metadata(&mut skin, width as u32, width as u32, &data).unwrap();
            let decoded = import_skin_metadata(&skin, width as u32, width as u32)
                .unwrap()
                .unwrap();
            assert_eq!(decoded.marks, marks);
            assert_eq!(decoded.settings.as_ref().unwrap().personality.id, "cute");
            assert_eq!(decoded.settings.as_ref().unwrap().lights[0].parts, 3);
            for y in 0..width {
                for x in 0..width {
                    if !(56 * scale..64 * scale).contains(&x)
                        || !(16 * scale..32 * scale).contains(&y)
                    {
                        let i = (y * width + x) * 4;
                        assert_eq!(skin[i..i + 4], original[i..i + 4]);
                    }
                }
            }
        }
    }
    #[test]
    fn corrupted_checksum_does_not_become_a_rig() {
        let mut skin = vec![0; 64 * 64 * 4];
        let mut marks = FaceMarks::default();
        marks.set(2, 4, 2);
        export_skin_metadata(
            &mut skin,
            64,
            64,
            &SkinMetadata {
                marks,
                settings: None,
            },
        )
        .unwrap();
        let i = (17 * 64 + 56) * 4;
        skin[i..i + 3].copy_from_slice(&[255; 3]);
        assert!(import_skin_metadata(&skin, 64, 64).unwrap().is_none());
    }
}
