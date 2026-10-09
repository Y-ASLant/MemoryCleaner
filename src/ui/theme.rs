use gpui_kit::component::{ActiveTheme, Theme, ThemeMode, button::Button, tooltip::Tooltip};
use gpui_kit::{App, InteractiveElement, SharedString, Styled, Window, px};

/// Use a styled tooltip when the component theme disables shadows.
pub fn button_tooltip(button: Button, text: impl Into<SharedString>, cx: &App) -> Button {
    let text = text.into();
    if cx.theme().shadow {
        return button.tooltip(text);
    }
    let mut button = button;
    button.interactivity().tooltip(move |window, cx| {
        Tooltip::new(text.clone())
            .shadow(Vec::new())
            .build(window, cx)
    });
    button
}

fn apply_platform_style(theme: &mut Theme, windows_11_or_later: bool) {
    if !windows_11_or_later {
        theme.radius = px(0.);
        theme.radius_lg = px(0.);
        theme.shadow = false;
    }
}

/// Initialize the light theme and apply Win10 square-corner chrome when needed.
pub fn init_light_theme(window: &mut Window, cx: &mut App) {
    Theme::change(ThemeMode::Light, None, cx);
    let windows_11_or_later = crate::win32::os::is_windows_11_or_later();
    Theme::update(cx, |theme| apply_platform_style(theme, windows_11_or_later));
    window.refresh();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_10_uses_square_corners_and_disables_theme_shadows() {
        let mut theme = Theme::default();
        theme.radius = px(6.);
        theme.radius_lg = px(8.);
        theme.shadow = true;
        apply_platform_style(&mut theme, false);
        assert_eq!(theme.radius, px(0.));
        assert_eq!(theme.radius_lg, px(0.));
        assert!(!theme.shadow);
    }

    #[test]
    fn windows_11_preserves_registered_theme_styles() {
        let mut theme = Theme::default();
        theme.radius = px(6.);
        theme.radius_lg = px(8.);
        theme.shadow = true;
        apply_platform_style(&mut theme, true);
        assert_eq!(theme.radius, px(6.));
        assert_eq!(theme.radius_lg, px(8.));
        assert!(theme.shadow);
    }
}
