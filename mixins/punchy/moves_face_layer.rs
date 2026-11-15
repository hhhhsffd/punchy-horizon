// Shared by the game adapter and all native avatar render paths, including
// inventory portraits and the GPU-skinned player body.
use super::{Atlas, Mob, MobData, Vertex};
use std::{cell::RefCell, collections::BTreeMap};

thread_local! {
    static EMISSION: RefCell<BTreeMap<String,String>> = const {RefCell::new(BTreeMap::new())};
    static BASE: std::cell::Cell<bool> = const {std::cell::Cell::new(false)};
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
            .is_some_and(|skin| EMISSION.with(|s| s.borrow().contains_key(skin)))
        || BASE.with(|flag| flag.replace(true))
    {
        None
    } else {
        Some(RenderGuard)
    }
}
pub fn clear() {
    EMISSION.with(|s| s.borrow_mut().clear());
}
pub fn set(skin: &str, emission: Option<&str>) {
    EMISSION.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(emission) = emission {
            s.insert(skin.into(), emission.into());
        } else {
            s.remove(skin);
        }
    });
}
pub fn render(
    m: &Mob,
    alpha: f32,
    base: glam::Mat4,
    atlas: Atlas<'_>,
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
    let uv = EMISSION.with(|s| s.borrow().get(skin).and_then(|texture| atlas(texture)));
    let Some(uv) = uv else {
        return;
    };
    // Use the native model tree and the very same BodyPose/CEM animation, not
    // an independently rotated billboard that slips during crouch or landing.
    let (model, poses, _) = super::posed_body(m, alpha, data);
    super::draw(
        &model,
        base,
        &poses,
        uv,
        0xFFFF_FFFF,
        super::FULL_BRIGHT,
        0,
        out,
    );
}
