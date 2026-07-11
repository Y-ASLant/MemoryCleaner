//! Shared layout metrics for window sizing and spacing.

pub const SECTION_GAP: f32 = 6.;
pub const DIALOG_PADDING_TOP: f32 = 16.;
pub const DIALOG_PADDING_HORIZONTAL: f32 = 16.;
pub const TITLE_BAR_H: f32 = 34.;
pub const CLEANUP_BUTTON_H: f32 = 48.;

const CARD_BORDER: f32 = 2.;
const PANEL_BORDER: f32 = 2.;
const MEMORY_HEADER_H: f32 = 20.;
const MEMORY_LINE_GAP: f32 = 4.;
const MEMORY_SUMMARY_H: f32 = 16.;
const SECTION_TITLE_H: f32 = 20.;
const HINT_H: f32 = 26.;
const CHECKBOX_ROW_H: f32 = 28.;
const CLEANUP_ROWS: f32 = 4.;
const CLEANUP_ROW_GAPS: f32 = SECTION_GAP * 3.;

pub fn memory_section_height() -> f32 {
    use crate::ui::memory_card::{MEMORY_CARD_PY, MEMORY_RING_SIZE};

    CARD_BORDER
        + MEMORY_CARD_PY * 2.
        + MEMORY_HEADER_H
        + MEMORY_LINE_GAP
        + MEMORY_RING_SIZE
        + MEMORY_LINE_GAP
        + MEMORY_SUMMARY_H
}

pub fn cleanup_section_height(content_padding: f32) -> f32 {
    let cleanup_areas = HINT_H + SECTION_GAP + CHECKBOX_ROW_H * CLEANUP_ROWS + CLEANUP_ROW_GAPS;

    PANEL_BORDER + content_padding * 2. + SECTION_TITLE_H + SECTION_GAP + cleanup_areas
}

pub fn expanded_window_height(content_padding: f32) -> f32 {
    TITLE_BAR_H
        + content_padding
        + memory_section_height()
        + SECTION_GAP
        + cleanup_section_height(content_padding)
        + SECTION_GAP
        + CLEANUP_BUTTON_H
        + content_padding
}
