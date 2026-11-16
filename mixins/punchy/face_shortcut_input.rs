fn face_shortcut_input(
    game: &crate::game::Game<'_>,
    event: &winit::event::WindowEvent,
) -> core::ops::ControlFlow<()> {
    if let winit::event::WindowEvent::KeyboardInput { event, .. } = event
        && event.state == winit::event::ElementState::Pressed
        && let winit::keyboard::PhysicalKey::Code(code) = event.physical_key
        && let Some(key) = horizon_client::keybinds::key_name(code)
        && crate::game::punchy_face_ui::shortcut(game, key, event.repeat)
    {
        return core::ops::ControlFlow::Break(());
    }
    core::ops::ControlFlow::Continue(())
}
