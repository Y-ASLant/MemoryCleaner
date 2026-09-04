use super::*;

/// 创建内存卡片的 GroupBox 容器
fn memory_group_box(
    id: &'static str,
    child: impl IntoElement,
) -> gpui_kit::component::group_box::GroupBox {
    use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants};

    GroupBox::new()
        .id(id)
        .outline()
        .w_full()
        .p_0()
        .content_style(StyleRefinement::default().p_2())
        .child(child)
}

impl Render for MemoryCleanerApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::ui::memory_card::render_memory_card;
        use crate::ui::settings_page::{render_cleanup_footer, render_settings_content};
        use crate::ui::title_bar::render_title_bar;
        use gpui_kit::component::{h_flex, v_flex};
        use gpui_kit::prelude::FluentBuilder;

        // Tick animations — if still running, schedule next frame.
        if self.tick_animations(window) {
            window.request_animation_frame();
        }
        let bg = cx.theme().background;
        let settings_progress = self.settings_expand_progress();
        let settings_visual_progress = ease_out_cubic(settings_progress);
        let settings_reveal_h = settings_reveal_height() * settings_progress;

        let physical_card = memory_group_box(
            "physical-memory-card",
            v_flex()
                .w_full()
                .items_center()
                .py(px(crate::ui::memory_card::MEMORY_CARD_PY))
                .child(render_memory_card(
                    &self.physical,
                    "physical-memory",
                    true,
                    self.anim_physical.current,
                    self.animated_used_phys(),
                    self.animated_avail_phys(),
                    cx,
                )),
        );

        let virtual_card = memory_group_box(
            "virtual-memory-card",
            v_flex()
                .w_full()
                .items_center()
                .py(px(crate::ui::memory_card::MEMORY_CARD_PY))
                .child(render_memory_card(
                    &self.virtual_mem,
                    "virtual-memory",
                    false,
                    self.anim_virtual.current,
                    self.animated_used_virt(),
                    self.animated_avail_virt(),
                    cx,
                )),
        );

        let memory_row = h_flex()
            .w_full()
            .flex_shrink_0()
            .gap(px(SECTION_GAP))
            .child(div().flex_1().min_w_0().child(physical_card))
            .child(div().flex_1().min_w_0().child(virtual_card))
            .into_any_element();

        div()
            .relative()
            .w_full()
            .h_full()
            .child(
                div().w_full().h_full().overflow_hidden().child(
                    v_flex()
                        .w_full()
                        .h_full()
                        .overflow_hidden()
                        .bg(bg)
                        .child(render_title_bar(self, window, cx))
                        .child({
                            let body = v_flex()
                                .w_full()
                                .flex_shrink_0()
                                .px(px(CONTENT_PADDING))
                                .pt(px(CONTENT_PADDING))
                                .child(memory_row)
                                .when(self.settings_panel_visible(), |body| {
                                    body.child(
                                        div()
                                            .w_full()
                                            .h(px(settings_reveal_h))
                                            .overflow_hidden()
                                            .child(
                                                div()
                                                    .w_full()
                                                    .pt(px(SECTION_GAP))
                                                    .opacity(settings_visual_progress)
                                                    .child(render_settings_content(self, cx)),
                                            ),
                                    )
                                });

                            v_flex()
                                .w_full()
                                .flex_shrink_0()
                                .min_h_0()
                                .overflow_hidden()
                                .gap(px(SECTION_GAP))
                                .child(body)
                                .child(
                                    div()
                                        .w_full()
                                        .flex_shrink_0()
                                        .px(px(CONTENT_PADDING))
                                        .pb(px(CONTENT_PADDING))
                                        .child(render_cleanup_footer(self, cx)),
                                )
                        }),
                ),
            )
            .children(gpui_kit::component::Root::render_dialog_layer(window, cx))
    }
}
