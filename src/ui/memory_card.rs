use std::{cell::Cell, f32::consts::TAU, time::Duration};

use gpui::{canvas, Animation, AnimationExt, *};
use gpui_component::{
    animation::ease_in_out_cubic,
    h_flex,
    label::Label,
    plot::shape::{Arc, ArcData},
    v_flex, ActiveTheme, Icon, IconName, Sizable,
};
use smol::Timer;

use crate::app::MemoryCleanerApp;
use crate::memory::MemorySection;

const RING_ANIM_DURATION: Duration = Duration::from_millis(450);
const OUTER_RADIUS: f32 = 55.;
const INNER_RADIUS: f32 = 35.;
const SLICE_PAD_ANGLE: f32 = 0.02;

#[derive(Clone, Copy)]
struct RingColors {
    chart_1: Hsla,
    chart_2: Hsla,
    warning: Hsla,
    danger: Hsla,
}

impl RingColors {
    fn from_theme(theme: &gpui_component::Theme) -> Self {
        Self {
            chart_1: theme.chart_1,
            chart_2: theme.chart_2,
            warning: theme.warning,
            danger: theme.danger,
        }
    }
}

struct RingAnimState {
    value: f32,
    target: Cell<f32>,
}

impl RingAnimState {
    fn new(value: f32) -> Self {
        Self {
            value,
            target: Cell::new(value),
        }
    }

    fn target(&self) -> f32 {
        self.target.get()
    }

    fn set_target(&self, value: f32) {
        self.target.set(value);
    }
}

fn usage_color(percent: f32, colors: RingColors) -> Hsla {
    if percent >= 90.0 {
        colors.danger
    } else if percent >= 70.0 {
        colors.warning
    } else {
        colors.chart_2
    }
}

struct DonutPrepaint {
    used_percent: f32,
    used_color: Hsla,
    free_color: Hsla,
    bounds: Bounds<Pixels>,
}

fn render_donut_canvas(used_percent: f32, colors: RingColors) -> impl IntoElement {
    let used_percent = used_percent.clamp(0.0, 100.0);
    let used_color = usage_color(used_percent, colors);
    let free_color = colors.chart_1;

    canvas(
        move |bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| DonutPrepaint {
            used_percent,
            used_color,
            free_color,
            bounds,
        },
        move |_bounds, prepaint, window: &mut Window, _cx: &mut App| {
            let used_angle = (prepaint.used_percent / 100.0) * TAU;
            let arc = Arc::new()
                .inner_radius(INNER_RADIUS)
                .outer_radius(OUTER_RADIUS);

            arc.paint(
                &ArcData {
                    data: &(),
                    index: 0,
                    value: 100.,
                    start_angle: 0.,
                    end_angle: TAU,
                    pad_angle: 0.,
                },
                prepaint.free_color,
                None,
                None,
                &prepaint.bounds,
                window,
            );

            if prepaint.used_percent > 0.01 {
                arc.paint(
                    &ArcData {
                        data: &(),
                        index: 1,
                        value: prepaint.used_percent,
                        start_angle: 0.,
                        end_angle: used_angle.max(SLICE_PAD_ANGLE),
                        pad_angle: SLICE_PAD_ANGLE,
                    },
                    prepaint.used_color,
                    None,
                    None,
                    &prepaint.bounds,
                    window,
                );
            }
        },
    )
    .absolute()
    .size_full()
}

fn render_ring(used_percent: f32, colors: RingColors) -> impl IntoElement {
    let percent_text = format!("{}%", used_percent.round() as u32);

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
        .child(render_donut_canvas(used_percent, colors))
}

fn schedule_ring_sync(state: Entity<RingAnimState>, cx: &mut Context<MemoryCleanerApp>) {
    cx.spawn(async move |_, async_cx| {
        Timer::after(RING_ANIM_DURATION).await;
        _ = state.update(async_cx, |this, _| {
            this.value = this.target();
        });
    })
    .detach();
}

pub fn render_memory_card(
    section: &MemorySection,
    id: &'static str,
    is_physical: bool,
    window: &mut Window,
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    let colors = RingColors::from_theme(cx.theme());
    let target_percent = section.used_percent.clamp(0.0, 100.0);

    let state = window.use_keyed_state(id, cx, |_, _| RingAnimState::new(target_percent));
    let current = state.read(cx).value;
    let anim_target = state.read(cx).target();

    if (anim_target - target_percent).abs() > 0.01 {
        state.read(cx).set_target(target_percent);
        schedule_ring_sync(state.clone(), cx);
    }

    let needs_animation = (current - target_percent).abs() > 0.01;

    let icon = if is_physical {
        IconName::Cpu
    } else {
        IconName::HardDrive
    };

    let ring = if needs_animation {
        let from = current;
        let to = state.read(cx).target();

        div()
            .id(id)
            .with_animation(
                format!("{id}-ring-anim"),
                Animation::new(RING_ANIM_DURATION).with_easing(ease_in_out_cubic),
                move |this, delta| {
                    let animated = from + (to - from) * delta;
                    this.child(render_ring(animated, colors))
                },
            )
            .into_any_element()
    } else {
        render_ring(target_percent, colors).into_any_element()
    };

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
        .child(ring)
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
