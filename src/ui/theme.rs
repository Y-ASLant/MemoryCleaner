use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Window, px};

/// Initialize the light theme and apply Win10 square-corner chrome when needed.
pub fn init_light_theme(window: &mut Window, cx: &mut App) {
    Theme::change(ThemeMode::Light, None, cx);
    if !crate::win32::os::is_windows_11_or_later() {
        let theme = Theme::global_mut(cx);
        theme.radius = px(0.);
        theme.radius_lg = px(0.);
        theme.shadow = false;
    }
    window.refresh();
}
