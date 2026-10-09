use crate::memory::{
    MemorySection, MemoryStatus, SystemWorkingSet, WORKING_SET_HISTORY_SECS, WorkingSetHistory,
};
use crate::ui::layout::{MEMORY_HEADER_H, MEMORY_LINE_GAP, MEMORY_SUMMARY_H};
use rust_i18n::t;

use gpui_kit::component::chart::LineChart;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, Size, h_flex, label::Label, progress::ProgressCircle,
    v_flex,
};
use gpui_kit::*;

pub const MEMORY_RING_SIZE: f32 = 108.;
pub const WORKING_SET_PLOT_HEIGHT: f32 = 96.;

const SYSTEM_WORKING_SET_ICON: IconName = IconName::Inbox;

/// ProgressCircle applies a 0.75 scale to custom sizes internally.
const PROGRESS_CIRCLE_LAYOUT_SIZE: Pixels = px(MEMORY_RING_SIZE / 0.75);

/// 卡片容器上下内边距（app 中 GroupBox 内 v_flex 使用）。
pub const MEMORY_CARD_PY: f32 = 2.;

fn usage_color(percent: f32, cx: &App) -> Hsla {
    let theme = cx.theme();
    if percent >= 90.0 {
        theme.danger
    } else if percent >= 70.0 {
        theme.warning
    } else {
        theme.chart_2
    }
}

fn render_usage_ring(
    id: &'static str,
    section: &MemorySection,
    animated_percent: f32,
    cx: &App,
) -> impl IntoElement {
    let unavailable = section.is_unavailable();
    let (display_percent, color, label_color, label_text) = if unavailable {
        (
            0.0,
            cx.theme().muted_foreground,
            cx.theme().muted_foreground,
            "—".to_string(),
        )
    } else {
        (
            animated_percent,
            usage_color(animated_percent, cx),
            cx.theme().foreground,
            format!("{}%", animated_percent.round() as u32),
        )
    };

    ProgressCircle::new(id)
        .with_size(Size::Size(PROGRESS_CIRCLE_LAYOUT_SIZE))
        .value(display_percent)
        .color(color)
        .child(
            Label::new(label_text)
                .text_lg()
                .font_weight(FontWeight::BOLD)
                .text_color(label_color),
        )
}

pub fn render_system_working_set_card(history: &WorkingSetHistory, cx: &App) -> impl IntoElement {
    let working_set = history.current();
    let (header, summary) = match working_set {
        Some(working_set) => (working_set.header(), working_set.summary()),
        None => (
            t!("memory.system_working_set").to_string(),
            t!("memory.unavailable").to_string(),
        ),
    };
    let data: Vec<(SharedString, SystemWorkingSet)> = history
        .samples()
        .iter()
        .map(|sample| {
            let seconds = history.elapsed_seconds(sample.at);
            (format!("{seconds}s").into(), sample.working_set)
        })
        .collect();
    let plot = if data.len() >= 2 {
        let upper = data
            .iter()
            .map(|(_, value)| value.current as f64 / (1024. * 1024.))
            .fold(1.0_f64, f64::max)
            * 1.1;
        LineChart::new(data)
            .id("system-working-set-trend")
            .x(|sample: &(SharedString, SystemWorkingSet)| sample.0.clone())
            .y(|sample| sample.1.current as f64 / (1024. * 1024.))
            .linear()
            .stroke(cx.theme().chart_2)
            .appear(false)
            .point_count(WORKING_SET_HISTORY_SECS as usize + 1)
            .x_tick_count(3)
            .y_axis(true)
            .y_tick_count(3)
            .y_domain(0., upper)
            .y_tick_format(|value| format!("{value:.1} MB"))
            .grid_dashed(false)
            .name(t!("memory.system_working_set").to_string())
            .tooltip_value(|sample, _| {
                format!(
                    "{} · {}",
                    MemoryStatus::format_bytes(sample.1.current),
                    sample.1.percent_label(),
                )
                .into()
            })
            .into_any_element()
    } else {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                Label::new(if working_set.is_some() {
                    t!("memory.trend_waiting").to_string()
                } else {
                    t!("memory.trend_unavailable").to_string()
                })
                .text_xs()
                .text_color(cx.theme().muted_foreground),
            )
            .into_any_element()
    };

    v_flex()
        .w_full()
        .py(px(MEMORY_CARD_PY))
        .gap(px(MEMORY_LINE_GAP))
        .child(
            h_flex()
                .w_full()
                .h(px(MEMORY_HEADER_H))
                .items_center()
                .justify_center()
                .gap(px(MEMORY_LINE_GAP))
                .child(Icon::new(SYSTEM_WORKING_SET_ICON).small())
                .child(
                    Label::new(header)
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD),
                ),
        )
        .child(
            Label::new(summary)
                .text_xs()
                .line_height(px(MEMORY_SUMMARY_H))
                .text_color(cx.theme().foreground.opacity(0.82)),
        )
        .child(div().w_full().h(px(WORKING_SET_PLOT_HEIGHT)).child(plot))
}

pub fn render_memory_card(
    section: &MemorySection,
    id: &'static str,
    is_physical: bool,
    animated_percent: f32,
    animated_used: u64,
    animated_avail: u64,
    cx: &App,
) -> impl IntoElement {
    let unavailable = section.is_unavailable();

    let icon = if is_physical {
        IconName::Cpu
    } else {
        IconName::HardDrive
    };

    let ring = render_usage_ring(id, section, animated_percent, cx);
    let summary = if unavailable {
        t!("memory.unavailable").to_string()
    } else {
        t!(
            "memory.used_avail",
            used = MemoryStatus::format_bytes(animated_used),
            avail = MemoryStatus::format_bytes(animated_avail),
        )
        .to_string()
    };
    let muted = cx.theme().foreground.opacity(0.82);

    v_flex()
        .w_full()
        .items_center()
        .gap(px(MEMORY_LINE_GAP))
        .child(
            h_flex()
                .items_center()
                .gap(px(MEMORY_LINE_GAP))
                .child(Icon::new(icon).small())
                .child(
                    Label::new(section.header())
                        .text_sm()
                        .line_height(px(MEMORY_HEADER_H))
                        .font_weight(FontWeight::SEMIBOLD),
                ),
        )
        .child(ring)
        .child(
            Label::new(summary)
                .text_xs()
                .line_height(px(MEMORY_SUMMARY_H))
                .text_color(if unavailable {
                    cx.theme().warning
                } else {
                    muted
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::SYSTEM_WORKING_SET_ICON;
    use gpui_kit::AssetSource;
    use gpui_kit::assets::IconNamed;

    #[test]
    fn system_working_set_icon_is_in_registered_assets() {
        let path = SYSTEM_WORKING_SET_ICON.path();
        let svg = gpui_kit::assets::Assets
            .load(path.as_ref())
            .expect("load system working-set icon from default assets")
            .expect("system working-set icon exists");
        assert!(std::str::from_utf8(svg.as_ref()).unwrap().contains("<svg"));
    }
}
