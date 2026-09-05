use super::*;

fn memory_area_checkbox(
    id: &'static str,
    area: MemoryAreas,
    app: &MemoryCleanerApp,
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    let checked = app.settings.memory_areas().contains(area);
    let mut checkbox = Checkbox::new(id)
        .label(area.label())
        .text_sm()
        .checked(checked)
        .on_click(cx.listener(move |app, enabled, _, cx| {
            app.set_memory_area(area, *enabled, cx);
        }));

    if app.is_optimizing {
        checkbox = checkbox.disabled(true);
    }

    div().flex_1().min_w_0().child(checkbox)
}

fn cleanup_area_row(
    left: (&'static str, MemoryAreas),
    right: Option<(&'static str, MemoryAreas)>,
    app: &MemoryCleanerApp,
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .h(px(CLEANUP_AREA_ROW_H))
        .gap_4()
        .child(memory_area_checkbox(left.0, left.1, app, cx))
        .when_some(right, |row, (id, area)| {
            row.child(memory_area_checkbox(id, area, app, cx))
        })
}

pub(super) fn render_cleanup_areas(
    app: &MemoryCleanerApp,
    muted: Hsla,
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap(px(ROW_GAP))
        .child(
            div()
                .w_full()
                .h(px(CLEANUP_AREAS_HINT_H))
                .rounded(cx.theme().radius)
                .px_2()
                .py_1()
                .bg(muted.opacity(0.12))
                .child(
                    Label::new(t!("settings.cleanup_areas_hint").to_string())
                        .text_xs()
                        .text_color(muted),
                ),
        )
        .child(cleanup_area_row(
            ("area-standby", MemoryAreas::STANDBY_LIST),
            Some(("area-standby-low", MemoryAreas::STANDBY_LIST_LOW_PRIORITY)),
            app,
            cx,
        ))
        .child(cleanup_area_row(
            ("area-working-set", MemoryAreas::WORKING_SET),
            Some(("area-system-cache", MemoryAreas::SYSTEM_FILE_CACHE)),
            app,
            cx,
        ))
        .child(cleanup_area_row(
            ("area-modified-page", MemoryAreas::MODIFIED_PAGE_LIST),
            Some(("area-combined", MemoryAreas::COMBINED_PAGE_LIST)),
            app,
            cx,
        ))
        .child(cleanup_area_row(
            ("area-modified-file", MemoryAreas::MODIFIED_FILE_CACHE),
            None,
            app,
            cx,
        ))
}
