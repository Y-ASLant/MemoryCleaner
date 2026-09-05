use super::*;

fn language_options() -> [(&'static str, String); 3] {
    [
        ("auto", t!("settings.language_auto").to_string()),
        ("zh-CN", t!("settings.language_zh").to_string()),
        ("en", t!("settings.language_en").to_string()),
    ]
}

struct SwitchRowConfig {
    id: &'static str,
    icon: IconName,
    title: String,
    description: String,
    checked: bool,
    disabled: bool,
}

fn switch_row_app(
    config: SwitchRowConfig,
    muted: Hsla,
    foreground: Hsla,
    on_click: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let icon = config.icon;

    h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .gap_3()
        .py(px(3.))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(1.))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .child(Icon::new(icon.clone()).small().text_color(muted)),
                        )
                        .child(
                            Label::new(config.title)
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(foreground),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .invisible()
                                .flex()
                                .items_center()
                                .child(Icon::new(icon).small()),
                        )
                        .child(
                            Label::new(config.description)
                                .text_xs()
                                .text_color(muted)
                                .flex_1()
                                .min_w_0(),
                        ),
                ),
        )
        .child(
            div().flex_shrink_0().child(
                Switch::new(config.id)
                    .checked(config.checked)
                    .disabled(config.disabled)
                    .on_click(on_click),
            ),
        )
}

fn render_version_row(cx: &App) -> impl IntoElement {
    let link_color = cx.theme().primary;
    let version = format!("v{}", crate::version::VERSION);

    h_flex().w_full().justify_center().items_center().child(
        div()
            .id("version-link")
            .cursor_pointer()
            .on_click(|_, _, cx| cx.open_url(crate::version::REPO_URL))
            .child(
                Label::new(version)
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(link_color),
            ),
    )
}

fn render_language_selector(
    weak: &WeakEntity<MemoryCleanerApp>,
    muted: Hsla,
    foreground: Hsla,
    cx: &App,
) -> impl IntoElement {
    let current = {
        let app = weak.upgrade();
        app.as_ref()
            .map(|a| a.read(cx).settings.language.clone())
            .unwrap_or_else(|| "auto".into())
    };

    let options = language_options();
    let current_label = options
        .iter()
        .find(|(k, _)| *k == current.as_str())
        .map(|(_, l)| l.clone())
        .unwrap_or_else(|| t!("settings.language_auto").to_string());

    h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .gap_3()
        .py(px(3.))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(1.))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .child(Icon::new(IconName::Globe).small().text_color(muted)),
                        )
                        .child(
                            Label::new(t!("settings.language").to_string())
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(foreground),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .invisible()
                                .flex()
                                .items_center()
                                .child(Icon::new(IconName::Globe).small()),
                        )
                        .child(
                            Label::new(t!("settings.language_desc").to_string())
                                .text_xs()
                                .text_color(muted)
                                .flex_1()
                                .min_w_0(),
                        ),
                ),
        )
        .child({
            let weak = weak.clone();
            Button::new("language-select")
                .ghost()
                .small()
                .min_w(px(128.))
                .label(current_label)
                .dropdown_caret(true)
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    let weak = weak.clone();
                    let current = current.clone();
                    options.iter().fold(menu, |menu, (value, label)| {
                        let value = (*value).to_string();
                        let label = label.clone();
                        let checked = current == value;
                        let weak = weak.clone();
                        menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                            move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    if app.settings.language != value {
                                        app.settings.language = value.clone();
                                        app.apply_locale(cx);
                                    }
                                });
                            },
                        ))
                    })
                })
        })
}

fn cleanup_hotkey_display(
    recording: bool,
    chord: &str,
    border: Hsla,
    background: Hsla,
    foreground: Hsla,
    primary: Hsla,
    muted: Hsla,
) -> Div {
    if recording {
        div().child(
            Label::new(t!("settings.cleanup_hotkey_recording").to_string())
                .text_sm()
                .text_color(primary),
        )
    } else if let Some(keystroke) = HotkeyBinding::chord_to_keystroke(chord) {
        div().child(
            Kbd::new(keystroke)
                .bg(background)
                .border_color(border)
                .text_color(foreground),
        )
    } else {
        div().child(Label::new(chord.to_string()).text_sm().text_color(muted))
    }
}

const AUTO_CLEANUP_THRESHOLD_OPTIONS: &[u32] = &[0, 70, 75, 80, 85, 90, 95];

fn format_threshold_value(value: u32) -> String {
    if value == 0 {
        t!("settings.auto_cleanup_option_off").to_string()
    } else {
        format!("{value}%")
    }
}

pub(crate) fn auto_cleanup_description(enabled: bool, threshold: u32) -> String {
    if !enabled {
        return t!("settings.auto_cleanup_status_disabled").to_string();
    }

    if threshold == 0 {
        t!("settings.auto_cleanup_status_low_memory").to_string()
    } else {
        t!(
            "settings.auto_cleanup_status_threshold",
            threshold = format_threshold_value(threshold)
        )
        .to_string()
    }
}

/// One auto-cleanup trigger row: label + description on the left, a preset
/// dropdown on the right.
struct AutoCleanupOptionRow {
    id: &'static str,
    icon: IconName,
    title: String,
    description: String,
    options: &'static [u32],
    current: u32,
    format_value: fn(u32) -> String,
    setter: fn(&mut MemoryCleanerApp, u32, &mut Context<MemoryCleanerApp>),
}

fn render_auto_cleanup_option_row(
    weak: &WeakEntity<MemoryCleanerApp>,
    row: AutoCleanupOptionRow,
    dim: bool,
    muted: Hsla,
    foreground: Hsla,
) -> impl IntoElement {
    let AutoCleanupOptionRow {
        id,
        icon,
        title,
        description,
        options,
        current,
        format_value,
        setter,
    } = row;
    let current_label = format_value(current);
    let weak_row = weak.clone();

    h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .gap_3()
        .py(px(3.))
        .when(dim, |row| row.opacity(0.5))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(1.))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .child(Icon::new(icon.clone()).small().text_color(muted)),
                        )
                        .child(
                            Label::new(title)
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(foreground),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .invisible()
                                .flex()
                                .items_center()
                                .child(Icon::new(icon).small()),
                        )
                        .child(
                            Label::new(description)
                                .text_xs()
                                .text_color(muted)
                                .flex_1()
                                .min_w_0(),
                        ),
                ),
        )
        .child(
            Button::new(id)
                .ghost()
                .small()
                .min_w(px(128.))
                .label(current_label)
                .dropdown_caret(true)
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    options.iter().fold(menu, |menu, value| {
                        let value = *value;
                        let checked = current == value;
                        let weak = weak_row.clone();
                        menu.item(
                            PopupMenuItem::new(format_value(value))
                                .checked(checked)
                                .on_click(move |_, _, cx| {
                                    let _ = weak.update(cx, |app, cx| setter(app, value, cx));
                                }),
                        )
                    })
                }),
        )
}

fn render_cleanup_hotkey_row(
    weak: &WeakEntity<MemoryCleanerApp>,
    muted: Hsla,
    foreground: Hsla,
    cx: &App,
) -> impl IntoElement {
    let Some(app) = weak.upgrade() else {
        return div();
    };

    let app = app.read(cx);
    let enabled = app.settings.cleanup_hotkey_enabled;
    let recording = app.cleanup_hotkey_recording;
    let chord = app.settings.cleanup_hotkey.clone();
    let focus = app.hotkey_capture_focus.clone();
    let border = cx.theme().border;
    let background = cx.theme().background;
    let primary = cx.theme().primary;
    let radius = cx.theme().radius;

    let weak_switch = weak.clone();
    let weak_capture = weak.clone();
    let focus_capture = focus.clone();

    h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .gap_3()
        .py(px(3.))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(1.))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .child(Icon::new(IconName::ALargeSmall).small().text_color(muted)),
                        )
                        .child(
                            Label::new(t!("settings.cleanup_hotkey").to_string())
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(foreground),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .invisible()
                                .flex()
                                .items_center()
                                .child(Icon::new(IconName::ALargeSmall).small()),
                        )
                        .child(
                            Label::new(if app.cleanup_hotkey_failed {
                                t!("settings.cleanup_hotkey_failed").to_string()
                            } else {
                                t!("settings.cleanup_hotkey_desc").to_string()
                            })
                            .text_xs()
                            .text_color(if app.cleanup_hotkey_failed {
                                cx.theme().danger
                            } else {
                                muted
                            })
                            .flex_1()
                            .min_w_0(),
                        ),
                ),
        )
        .child(
            h_flex()
                .flex_shrink_0()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id("cleanup-hotkey-capture")
                        .track_focus(&focus)
                        .min_w(px(128.))
                        .h(px(28.))
                        .px_2()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(radius)
                        .border_1()
                        .border_color(if recording { primary } else { border })
                        .bg(background)
                        .when(enabled, |this| this.cursor_pointer())
                        .when(!enabled, |this| this.opacity(0.5))
                        .on_key_down({
                            let weak = weak_capture.clone();
                            move |event, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.handle_cleanup_hotkey_key(event, cx);
                                });
                            }
                        })
                        .on_click({
                            let weak = weak_capture;
                            move |_, window, cx| {
                                if !enabled {
                                    return;
                                }
                                let _ = weak.update(cx, |app, cx| {
                                    app.start_cleanup_hotkey_recording(window, cx);
                                });
                                window.focus(&focus_capture, cx);
                            }
                        })
                        .child(cleanup_hotkey_display(
                            recording, &chord, border, background, foreground, primary, muted,
                        )),
                )
                .child(
                    Switch::new("dialog-switch-cleanup-hotkey")
                        .checked(enabled)
                        .on_click({
                            let weak = weak_switch;
                            move |checked, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.set_cleanup_hotkey_enabled(*checked, cx);
                                });
                            }
                        }),
                ),
        )
}

pub fn render_window_behavior_dialog(
    weak: WeakEntity<MemoryCleanerApp>,
    cx: &App,
) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let foreground = cx.theme().foreground;

    let Some(app) = weak.upgrade() else {
        return v_flex()
            .w_full()
            .child(div().w_full().pt(px(4.)).child(render_version_row(cx)));
    };

    let state = app.read(cx);
    let settings = state.settings.clone();
    let startup_pending = state.startup_setting_pending;
    let startup_failed = state.startup_setting_failed;

    v_flex()
        .w_full()
        .gap(px(2.))
        .child(render_language_selector(&weak, muted, foreground, cx))
        .child(render_cleanup_hotkey_row(&weak, muted, foreground, cx))
        .child(switch_row_app(
            SwitchRowConfig {
                id: "dialog-switch-auto-cleanup",
                icon: IconName::Play,
                title: t!("settings.auto_cleanup").to_string(),
                description: auto_cleanup_description(
                    settings.auto_cleanup_enabled,
                    settings.auto_cleanup_threshold,
                ),
                checked: settings.auto_cleanup_enabled,
                disabled: false,
            },
            muted,
            foreground,
            {
                let weak = weak.clone();
                move |checked, _window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.set_auto_cleanup_enabled(*checked, cx);
                    });
                }
            },
        ))
        .child(render_auto_cleanup_option_row(
            &weak,
            AutoCleanupOptionRow {
                id: "dialog-select-auto-cleanup-threshold",
                icon: IconName::ChartPie,
                title: t!("settings.auto_cleanup_threshold").to_string(),
                description: t!("settings.auto_cleanup_threshold_desc").to_string(),
                options: AUTO_CLEANUP_THRESHOLD_OPTIONS,
                current: settings.auto_cleanup_threshold,
                format_value: format_threshold_value,
                setter: MemoryCleanerApp::set_auto_cleanup_threshold,
            },
            !settings.auto_cleanup_enabled,
            muted,
            foreground,
        ))
        .child(switch_row_app(
            SwitchRowConfig {
                id: "dialog-switch-always-on-top",
                icon: IconName::Star,
                title: t!("settings.always_on_top").to_string(),
                description: t!("settings.always_on_top_desc").to_string(),
                checked: settings.always_on_top,
                disabled: false,
            },
            muted,
            foreground,
            {
                let weak = weak.clone();
                move |checked, window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.set_always_on_top(*checked, window, cx);
                    });
                }
            },
        ))
        .child(switch_row_app(
            SwitchRowConfig {
                id: "dialog-switch-close-to-tray",
                icon: IconName::Minimize,
                title: t!("settings.close_to_tray").to_string(),
                description: t!("settings.close_to_tray_desc").to_string(),
                checked: settings.close_to_notification_area,
                disabled: false,
            },
            muted,
            foreground,
            {
                let weak = weak.clone();
                move |checked, _window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.set_close_to_tray(*checked, cx);
                    });
                }
            },
        ))
        .child(switch_row_app(
            SwitchRowConfig {
                id: "dialog-switch-run-at-startup",
                icon: IconName::Play,
                title: t!("settings.run_at_startup").to_string(),
                description: if startup_pending {
                    t!("settings.run_at_startup_pending").to_string()
                } else if startup_failed {
                    t!("settings.run_at_startup_failed").to_string()
                } else {
                    t!("settings.run_at_startup_desc").to_string()
                },
                checked: settings.run_at_startup,
                disabled: startup_pending,
            },
            if startup_failed {
                cx.theme().danger
            } else {
                muted
            },
            foreground,
            {
                let weak = weak.clone();
                move |checked, _window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.set_run_at_startup(*checked, cx);
                    });
                }
            },
        ))
        .child(switch_row_app(
            SwitchRowConfig {
                id: "dialog-switch-optimization-notifications",
                icon: IconName::Bell,
                title: t!("settings.optimization_notifications").to_string(),
                description: t!("settings.optimization_notifications_desc").to_string(),
                checked: settings.show_optimization_notifications,
                disabled: false,
            },
            muted,
            foreground,
            {
                let weak = weak.clone();
                move |checked, _window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.set_show_optimization_notifications(*checked, cx);
                    });
                }
            },
        ))
        .child(switch_row_app(
            SwitchRowConfig {
                id: "dialog-switch-debug-logging",
                icon: IconName::Settings2,
                title: t!("settings.debug_logging").to_string(),
                description: t!("settings.debug_logging_desc").to_string(),
                checked: settings.debug_logging,
                disabled: false,
            },
            muted,
            foreground,
            {
                let weak = weak.clone();
                move |checked, _window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.set_debug_logging(*checked, cx);
                    });
                }
            },
        ))
        .child(
            div()
                .w_full()
                .mt(px(SECTION_GAP))
                .pt(px(SECTION_GAP))
                .border_t_1()
                .border_color(muted.opacity(0.25))
                .child(render_version_row(cx)),
        )
}
