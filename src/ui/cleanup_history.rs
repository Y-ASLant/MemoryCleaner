use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, label::Label, v_flex};
use gpui_kit::*;
use rust_i18n::t;

use crate::{
    app::MemoryCleanerApp,
    memory::MemoryStatus,
    settings::{CleanupHistoryEntry, CleanupHistorySource},
};

const HISTORY_CLOCK_SVG: &[u8] = br#"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="9"/>
  <path d="M12 7v5l3 2"/>
</svg>
"#;

pub fn history_icon() -> Icon {
    Icon::default().data(HISTORY_CLOCK_SVG).small()
}

fn source_label(source: CleanupHistorySource) -> String {
    match source {
        CleanupHistorySource::Manual => t!("cleanup.history_source_manual").to_string(),
        CleanupHistorySource::LowMemoryNotification => {
            t!("cleanup.history_source_low_memory").to_string()
        }
        CleanupHistorySource::Threshold => t!("cleanup.history_source_threshold").to_string(),
    }
}

fn age(entry: &CleanupHistoryEntry) -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(entry.completed_at_unix_secs);
    if elapsed < 60 {
        return t!("cleanup.history_just_now").to_string();
    }

    let (value, unit) = if elapsed < 60 * 60 {
        (elapsed / 60, "cleanup.history_minutes_ago")
    } else if elapsed < 24 * 60 * 60 {
        (elapsed / (60 * 60), "cleanup.history_hours_ago")
    } else {
        (elapsed / (24 * 60 * 60), "cleanup.history_days_ago")
    };
    t!(unit, count = value.to_string()).to_string()
}

fn available_delta(before: u64, after: u64) -> String {
    if after >= before {
        format!("+{}", MemoryStatus::format_bytes(after - before))
    } else {
        format!("-{}", MemoryStatus::format_bytes(before - after))
    }
}

fn detail(entry: &CleanupHistoryEntry) -> String {
    t!(
        "cleanup.history_detail",
        selected = entry.selected_areas.count_ones().to_string(),
        completed = entry.completed_count.to_string(),
        failed = entry.failed_count.to_string(),
        before = entry.memory_load_before.to_string(),
        after = entry.memory_load_after.to_string(),
        available = available_delta(entry.available_before, entry.available_after),
        duration = format!("{:.1}s", entry.duration_millis as f64 / 1_000.0),
    )
    .to_string()
}

fn history_entry(index: usize, entry: &CleanupHistoryEntry, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .id(("cleanup-history-entry", index))
        .w_full()
        .p_2()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.muted_foreground.opacity(0.05))
        .child(
            h_flex()
                .w_full()
                .items_start()
                .gap_2()
                .child(
                    div()
                        .pt(px(2.))
                        .text_color(theme.muted_foreground)
                        .child(history_icon()),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(
                            Label::new(format!("{} · {}", source_label(entry.source), age(entry)))
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground),
                        )
                        .child(
                            Label::new(detail(entry))
                                .text_xs()
                                .text_color(theme.muted_foreground),
                        ),
                ),
        )
}

pub fn render_cleanup_history_dialog(
    weak: WeakEntity<MemoryCleanerApp>,
    cx: &App,
) -> impl IntoElement {
    let history = weak
        .upgrade()
        .map(|app| app.read(cx).settings.cleanup_history.clone())
        .unwrap_or_default();
    let theme = cx.theme();

    if history.is_empty() {
        return div()
            .w_full()
            .py_6()
            .flex()
            .justify_center()
            .child(
                Label::new(t!("cleanup.history_empty").to_string())
                    .text_sm()
                    .text_color(theme.muted_foreground),
            )
            .into_any_element();
    }

    div()
        .id("cleanup-history-list")
        .w_full()
        .child(
            v_flex().w_full().gap_2().children(
                history
                    .iter()
                    .rev()
                    .enumerate()
                    .map(|(index, entry)| history_entry(index, entry, cx)),
            ),
        )
        .into_any_element()
}
