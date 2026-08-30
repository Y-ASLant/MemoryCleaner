pub mod layout;
pub mod memory_card;
pub mod settings_page;
pub mod theme;
pub mod title_bar;

#[cfg(test)]
mod tests {
    #[test]
    fn auto_cleanup_description_explains_low_memory_only_mode() {
        crate::locale::with_locale("en", || {
            assert_eq!(
                super::settings_page::auto_cleanup_description(true, 0),
                "Currently: only when Windows reports low physical memory"
            );
        });
    }
}
