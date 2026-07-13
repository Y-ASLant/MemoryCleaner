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
        (false, false) => t!(
            "cleanup.partial",
            count = completed.len(),
            errors = errors.join(list_separator())
        )
        .to_string(),
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
                build_cleanup_result_message(&["工作集", "待机列表"], &["注册表缓存"], ""),
                "完成 2 项，失败：注册表缓存"
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
                build_cleanup_result_message(&[], &["Working Set", "Registry Cache"], ""),
                "Cleanup failed: Working Set, Registry Cache"
            );
            assert_eq!(
                build_cleanup_result_message(&["Working Set"], &[], ""),
                "Completed (1 items)"
            );
            assert_eq!(
                build_cleanup_result_message(
                    &["Working Set", "Standby List"],
                    &["Registry Cache"],
                    ""
                ),
                "2 done, failed: Registry Cache"
            );
        });
    }
}
