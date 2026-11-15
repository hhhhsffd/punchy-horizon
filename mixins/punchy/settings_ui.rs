// Use the actual scrolled, clipped native layout, including Spatial GUI's
// already inverse-mapped cursor. Never intercept binding capture.
use super::{Act, OptionsUi, Page, Settings, WKind, ui};
impl OptionsUi {
    pub fn punchy_setting_at(
        &mut self,
        settings: &Settings,
        size: (f32, f32),
        cursor: (f32, f32),
    ) -> Option<usize> {
        let Page::NativeMod(owner) = self.page() else {
            return None;
        };
        if settings.mods.installed.get(owner)?.id != "punchy"
            || self.selected_key.is_some()
            || self.selected_native_key.is_some()
            || self.scrollbar_drag_y.is_some()
            || self.resource_error.is_some()
        {
            return None;
        }
        let (widgets, top, bottom, _) = self.layout(settings, size.0, size.1);
        widgets.into_iter().find_map(|w| match w.kind {
            WKind::Button {
                active: true,
                act: Act::NativeButton(index, button),
                ..
            } if index == owner
                && ui::hovered(cursor, w.x, w.y, w.w, w.h)
                && !(w.list && (cursor.1 < top || cursor.1 >= bottom)) =>
            {
                Some(button)
            }
            _ => None,
        })
    }
    pub fn punchy_binding_active(&self) -> bool {
        self.selected_key.is_some()
            || self.selected_native_key.is_some()
            || self.scrollbar_drag_y.is_some()
            || self.resource_error.is_some()
    }
}
