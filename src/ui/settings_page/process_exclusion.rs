use super::*;

fn process_picker_detail(entry: &ProcessPickerEntry) -> String {
    let memory = entry
        .memory_display()
        .unwrap_or_else(|| t!("settings.process_exclusion_picker_unknown").to_string());
    if entry.instance_count > 1 {
        t!(
            "settings.process_exclusion_picker_detail",
            count = entry.instance_count,
            memory = memory
        )
        .to_string()
    } else {
        memory
    }
}

fn render_process_exclusion_tag(
    index: usize,
    name: &str,
    app: &MemoryCleanerApp,
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    let name = name.to_string();
    let remove_id = SharedString::from(format!("process-exclusion-remove-{index}"));
    let muted = cx.theme().muted_foreground;

    Tag::secondary()
        .outline()
        .small()
        .rounded(cx.theme().radius)
        .child(h_flex().items_center().gap_1().child(name.clone()).child({
            let name = name.clone();
            let mut button = Button::new(remove_id)
                .ghost()
                .xsmall()
                .flex_shrink_0()
                .tooltip(t!("settings.process_exclusion_remove").to_string())
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.remove_excluded_process(&name, cx);
                }))
                .icon(Icon::new(IconName::CircleX).xsmall().text_color(muted));
            if app.is_optimizing {
                button = button.disabled(true);
            }
            button
        }))
}

fn render_process_exclusion_list(
    app: &MemoryCleanerApp,
    excluded: &[String],
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    let list_height = process_exclusion_list_max_height();
    let border = cx.theme().border;
    let muted_fg = cx.theme().muted_foreground;
    let empty_fg = cx.theme().foreground.opacity(0.55);

    let content = if excluded.is_empty() {
        div()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                Label::new(t!("settings.process_exclusion_empty").to_string())
                    .text_sm()
                    .text_color(empty_fg),
            )
            .into_any_element()
    } else {
        excluded
            .iter()
            .enumerate()
            .fold(
                h_flex().w_full().flex_wrap().gap(px(EXCLUSION_TAG_GAP)),
                |tags, (index, name)| {
                    tags.child(render_process_exclusion_tag(index, name, app, cx))
                },
            )
            .into_any_element()
    };

    div()
        .id("process-exclusion-list")
        .w_full()
        .h(px(list_height))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(border)
        .bg(muted_fg.opacity(0.06))
        .p(px(EXCLUSION_LIST_PADDING))
        .overflow_y_scrollbar()
        .child(content)
}

pub(super) fn render_process_exclusion(
    app: &MemoryCleanerApp,
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    let weak = cx.weak_entity();
    let excluded = app.settings.excluded_processes.clone();
    let selector_h = px(EXCLUSION_SELECTOR_H);
    let selector_w = px(process_exclusion_selector_width(
        MAIN_WINDOW_WIDTH,
        MAIN_CONTENT_PADDING,
    ));

    v_flex()
        .w_full()
        .gap(px(crate::ui::layout::EXCLUSION_FOOTER_GAP))
        .child(div().w_full().child({
            let weak = weak.clone();
            Button::new("process-exclusion-select")
                .outline()
                .small()
                .w_full()
                .h(selector_h)
                .when(app.is_optimizing, |this| this.disabled(true))
                .label(t!("settings.process_exclusion_select").to_string())
                .dropdown_caret(true)
                .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, cx| {
                    let mut available = Vec::new();
                    let _ = weak.update(cx, |app, _| {
                        available = list_processes_for_exclusion_picker(
                            PROCESS_BASE_NAME,
                            &app.settings.excluded_processes,
                        );
                    });

                    let menu = available.iter().fold(menu, |menu, entry| {
                        let name = entry.name.clone();
                        let detail = process_picker_detail(entry);
                        let weak = weak.clone();
                        let label = name.clone();
                        let detail_label = detail.clone();
                        menu.item(
                            PopupMenuItem::element(move |_, cx| {
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(Label::new(label.clone()).text_sm().truncate())
                                    .child(
                                        Label::new(detail_label.clone())
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .flex_shrink_0(),
                                    )
                            })
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.add_excluded_process_by_name(&name, cx);
                                });
                            }),
                        )
                    });
                    menu.scrollable(true)
                        .max_h(px(PROCESS_PICKER_MENU_MAX_H))
                        .min_w(selector_w)
                        .max_w(selector_w)
                })
        }))
        .child(render_process_exclusion_list(app, &excluded, cx))
}
