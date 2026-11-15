//! Center-crosshair picking with player-eye reach and occlusion validation.
use super::{LookAngles, center_aim_ray};
use glam::{DVec3, IVec3, Mat4};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    /// Physical block collision shapes. Fluids must be ignored.
    Collider,
    /// Selectable block outline shapes. Fluids must be ignored.
    Outline,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHit {
    pub position: DVec3,
    pub block: IVec3,
    /// Outward block-face normal, useful when applying the selected hit.
    pub normal: IVec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntityHit {
    pub position: DVec3,
    pub entity: u64,
}

/// Engine adapter. All queries exclude the focused/local player. `pick_entity`
/// returns the nearest pickable non-spectator hit in an AABB enclosing the ray
/// inflated by one block, within `max_distance_squared` from `from`.
pub trait CameraWorld {
    fn clip(&mut self, from: DVec3, to: DVec3, kind: ClipKind) -> Option<BlockHit>;

    fn pick_entity(
        &mut self,
        _from: DVec3,
        _to: DVec3,
        _max_distance_squared: f64,
    ) -> Option<EntityHit> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AimRay {
    pub origin: DVec3,
    pub direction: DVec3,
}

impl AimRay {
    pub fn new(origin: DVec3, direction: DVec3) -> Option<Self> {
        if !origin.is_finite() || !direction.is_finite() || direction.length_squared() < 1e-12 {
            return None;
        }
        Some(Self {
            origin,
            direction: direction.normalize(),
        })
    }

    pub fn at(self, distance: f64) -> DVec3 {
        self.origin + self.direction * distance
    }

    pub fn look_from(self, eye: DVec3, distance: f64) -> Option<LookAngles> {
        LookAngles::toward(eye, self.at(distance))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TargetHit {
    Miss {
        position: DVec3,
        block: IVec3,
        normal: IVec3,
    },
    Block(BlockHit),
    Entity(EntityHit),
}

impl TargetHit {
    pub fn position(self) -> DVec3 {
        match self {
            Self::Miss { position, .. } => position,
            Self::Block(hit) => hit.position,
            Self::Entity(hit) => hit.position,
        }
    }

    pub fn entity(self) -> Option<u64> {
        match self {
            Self::Entity(hit) => Some(hit.entity),
            _ => None,
        }
    }

    /// Optional aiming helper; picking alone must not force the player to turn.
    pub fn look_from(self, eye: DVec3) -> Option<LookAngles> {
        LookAngles::toward(eye, self.position())
    }
}

/// Change these identities on level/player replacement to invalidate old frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameIdentity {
    pub world: u64,
    pub player: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct PickInput {
    /// True only when custom camera active, level/player exist, focus is local.
    pub enabled: bool,
    pub identity: FrameIdentity,
    pub camera_position: DVec3,
    pub camera_forward: DVec3,
    pub eye: DVec3,
    pub block_reach: f64,
    pub entity_reach: f64,
}

#[derive(Clone, Copy, Debug)]
struct DisplayedFrame {
    identity: FrameIdentity,
    ray: AimRay,
}

#[derive(Clone, Debug, Default)]
pub struct CameraTargeting {
    frame: Option<DisplayedFrame>,
}

impl CameraTargeting {
    pub fn clear(&mut self) {
        self.frame = None;
    }

    /// Capture after camera extraction, using the exact effect-adjusted matrices.
    /// Then call pick immediately and re-extract the block selection outline.
    pub fn capture(
        &mut self,
        enabled: bool,
        identity: FrameIdentity,
        camera_position: DVec3,
        view: Mat4,
        projection_with_effects: Mat4,
    ) -> Option<AimRay> {
        self.frame = None;
        if !enabled {
            return None;
        }
        let ray = center_aim_ray(camera_position, view, projection_with_effects)?;
        self.frame = Some(DisplayedFrame { identity, ray });
        Some(ray)
    }

    pub fn displayed_ray(&self, identity: FrameIdentity) -> Option<AimRay> {
        self.frame
            .filter(|frame| frame.identity == identity)
            .map(|frame| frame.ray)
    }

    /// Return None when vanilla should retain control. A Miss is a real custom
    /// pick result: clear crosshair entity, never retain last frame's entity.
    pub fn pick(&mut self, input: &PickInput, world: &mut impl CameraWorld) -> Option<TargetHit> {
        if !input.enabled {
            self.clear();
            return None;
        }
        if !input.eye.is_finite()
            || !input.block_reach.is_finite()
            || !input.entity_reach.is_finite()
            || input.block_reach < 0.
            || input.entity_reach < 0.
        {
            return None;
        }
        let ray = match self.displayed_ray(input.identity) {
            Some(ray) => ray,
            None => AimRay::new(input.camera_position, input.camera_forward)?,
        };
        let length =
            input.block_reach.max(input.entity_reach) + ray.origin.distance(input.eye) + 1.;
        let end = ray.at(length);
        let block = world
            .clip(ray.origin, end, ClipKind::Outline)
            .filter(|hit| hit.position.is_finite());
        // Even an unreachable or eye-occluded camera-side block limits entity
        // picking: the crosshair must never select through the displayed wall.
        let camera_limit = block.map_or(f64::MAX, |hit| ray.origin.distance_squared(hit.position));
        let valid_block = block.filter(|hit| {
            hit.position.distance_squared(input.eye) <= input.block_reach * input.block_reach
                && block_visible(world, input.eye, *hit)
        });
        let mut selected = valid_block.map_or_else(|| miss(end, ray.direction), TargetHit::Block);
        if let Some(entity) =
            world.pick_entity(ray.origin, end, (length * length).min(camera_limit))
            && entity.position.is_finite()
            && entity.position.distance_squared(input.eye)
                <= input.entity_reach * input.entity_reach
            && ray.origin.distance_squared(entity.position) < camera_limit
            && entity_visible(world, input.eye, entity.position)
        {
            selected = TargetHit::Entity(entity);
        }
        Some(selected)
    }
}

fn block_visible(world: &mut impl CameraWorld, eye: DVec3, target: BlockHit) -> bool {
    let delta = target.position - eye;
    let end = if delta.length_squared() > 1e-12 {
        target.position + delta.normalize() * 0.01
    } else {
        target.position
    };
    world
        .clip(eye, end, ClipKind::Outline)
        .is_some_and(|hit| hit.block == target.block)
}

fn entity_visible(world: &mut impl CameraWorld, eye: DVec3, target: DVec3) -> bool {
    world
        .clip(eye, target, ClipKind::Collider)
        .is_none_or(|hit| {
            hit.position.is_finite()
                && eye.distance_squared(hit.position) + 1e-4 >= eye.distance_squared(target)
        })
}

fn miss(position: DVec3, direction: DVec3) -> TargetHit {
    let facing = -direction;
    let rounded = (facing * 1000.).round();
    // Vanilla chooses the largest dot product against the six axis normals.
    let mut normal = IVec3::new(0, 0, -1);
    let mut best = f64::MIN;
    for axis in [
        IVec3::new(0, -1, 0),
        IVec3::Y,
        IVec3::new(0, 0, -1),
        IVec3::Z,
        IVec3::new(-1, 0, 0),
        IVec3::X,
    ] {
        let dot = rounded.dot(axis.as_dvec3());
        if dot > best {
            best = dot;
            normal = axis;
        }
    }
    TargetHit::Miss {
        position,
        block: position.floor().as_ivec3(),
        normal,
    }
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct World {
        clips: VecDeque<Option<BlockHit>>,
        entity: Option<EntityHit>,
        entity_limit: f64,
    }
    impl CameraWorld for World {
        fn clip(&mut self, _: DVec3, _: DVec3, _: ClipKind) -> Option<BlockHit> {
            self.clips.pop_front().flatten()
        }
        fn pick_entity(&mut self, _: DVec3, _: DVec3, limit: f64) -> Option<EntityHit> {
            self.entity_limit = limit;
            self.entity
        }
    }

    fn input() -> PickInput {
        PickInput {
            enabled: true,
            identity: FrameIdentity {
                world: 1,
                player: 2,
            },
            camera_position: DVec3::new(1., 0., -2.),
            camera_forward: DVec3::Z,
            eye: DVec3::ZERO,
            block_reach: 4.5,
            entity_reach: 3.,
        }
    }

    fn block(position: DVec3) -> BlockHit {
        BlockHit {
            position,
            block: IVec3::new(1, 0, 2),
            normal: IVec3::NEG_Z,
        }
    }

    #[test]
    fn entity_in_front_of_block_wins_only_if_eye_can_reach_and_see_it() {
        let target = block(DVec3::new(1., 0., 3.));
        let entity = EntityHit {
            position: DVec3::new(1., 0., 2.),
            entity: 42,
        };
        let mut world = World {
            clips: [Some(target), Some(target), None].into(),
            entity: Some(entity),
            entity_limit: 0.,
        };
        let mut targeting = CameraTargeting::default();
        let selected = targeting.pick(&input(), &mut world).unwrap();
        assert_eq!(selected, TargetHit::Entity(entity));
        assert_eq!(world.entity_limit, 25.);
    }

    #[test]
    fn unreachable_camera_wall_still_blocks_entity_selection() {
        let wall = block(DVec3::new(1., 0., -1.));
        let entity = EntityHit {
            position: DVec3::new(1., 0., 1.),
            entity: 42,
        };
        let mut world = World {
            clips: [Some(wall)].into(),
            entity: Some(entity),
            entity_limit: 0.,
        };
        let mut input = input();
        input.block_reach = 0.5;
        let selected = CameraTargeting::default().pick(&input, &mut world).unwrap();
        assert!(matches!(selected, TargetHit::Miss { .. }));
        assert_eq!(world.entity_limit, 1.);
    }

    #[test]
    fn player_eye_occlusion_rejects_a_camera_visible_block() {
        let target = block(DVec3::new(1., 0., 2.));
        let occluder = BlockHit {
            block: IVec3::ZERO,
            ..target
        };
        let mut world = World {
            clips: [Some(target), Some(occluder)].into(),
            entity: None,
            entity_limit: 0.,
        };
        let selected = CameraTargeting::default()
            .pick(&input(), &mut world)
            .unwrap();
        assert!(matches!(selected, TargetHit::Miss { .. }));
        assert_eq!(selected.entity(), None);
    }

    #[test]
    fn displayed_rays_are_scoped_to_world_and_player_and_disabled_pick_clears() {
        let input = input();
        let mut targeting = CameraTargeting::default();
        let projection = Mat4::perspective_rh_gl(70_f32.to_radians(), 1., 0.05, 100.);
        targeting.capture(
            true,
            input.identity,
            input.camera_position,
            Mat4::IDENTITY,
            projection,
        );
        assert!(targeting.displayed_ray(input.identity).is_some());
        assert!(
            targeting
                .displayed_ray(FrameIdentity {
                    world: 2,
                    ..input.identity
                })
                .is_none()
        );
        let mut world = World {
            clips: VecDeque::new(),
            entity: None,
            entity_limit: 0.,
        };
        assert_eq!(
            targeting.pick(
                &PickInput {
                    enabled: false,
                    ..input
                },
                &mut world
            ),
            None
        );
        assert!(targeting.displayed_ray(input.identity).is_none());
    }

    #[test]
    fn turn_angles_follow_minecraft_yaw_pitch_without_forcing_a_turn_on_pick() {
        let turn = LookAngles::toward(DVec3::ZERO, DVec3::new(-1., -1., 0.)).unwrap();
        assert!((turn.yaw - 90.).abs() < 1e-5);
        assert!((turn.pitch - 45.).abs() < 1e-5);
        assert!(LookAngles::toward(DVec3::ZERO, DVec3::ZERO).is_none());
    }
}
