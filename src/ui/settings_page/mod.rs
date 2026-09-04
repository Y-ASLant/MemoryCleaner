use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    kbd::Kbd,
    label::Label,
    menu::{DropdownMenu, PopupMenuItem},
    progress::ProgressCircle,
    scroll::ScrollableElement as _,
    switch::Switch,
    tag::Tag,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use crate::app::MemoryCleanerApp;
use crate::optimize::MemoryAreas;
use crate::ui::layout::{
    CLEANUP_AREA_ROW_H, CLEANUP_AREAS_HINT_H, CLEANUP_BUTTON_H, EXCLUSION_LIST_PADDING,
    EXCLUSION_SELECTOR_H, EXCLUSION_TAG_GAP, MAIN_CONTENT_PADDING, MAIN_WINDOW_WIDTH,
    PROCESS_PICKER_MENU_MAX_H, SECTION_GAP, SETTINGS_CARD_TITLE_H, cleanup_areas_card_height,
    process_exclusion_card_height, process_exclusion_list_max_height,
    process_exclusion_selector_width,
};
use crate::version::PROCESS_BASE_NAME;
use crate::win32::hotkey::HotkeyBinding;
use crate::win32::process::{ProcessPickerEntry, list_processes_for_exclusion_picker};

mod cleanup_areas;
mod cleanup_footer;
mod process_exclusion;
mod window_behavior;

use cleanup_areas::render_cleanup_areas;
use process_exclusion::render_process_exclusion;

pub use cleanup_footer::render_cleanup_footer;
#[cfg(test)]
pub(crate) use window_behavior::auto_cleanup_description;
pub use window_behavior::render_window_behavior_dialog;

const ROW_GAP: f32 = 6.;
const BUTTON_STATUS_TRUNCATE_CHARS: usize = 24;

fn panel_section_title(icon: IconName, label: String) -> impl IntoElement {
    h_flex()
        .w_full()
        .h(px(SETTINGS_CARD_TITLE_H))
        .items_center()
        .gap_2()
        .child(Icon::new(icon).small())
        .child(
            Label::new(label)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD),
        )
}

pub fn render_settings_content(
    app: &MemoryCleanerApp,
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    let theme = cx.theme();
    render_settings_details(app, theme.muted_foreground, cx)
}

fn compact_group_box_style() -> StyleRefinement {
    StyleRefinement::default().p_2().gap_2()
}

fn render_settings_card(
    id: &'static str,
    title_icon: IconName,
    title: String,
    height: f32,
    content: impl IntoElement,
) -> impl IntoElement {
    GroupBox::new()
        .id(id)
        .outline()
        .w_full()
        .h(px(height))
        .p_0()
        .content_style(compact_group_box_style())
        .child(
            v_flex()
                .w_full()
                .gap(px(SECTION_GAP))
                .child(panel_section_title(title_icon, title))
                .child(content),
        )
}

fn render_settings_details(
    app: &MemoryCleanerApp,
    muted: Hsla,
    cx: &mut Context<MemoryCleanerApp>,
) -> impl IntoElement {
    v_flex()
        .id("settings-details-panel")
        .w_full()
        .flex_shrink_0()
        .gap(px(SECTION_GAP))
        .child(render_settings_card(
            "process-exclusion-card",
            IconName::CircleX,
            t!("settings.process_exclusion").to_string(),
            process_exclusion_card_height(),
            render_process_exclusion(app, cx),
        ))
        .child(render_settings_card(
            "cleanup-areas-card",
            IconName::Settings,
            t!("settings.cleanup_areas").to_string(),
            cleanup_areas_card_height(),
            render_cleanup_areas(app, muted, cx),
        ))
}
