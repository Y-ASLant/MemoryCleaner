#![windows_subsystem = "windows"]

use memory_cleaner::{locale, log_msg, runtime::Runtime, settings::Settings, tray, win32};

fn main() {
    if !win32::os::is_supported() {
        win32::os::reject_unsupported();
        return;
    }

    if win32::single_instance::signal_existing_instance()
        == win32::single_instance::InstanceSignal::Signaled
    {
        return;
    }

    if win32::elevation::ensure_elevated() {
        return;
    }

    if win32::single_instance::signal_existing_instance()
        == win32::single_instance::InstanceSignal::Signaled
    {
        return;
    }

    let (command_tx, command_rx) = match tray::command_channel() {
        Ok(ch) => ch,
        Err(e) => {
            log_msg(&format!("[main] command channel failed: {e:#}"));
            return;
        }
    };
    if let Err(e) = win32::single_instance::ensure_single_instance(command_tx.clone()) {
        log_msg(&e.to_string());
        std::process::exit(0);
    }

    let settings = Settings::load();
    locale::apply(&settings);

    if let Err(e) = win32::notification::init() {
        log_msg(&format!("[notification] init failed: {e:#}"));
    }

    win32::hotkey::bind_command_sender(command_tx.clone());

    // 托盘 HWND 必须建在将要泵消息的线程上。
    if let Err(e) = tray::install(command_tx.clone(), &settings) {
        log_msg(&format!("[tray] install failed: {e:#}"));
        return;
    }

    if let Err(e) = win32::taskbar_window::install(settings.taskbar_font_point_size) {
        log_msg(&format!("[taskbar-window] install failed: {e:#}"));
    } else {
        win32::taskbar_window::set_enabled(settings.show_taskbar_window);
    }

    let runtime = Runtime::start(settings, command_tx);

    tray::run_loop(command_rx, |command| {
        Runtime::handle_command(&runtime, command);
    });
}
