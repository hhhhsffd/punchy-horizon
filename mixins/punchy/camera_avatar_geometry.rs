fn moves_camera_avatar_geometry(
    game: &crate::game::Game<'_>,
    camera: glam::DVec3,
    partial: f64,
    vertices: &mut Vec<horizon_client::mesher::Vertex>,
) -> core::ops::ControlFlow<()> {
    crate::game::punchy_camera::avatar_geometry(game, camera, partial, vertices)
}
