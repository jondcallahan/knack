//! Knack: browse, edit, and share AI agent skills across tools and projects.

use gpui_kit::{
    App, AppContext as _, WindowBounds, WindowOptions,
    component::{
        TitleBar,
        theme::{Theme, ThemeMode},
    },
    px, size,
};
use knack::{actions, assets, theme, ui};

fn apply_startup_appearance(cx: &mut App) {
    match std::env::var("KNACK_APPEARANCE").ok().as_deref() {
        Some("light") => Theme::change(ThemeMode::Light, None, cx),
        Some("dark") => Theme::change(ThemeMode::Dark, None, cx),
        _ => Theme::sync_system_appearance(None, cx),
    }
}

fn dispatch_to_window(cx: &mut App, action: fn() -> Box<dyn gpui_kit::Action>) {
    if cx.windows().is_empty() {
        open_main_window(cx);
    }
    if let Some(window) = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next())
    {
        let _ = window.update(cx, |_, window, cx| {
            window.dispatch_action(action(), cx);
        });
    }
}

fn open_main_window(cx: &mut App) {
    let bounds = WindowBounds::centered(None, size(px(1260.), px(800.)), cx);
    let options = WindowOptions {
        window_bounds: Some(bounds),
        window_min_size: Some(size(px(860.), px(540.))),
        ..TitleBar::window_options()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| ui::KnackApp::new(window, cx))
    })
    .expect("failed to open Knack window");
}

fn main() {
    let app = gpui_kit::application().with_assets(assets::AppAssets);
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_main_window(cx);
        }
        cx.activate(true);
    });
    app.run(|cx| {
        gpui_kit::init(cx);
        theme::install(cx);
        apply_startup_appearance(cx);
        actions::bind_keys(cx);
        actions::install_menus("Disable", cx);

        cx.on_action(|_: &actions::Quit, cx| ui::KnackApp::handle_global_quit(cx));
        cx.on_action(|_: &actions::HideApp, cx| cx.hide());
        cx.on_action(|_: &actions::HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &actions::ShowAllApps, cx| cx.unhide_other_apps());
        cx.on_action(|_: &actions::CloseWindow, cx| ui::KnackApp::handle_global_close_window(cx));
        cx.on_action(|_: &actions::MinimizeWindow, cx| {
            if let Some(window) = cx.active_window() {
                let _ = window.update(cx, |_, window, _| window.minimize_window());
            }
        });
        cx.on_action(|_: &actions::ZoomWindow, cx| {
            if let Some(window) = cx.active_window() {
                let _ = window.update(cx, |_, window, _| window.zoom_window());
            }
        });
        cx.on_action(|_: &actions::ToggleFullscreen, cx| {
            if let Some(window) = cx.active_window() {
                let _ = window.update(cx, |_, window, _| window.toggle_fullscreen());
            }
        });
        cx.on_action(|_: &actions::NewSkill, cx| {
            if cx.windows().is_empty() {
                open_main_window(cx);
            }
        });
        cx.on_action(|_: &actions::About, cx| {
            dispatch_to_window(cx, || Box::new(actions::About));
        });
        cx.on_action(|_: &actions::CheckForUpdates, cx| {
            dispatch_to_window(cx, || Box::new(actions::CheckForUpdates));
        });

        open_main_window(cx);
        cx.activate(true);
    });
}
