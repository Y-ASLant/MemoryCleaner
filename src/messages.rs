use rust_i18n::t;

use crate::locale::list_separator;
use crate::memory::MemoryStatus;

pub fn format_freed_message(avail_before: u64, avail_after: u64) -> String {
    if avail_after > avail_before {
        format!(
            "+{}",
            MemoryStatus::format_bytes(avail_after - avail_before)
        )
    } else {
        String::new()
    }
}

pub fn format_cleanup_effect(
    avail_before: u64,
    avail_after: u64,
    memory_load_before: u32,
    memory_load_after: u32,
) -> String {
    let freed = format_freed_message(avail_before, avail_after);
    let pressure = t!(
        "cleanup.pressure_change",
        before = memory_load_before.to_string(),
        after = memory_load_after.to_string()
    )
    .to_string();

    if freed.is_empty() {
        pressure
    } else {
        format!("{freed} · {pressure}")
    }
}

pub fn build_cleanup_result_message(
    completed: &[&str],
    errors: &[&str],
    freed_detail: &str,
) -> String {
    match (completed.is_empty(), errors.is_empty()) {
        (true, true) => t!("cleanup.none").to_string(),
        (true, false) => t!("cleanup.failed", errors = errors.join(list_separator())).to_string(),
        (false, true) => {
            if freed_detail.is_empty() {
                t!("cleanup.completed", count = completed.len()).to_string()
            } else {
                t!("cleanup.completed_detail", detail = freed_detail).to_string()
            }
        }
        (false, false) => {
            if freed_detail.is_empty() {
                t!(
                    "cleanup.partial",
                    count = completed.len(),
                    errors = errors.join(list_separator())
                )
                .to_string()
            } else {
                t!(
                    "cleanup.partial_detail",
                    count = completed.len(),
                    errors = errors.join(list_separator()),
                    detail = freed_detail
                )
                .to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::with_locale;

    #[test]
    fn format_freed_message_only_when_memory_increased() {
        assert_eq!(format_freed_message(1_000, 2_000_000_000), "+1.86 GB");
        assert_eq!(format_freed_message(2_000, 1_000), "");
    }

    #[test]
    fn cleanup_effect_reports_pressure_even_without_more_available_memory() {
        with_locale("en", || {
            assert_eq!(format_cleanup_effect(2_000, 1_000, 80, 81), "80% → 81%");
            assert_eq!(
                format_cleanup_effect(1_000, 2_000_000_000, 80, 75),
                "+1.86 GB · 80% → 75%"
            );
        });
    }

    #[test]
    fn build_cleanup_result_message_variants_zh() {
        with_locale("zh-CN", || {
            assert_eq!(build_cleanup_result_message(&[], &[], ""), "未执行清理");
            assert_eq!(
                build_cleanup_result_message(&[], &["工作集"], ""),
                "清理失败：工作集"
            );
            assert_eq!(
                build_cleanup_result_message(&["工作集"], &[], ""),
                "清理完成（1 项）"
            );
            assert_eq!(
                build_cleanup_result_message(&["工作集"], &[], "+512.00 MB"),
                "清理完成 · +512.00 MB"
            );
            assert_eq!(
                build_cleanup_result_message(&["工作集", "待机列表"], &["已修改页面"], ""),
                "完成 2 项，失败：已修改页面"
            );
        });
    }

    #[test]
    fn build_cleanup_result_message_variants_en() {
        with_locale("en", || {
            assert_eq!(
                build_cleanup_result_message(&[], &[], ""),
                "No cleanup performed"
            );
            assert_eq!(
                build_cleanup_result_message(&[], &["Working Set", "Modified Pages"], ""),
                "Cleanup failed: Working Set, Modified Pages"
            );
            assert_eq!(
                build_cleanup_result_message(&["Working Set"], &[], ""),
                "Completed (1 items)"
            );
            assert_eq!(
                build_cleanup_result_message(
                    &["Working Set", "Standby List"],
                    &["Modified Pages"],
                    ""
                ),
                "2 done, failed: Modified Pages"
            );
        });
    }
}
