// All face pieces use the native posed model tree, including CEM and BodyPose.
use super::super::anim::punchy_moves::engine::moves_expressions::{
    FaceOutput, FaceQuad, GlowAnchor, Rgba,
};
use super::{Atlas, Mob, MobData, Vertex};
use glam::{DVec3, Mat4, Vec3};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
};

pub const WHITE: &str = "punchy/face_geometry_white";
struct Layer {
    quads: Vec<FaceQuad>,
    anchors: Vec<GlowAnchor>,
    time: f32,
    rendered_time: Option<f32>,
    trails: Vec<VecDeque<Point>>,
}
struct Point {
    position: DVec3,
    drift: Vec3,
    time: f32,
    power: f32,
}
thread_local! {
    static LAYERS: RefCell<BTreeMap<String,Layer>> = const {RefCell::new(BTreeMap::new())};
    static BASE: std::cell::Cell<bool> = const {std::cell::Cell::new(false)};
    static WINGS: RefCell<BTreeMap<u32,(f32,[f32;16])>> = const {RefCell::new(BTreeMap::new())};
}
pub fn record_wings(m: &Mob, partial: f32, original: Mat4, posed: Mat4) {
    if std::env::var_os("PUNCHY_MOVES_DIAGNOSTICS").is_none() || original.determinant().abs() < 1e-8
    {
        return;
    }
    WINGS.with(|w| {
        let mut w = w.borrow_mut();
        if w.len() >= 64 && !w.contains_key(&m.id) {
            w.clear();
        }
        w.insert(
            m.id,
            (
                m.age as f32 + partial,
                (original.inverse() * posed).to_cols_array(),
            ),
        );
    });
}
pub fn wing_diagnostic(m: &Mob) -> Option<[f32; 16]> {
    WINGS.with(|w| {
        w.borrow()
            .get(&m.id)
            .filter(|(age, _)| (*age - m.age as f32).abs() <= 2.)
            .map(|(_, matrix)| *matrix)
    })
}
pub struct RenderGuard;
impl Drop for RenderGuard {
    fn drop(&mut self) {
        BASE.with(|flag| flag.set(false));
    }
}
pub fn begin(m: &Mob, visible: bool) -> Option<RenderGuard> {
    if !visible
        || m.is_invisible()
        || m.extra.spectator
        || !m
            .skin
            .as_deref()
            .is_some_and(|skin| LAYERS.with(|s| s.borrow().contains_key(skin)))
        || BASE.with(|flag| flag.replace(true))
    {
        None
    } else {
        Some(RenderGuard)
    }
}
pub fn clear() {
    LAYERS.with(|s| s.borrow_mut().clear());
}
/// Apply only the authored body delta to a native equipment anchor. Calibrate
/// against the same model's vanilla pose, so native crouch/wing offsets are
/// retained once instead of applied twice.
pub fn body_attachment(m: &Mob, alpha: f32, base: Mat4, data: &MobData) -> Option<Mat4> {
    let (model, poses, _) = super::posed_body(m, alpha, data);
    let body = super::part_matrix(&model, base, &poses, "body", 0.)?;
    let mut vanilla = m.clone();
    vanilla.emote = None;
    let (rest_model, rest_poses, _) = super::posed_body(&vanilla, alpha, data);
    let rest = super::part_matrix(&rest_model, base, &rest_poses, "body", 0.)?;
    if !rest.is_finite() || rest.determinant().abs() < 1e-8 {
        return None;
    }
    let result = body * rest.inverse() * base;
    result.is_finite().then_some(result)
}
pub fn remove(skin: &str) {
    LAYERS.with(|s| {
        s.borrow_mut().remove(skin);
    });
}
pub fn set(skin: &str, output: &FaceOutput) {
    LAYERS.with(|s| {
        let mut s = s.borrow_mut();
        let layer = s.entry(skin.into()).or_insert_with(|| Layer {
            quads: Vec::new(),
            anchors: Vec::new(),
            time: 0.,
            rendered_time: None,
            trails: Vec::new(),
        });
        let time = output.frame.age_ticks / 20.;
        if time < layer.time {
            layer.trails.clear();
            layer.rendered_time = None;
        }
        layer.time = time;
        layer.quads.clone_from(&output.quads);
        layer.anchors.clone_from(&output.glow_anchors);
        layer.trails.resize_with(layer.anchors.len(), VecDeque::new);
        layer.trails.truncate(layer.anchors.len());
    });
}
/// World-space anchors use the same posed head and interpolated root as meshes.
/// This path is also available before mesh extraction and never writes world light.
pub fn lights(m: &Mob, alpha: f32, data: &MobData) -> Vec<([f64; 3], u32, f32, f32)> {
    if m.is_invisible() || m.extra.spectator {
        return Vec::new();
    }
    let Some(skin) = m.skin.as_deref() else {
        return Vec::new();
    };
    let base = super::base_transform_at(m, alpha, Mat4::IDENTITY);
    let (model, poses, _) = super::posed_body(m, alpha, data);
    let Some(head) = super::part_matrix(&model, base, &poses, "head", 0.) else {
        return Vec::new();
    };
    if super::find_part(&model, "head").is_some_and(|p| poses.is_hidden(p)) {
        return Vec::new();
    }
    let origin = DVec3::new(m.previous.x, m.previous.y, m.previous.z).lerp(
        DVec3::new(m.position.x, m.position.y, m.position.z),
        f64::from(alpha),
    );
    LAYERS.with(|s| {
        let s = s.borrow();
        s.get(skin).map_or_else(Vec::new, |layer| {
            layer
                .anchors
                .iter()
                .filter_map(|anchor| {
                    if anchor.strength <= 0. || !anchor.strength.is_finite() {
                        return None;
                    }
                    let position = origin
                        + head
                            .transform_point3(Vec3::from_array(anchor.position) / 16.)
                            .as_dvec3();
                    let [r, g, b, _] = anchor.color;
                    Some((
                        position.to_array(),
                        u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b),
                        (2.5 * anchor.strength.sqrt()).clamp(0.25, 8.),
                        (anchor.strength * 0.35).clamp(0., 1.),
                    ))
                })
                .collect()
        })
    })
}
fn color(c: Rgba) -> u32 {
    u32::from_le_bytes(c)
}
fn quad(
    points: [Vec3; 4],
    uv: [f32; 2],
    rgba: Rgba,
    normal: [i8; 4],
    light: u32,
    overlay: u32,
    out: &mut Vec<Vertex>,
) {
    if rgba[3] == 0 {
        return;
    }
    // Entity meshes are quad lists, not independently indexed triangles.
    for p in points {
        out.push(Vertex {
            pos: p.to_array(),
            uv,
            layer: overlay | crate::gfx::OIT_ENTITY,
            color: color(rgba),
            light,
            normal,
            glint_uv: [0.; 2],
        });
    }
}
fn sprite(
    center: Vec3,
    right: Vec3,
    up: Vec3,
    size: [f32; 2],
    rgb: Rgba,
    power: f32,
    uv: [f32; 2],
    out: &mut Vec<Vertex>,
) {
    // A bounded procedural falloff avoids shipping the original glow textures.
    // Per-cell opacity makes the core, halo and streak continuously fade.
    for y in 0..12 {
        for x in 0..12 {
            let u = (x as f32 + 0.5) / 6. - 1.;
            let v = (y as f32 + 0.5) / 6. - 1.;
            let fade = (-4. * (u * u + v * v)).exp();
            let mut c = rgb;
            c[3] = (255. * power * fade).clamp(0., 255.).round() as u8;
            let corner = |x: f32, y: f32| center + right * (x * size[0]) + up * (y * size[1]);
            let l = x as f32 / 6. - 1.;
            let t = y as f32 / 6. - 1.;
            quad(
                [
                    corner(l, t),
                    corner(l + 1. / 6., t),
                    corner(l + 1. / 6., t + 1. / 6.),
                    corner(l, t + 1. / 6.),
                ],
                uv,
                c,
                [0; 4],
                super::FULL_BRIGHT,
                0,
                out,
            );
        }
    }
}
pub fn render(
    m: &Mob,
    alpha: f32,
    base: Mat4,
    atlas: Atlas<'_>,
    light: u32,
    data: &MobData,
    visible: bool,
    out: &mut Vec<Vertex>,
) {
    if !visible || m.is_invisible() || m.extra.spectator {
        return;
    }
    let Some(skin) = m.skin.as_deref() else {
        return;
    };
    let Some(white) = atlas(WHITE) else {
        return;
    };
    let uv = [(white[0] + white[2]) * 0.5, (white[1] + white[3]) * 0.5];
    let (model, poses, _) = super::posed_body(m, alpha, data);
    let Some(head) = super::part_matrix(&model, base, &poses, "head", 0.) else {
        return;
    };
    if super::find_part(&model, "head").is_some_and(|p| poses.is_hidden(p)) {
        return;
    }
    let normal_matrix = head.inverse().transpose();
    LAYERS.with(|s| {
        let mut s = s.borrow_mut();
        let Some(layer) = s.get_mut(skin) else {
            return;
        };
        let dt = layer
            .rendered_time
            .map_or(0., |last| (layer.time - last).clamp(0., 0.1));
        layer.rendered_time = Some(layer.time);
        for q in &layer.quads {
            let points = q
                .positions
                .map(|p| head.transform_point3(Vec3::from_array(p) / 16.));
            let n = normal_matrix
                .transform_vector3(Vec3::from_array(q.normal))
                .normalize_or_zero();
            let normal = [
                (n.x * 127.).round() as i8,
                (n.y * 127.).round() as i8,
                (n.z * 127.).round() as i8,
                -127,
            ];
            quad(
                points,
                uv,
                q.color,
                normal,
                light,
                super::body_overlay(m, alpha),
                out,
            );
            if q.emission[3] > 0 {
                // Offset the full-bright pass in head space, including nonuniform scale.
                let points = q.positions.map(|p| {
                    head.transform_point3(
                        (Vec3::from_array(p) + Vec3::from_array(q.normal) * 0.0005) / 16.,
                    )
                });
                quad(points, uv, q.emission, [0; 4], super::FULL_BRIGHT, 0, out);
            }
        }
        for (anchor, trail) in layer.anchors.iter().zip(&mut layer.trails) {
            let center = head.transform_point3(Vec3::from_array(anchor.position) / 16.);
            let pixel = head.transform_vector3(Vec3::X / 16.).length();
            if pixel <= 0. || !pixel.is_finite() {
                continue;
            }
            let view = (-center).normalize_or_zero();
            let right = Vec3::Y.cross(view).normalize_or_zero();
            let right = if right.length_squared() < 0.1 {
                Vec3::X
            } else {
                right
            };
            let up = view.cross(right).normalize_or_zero();
            let hot = std::array::from_fn(|i| {
                if i == 3 {
                    255
                } else {
                    (anchor.color[i] as f32 * 0.45 + 140.25).round() as u8
                }
            });
            let power = anchor.strength.max(0.);
            let bloom = if anchor.effects & 1 != 0 {
                anchor.effect_strengths[0]
            } else {
                0.
            };
            let glare = if anchor.effects & 2 != 0 {
                anchor.effect_strengths[1]
            } else {
                0.
            };
            if bloom > 0. {
                let width = bloom.sqrt() * pixel;
                sprite(
                    center,
                    right,
                    up,
                    [10. * width; 2],
                    anchor.color,
                    power * bloom * 0.06,
                    uv,
                    out,
                );
                sprite(
                    center,
                    right,
                    up,
                    [4.5 * width; 2],
                    anchor.color,
                    power * bloom * 0.28,
                    uv,
                    out,
                );
                sprite(
                    center,
                    right,
                    up,
                    [1.6 * width; 2],
                    hot,
                    power * bloom * 0.3,
                    uv,
                    out,
                );
            }
            if glare > 0. {
                let width = glare.sqrt() * pixel;
                sprite(
                    center,
                    right,
                    up,
                    [14. * width, pixel],
                    anchor.color,
                    power * glare * 0.5,
                    uv,
                    out,
                );
                sprite(
                    center,
                    right,
                    up,
                    [8.4 * width, pixel * 0.5],
                    hot,
                    power * glare * 0.3,
                    uv,
                    out,
                );
            }
            if anchor.effects & 12 == 0 {
                trail.clear();
                continue;
            }
            // Store world positions, not camera-relative ones: turning the camera
            // must not manufacture trails, and teleports must not join ribbons.
            let world = DVec3::new(m.previous.x, m.previous.y, m.previous.z).lerp(
                DVec3::new(m.position.x, m.position.y, m.position.z),
                f64::from(alpha),
            ) + (center - base.w_axis.truncate()).as_dvec3();
            let time = layer.time;
            if trail.back().is_some_and(|last| {
                world.distance_squared(last.position) > (pixel as f64 * 96.).powi(2)
            }) {
                trail.clear();
            }
            if dt > 0. {
                for point in trail.iter_mut() {
                    point.position += point.drift.as_dvec3() * dt as f64;
                    point.drift *= (-2.5 * dt).exp();
                }
            }
            while trail.front().is_some_and(|p| time - p.time > 0.9) {
                trail.pop_front();
            }
            if power > 0. && trail.back().is_none_or(|p| time > p.time) {
                let outward = head
                    .transform_vector3(Vec3::new(0., 0., -1.))
                    .normalize_or_zero();
                trail.push_back(Point {
                    position: world,
                    drift: (Vec3::Y + outward * 0.9) * pixel,
                    time,
                    power,
                });
            }
            while trail.len() > 192 {
                trail.pop_front();
            }
            for (bit, life, width, gain, index) in
                [(4, 0.5_f32, 2.2_f32, 0.55_f32, 2), (8, 0.9, 1., 0.8, 3)]
            {
                if anchor.effects & bit == 0 {
                    continue;
                }
                let strength = anchor.effect_strengths[index];
                let mut previous: Option<(Vec3, Vec3, f32)> = None;
                for p in trail.iter().rev() {
                    let age = ((time - p.time) / life).clamp(0., 1.);
                    if age >= 1. {
                        break;
                    }
                    let at = center + (p.position - world).as_vec3();
                    let tail_width = width * pixel * 0.5 * (1. - age).powf(0.75);
                    let alpha = p.power * strength * gain * (1. - age).powf(1.6);
                    if let Some((last, last_side, last_alpha)) = previous {
                        let tangent = (last - at).normalize_or_zero();
                        let mut side = tangent.cross(view).normalize_or_zero();
                        if side.length_squared() < 0.1 {
                            side = right;
                        }
                        if side.dot(last_side) < 0. {
                            side = -side;
                        }
                        let side = side * tail_width;
                        let mut c = anchor.trail_color;
                        c[3] = (255. * alpha.min(last_alpha)).clamp(0., 255.).round() as u8;
                        quad(
                            [last + last_side, last - last_side, at - side, at + side],
                            uv,
                            c,
                            [0; 4],
                            super::FULL_BRIGHT,
                            0,
                            out,
                        );
                        previous = Some((at, side, alpha));
                    } else {
                        previous = Some((at, right * tail_width, alpha));
                    }
                }
            }
        }
    });
}
