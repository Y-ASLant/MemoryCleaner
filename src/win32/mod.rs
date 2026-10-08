pub(crate) mod com;
pub mod elevation;
pub(crate) mod handle;
pub mod hotkey;
pub mod memory_notification;
pub mod notification;
pub mod nt;
pub mod os;
pub mod process;
pub mod single_instance;
pub mod startup;
pub mod volume;
pub mod window;

pub(crate) fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
