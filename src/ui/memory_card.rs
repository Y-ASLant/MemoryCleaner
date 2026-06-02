use gpui::*;
use gpui_component::{
    chart::PieChart,
    h_flex,
    label::Label,
    v_flex, ActiveTheme, Icon, IconName, Sizable,
};

use crate::app::MemoryCleanerApp;
use crate::memory::MemorySection;

pub fn render_memory_card(
    section: &MemorySection,
    _id: &'static str,
    is_physical: bool,
    _window: &mut Window,
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    let theme = cx.theme();
    let used_percent = section.used_percent.clamp(0.0, 100.0);
    let free_percent = (100.0 - used_percent).max(0.0);
    let used_color = usage_color(used_percent, theme);
    let free_color = theme.chart_1;

    let icon = if is_physical {
        IconName::Cpu
    } else {
        IconName::HardDrive
    };

    #[derive(Clone)]
    struct Slice {
        value: f32,
        color: Hsla,
    }

    let slices = vec![
        Slice {
            value: used_percent,
            color: used_color,
        },
        Slice {
            value: free_percent,
            color: free_color,
        },
    ];

    let pie = PieChart::new(slices)
        .value(|s| s.value)
        .outer_radius(55.)
        .inner_radius(35.)
        .pad_angle(2. / 100.)
        .color(|s| s.color);

    let percent_text = format!("{}%", used_percent as u32);

    v_flex()
        .w_full()
        .items_center()
        .gap_1()
        .child(
            h_flex()
                .items_center()
                .gap_1()
                .child(Icon::new(icon).small())
                .child(
                    Label::new(section.header.clone())
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD),
                ),
        )
        .child(
            div()
                .relative()
                .w(px(110.))
                .h(px(110.))
                .flex_shrink_0()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Label::new(percent_text)
                                .text_lg()
                                .font_weight(FontWeight::BOLD),
                        ),
                )
                .child(pie),
        )
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .child(
                    Label::new(section.used_label.clone())
                        .text_xs()
                        .text_color(cx.theme().foreground.opacity(0.82)),
                )
                .child(
                    Label::new(section.free_label.clone())
                        .text_xs()
                        .text_color(cx.theme().foreground.opacity(0.82)),
                ),
        )
}

fn usage_color(percent: f32, theme: &gpui_component::Theme) -> Hsla {
    if percent >= 90.0 {
        theme.danger
    } else if percent >= 70.0 {
        theme.warning
    } else {
        theme.chart_2
    }
}
