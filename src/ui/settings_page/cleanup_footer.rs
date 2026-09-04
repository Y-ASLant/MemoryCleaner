use super::*;

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}…")
}

fn cleanup_step_text(app: &MemoryCleanerApp) -> String {
    if app.optimize_step.is_empty() {
        t!("button.cleanup_preparing").to_string()
    } else {
        app.optimize_step.clone()
    }
}

fn cleanup_result_text(app: &MemoryCleanerApp) -> String {
    truncate_chars(&app.optimize_status, BUTTON_STATUS_TRUNCATE_CHARS)
}

fn cleanup_button_is_danger(app: &MemoryCleanerApp) -> bool {
    !app.optimize_status.is_empty() && app.optimize_has_errors
}

fn cleanup_button_text_color(app: &MemoryCleanerApp, cx: &App) -> Hsla {
    let theme = cx.theme();
    if app.settings.memory_areas().is_empty() {
        return theme.muted_foreground.opacity(0.5);
    }
    if cleanup_button_is_danger(app) {
        return theme.danger_foreground;
    }
    theme.button_primary_foreground
}

fn render_cleanup_button_content(
    app: &MemoryCleanerApp,
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    let color = cleanup_button_text_color(app, cx);

    if app.is_optimizing {
        let line = truncate_chars(&cleanup_step_text(app), BUTTON_STATUS_TRUNCATE_CHARS);
        return h_flex()
            .w_full()
            .px_3()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                ProgressCircle::new("inline-optimize-progress")
                    .color(color)
                    .small()
                    .value(app.animated_optimize_percent()),
            )
            .child(
                Label::new(line)
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color)
                    .truncate(),
            )
            .into_any_element();
    }

    if !app.optimize_status.is_empty() {
        return Label::new(cleanup_result_text(app))
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(color)
            .truncate()
            .into_any_element();
    }

    Label::new(t!("button.cleanup").to_string())
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .into_any_element()
}

pub fn render_cleanup_footer(
    app: &MemoryCleanerApp,
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    let areas_empty = app.settings.memory_areas().is_empty();
    let mut button = Button::new("inline-optimize")
        .w_full()
        .flex_shrink_0()
        .h(px(CLEANUP_BUTTON_H))
        .disabled(areas_empty)
        .child(render_cleanup_button_content(app, cx))
        .on_click(cx.listener(|app, _, _, cx| {
            app.run_optimize(cx);
        }));

    button = if cleanup_button_is_danger(app) {
        button.danger()
    } else {
        button.primary()
    };

    if areas_empty {
        button.tooltip(t!("tooltip.select_areas").to_string())
    } else if app.is_optimizing {
        button.tooltip(cleanup_step_text(app))
    } else if app.optimize_status.is_empty() {
        button.tooltip(t!("tooltip.start_cleanup").to_string())
    } else {
        button.tooltip(app.optimize_status.clone())
    }
}
