#![windows_subsystem = "windows"]

use gpui_kit::{actions, *};

use memory_cleaner::{
    app::{self, AppEntityHolder},
    locale, log_msg,
    settings::Settings,
    tray::Tray,
    win32,
};

actions!(wmc_gpui, [Quit]);

fn main() {
    // Windows 8 以下直接退出，不做后续工作。
    if !win32::os::is_supported() {
        win32::os::reject_unsupported();
        return;
    }

    // 触碰 UAC 前先唤醒已运行实例：同完整性级别启动可直接激活窗口，
    // 无需提权往返。
    if win32::single_instance::signal_existing_instance()
        == win32::single_instance::InstanceSignal::Signaled
    {
        return;
    }

    if win32::elevation::ensure_elevated() {
        return;
    }

    // 提权重启从这里进入；完整性已对齐后再发信号，然后抢互斥体。
    if win32::single_instance::signal_existing_instance()
        == win32::single_instance::InstanceSignal::Signaled
    {
        return;
    }

    let (command_tx, command_rx) = std::sync::mpsc::channel();
    if let Err(e) = win32::single_instance::ensure_single_instance(command_tx.clone()) {
        log_msg(&e.to_string());
        std::process::exit(0);
    }

    let settings = Settings::load();
    let launch_hidden = win32::startup::is_startup_launch();
    locale::apply(&settings);

    if let Err(e) = win32::notification::init() {
        log_msg(&format!("[notification] init failed: {e:#}"));
    }

    win32::hotkey::bind_command_sender(command_tx.clone());

    if let Err(e) = Tray::install(
        command_tx.clone(),
        settings.run_at_startup,
        settings.auto_cleanup_enabled,
    ) {
        log_msg(&format!("[tray] install failed: {e:#}"));
    }

    let app = gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_quit_mode(QuitMode::Explicit);

    app.run(move |cx| {
        gpui_kit::init(cx);

        cx.bind_keys([KeyBinding::new("alt-f4", Quit, None)]);
        cx.on_action(|_: &Quit, cx: &mut App| {
            let entity = cx
                .try_global::<AppEntityHolder>()
                .map(|holder| holder.0.clone());
            if let Some(entity) = entity {
                entity.update(cx, |app, _| app.save_settings_on_close());
            }
            cx.quit();
        });

        cx.spawn(async move |cx| {
            app::open_main_window(cx, settings, command_tx, command_rx, launch_hidden)
                .expect("Failed to open window");
        })
        .detach();
    });
}
