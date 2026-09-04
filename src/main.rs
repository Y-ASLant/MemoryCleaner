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
    // Wake an already-running instance before touching UAC: a same-integrity
    // launch can activate the running window without an elevation round-trip.
    if win32::single_instance::signal_existing_instance()
        == win32::single_instance::InstanceSignal::Signaled
    {
        return;
    }

    win32::elevation::ensure_elevated();

    // The elevated relaunch enters here; retry the signal now that the
    // integrity level matches the running instance, then claim the mutex.
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
    if let Err(error) = win32::startup::sync(&settings) {
        log_msg(&format!("[startup] sync failed: {error:#}"));
    }
    let launch_hidden = win32::startup::is_startup_launch();
    locale::apply(&settings);

    if let Err(e) = win32::notification::init() {
        log_msg(&format!("[notification] init failed: {e:#}"));
    }

    win32::hotkey::bind_command_sender(command_tx.clone());

    if let Err(e) = Tray::install(command_tx.clone()) {
        log_msg(&format!("[tray] install failed: {e:#}"));
    }
    win32::hotkey::sync(&settings);

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
                entity.update(cx, |app, _| app.settings.save());
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
