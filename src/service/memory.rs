use anyhow::Result;
use rust_i18n::t;

use crate::memory::{MemorySection, MemoryStatus};

pub fn query_sections(show_virtual: bool) -> Result<(MemorySection, Option<MemorySection>)> {
    let status = MemoryStatus::query()?;

    let physical = MemorySection {
        title: t!("memory.physical").to_string(),
        total: status.total_phys,
        used: status.used_phys(),
        avail: status.avail_phys,
        used_percent: status.memory_load as f32,
    };

    let virtual_mem = if show_virtual {
        let virt_used = status
            .total_page_file
            .saturating_sub(status.avail_page_file);
        let virt_percent = if status.total_page_file > 0 {
            (virt_used as f64 / status.total_page_file as f64 * 100.0).round() as u32
        } else {
            0
        };
        Some(MemorySection {
            title: t!("memory.virtual").to_string(),
            total: status.total_page_file,
            used: virt_used,
            avail: status.avail_page_file,
            used_percent: virt_percent as f32,
        })
    } else {
        None
    };

    Ok((physical, virtual_mem))
}

pub fn unavailable_sections(show_virtual: bool) -> (MemorySection, Option<MemorySection>) {
    (
        MemorySection::unavailable(&t!("memory.physical")),
        if show_virtual {
            Some(MemorySection::unavailable(&t!("memory.virtual")))
        } else {
            None
        },
    )
}

/// Load physical/virtual sections, falling back to unavailable placeholders on error.
pub fn initial_sections(show_virtual: bool) -> (MemorySection, Option<MemorySection>) {
    query_sections(show_virtual).unwrap_or_else(|error| {
        crate::log_msg(&format!("[memory] initial query failed: {error}"));
        unavailable_sections(show_virtual)
    })
}

/// Refresh cached sections. Returns `true` when displayed values changed.
pub fn refresh_sections(
    physical: &mut MemorySection,
    virtual_mem: &mut Option<MemorySection>,
    show_virtual: bool,
) -> bool {
    let Ok((next_physical, next_virtual)) = query_sections(show_virtual) else {
        if physical.is_unavailable() && virtual_mem.as_ref().is_none_or(|v| v.is_unavailable()) {
            return false;
        }
        let (next_physical, next_virtual) = unavailable_sections(show_virtual);
        *physical = next_physical;
        *virtual_mem = next_virtual;
        return true;
    };

    let changed = *physical != next_physical || *virtual_mem != next_virtual;
    if changed {
        *physical = next_physical;
        *virtual_mem = next_virtual;
    }
    changed
}

pub fn virtual_for_display(
    virtual_mem: &Option<MemorySection>,
    show_virtual: bool,
) -> Option<&MemorySection> {
    if show_virtual {
        virtual_mem.as_ref()
    } else {
        None
    }
}
