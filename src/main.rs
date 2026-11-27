#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activation;
mod browser;
mod commands;
mod core;
mod downloads;
mod migration;
#[cfg(windows)]
mod os_integration;
mod pages;
mod profiles;
#[cfg(all(windows, target_arch = "x86_64"))]
mod update;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_extensions;

use tauri::utils::config::WindowEffectsConfig;
use tauri::utils::{WindowEffect, WindowEffectState};
use tauri::{Manager, WebviewUrl};

use crate::core::{store::AppState, urls};

pub mod layout {
    use std::sync::atomic::Ordering::Relaxed;
    use std::sync::Mutex;
    use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Rect};

    use crate::core::store::AppState;

    #[derive(Default)]
    pub struct ActiveView(Mutex<Option<String>>);

    #[derive(Default)]
    pub struct ChromeBounds(Mutex<Option<(f64, f64)>>);

    pub fn metrics(app: &AppHandle) -> tauri::Result<(f64, f64, f64, f64)> {
        let state = app.state::<AppState>();
        let window = app.get_window("main").ok_or(tauri::Error::WindowNotFound)?;
        let scale = window.scale_factor()?;
        let size = window.inner_size()?;
        let width = f64::from(size.width) / scale;
        let height = f64::from(size.height) / scale;
        let (compact, show_tabs, show_bookmarks) = {
            let settings = &state.store_guard().settings;
            (
                settings.compact,
                settings.show_tab_row,
                settings.show_bookmark_bar,
            )
        };
        let strip = if state.media_fullscreen.load(Relaxed) != 0 {
            0.0
        } else {
            let address = if compact { 42.0 } else { 52.0 };
            address + if show_tabs { 44.0 } else { 0.0 } + if show_bookmarks { 36.0 } else { 0.0 }
        };
        let overlay = f64::from(state.ui_overlay.load(Relaxed)).min((height - strip).max(0.0));
        Ok((width, height, strip, overlay))
    }

    pub fn relayout(app: &AppHandle) -> tauri::Result<()> {
        let (width, height, strip, overlay) = metrics(app)?;
        if width < 1.0 || height < 1.0 {
            return Ok(());
        }
        let state = app.state::<AppState>();
        let active = state.store_guard().active().map(|tab| tab.label.clone());
        let visibility = app.state::<ActiveView>();
        let (previous, active, changed) = {
            let mut visible = visibility
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let desired = active.filter(|_| cfg!(windows) || overlay == 0.0 || strip == 0.0);
            let changed = *visible != desired;
            let previous = if changed { visible.clone() } else { None };
            *visible = desired.clone();
            (previous, desired, changed)
        };
        if let Some(view) = previous.and_then(|label| app.get_webview(&label)) {
            view.hide()?;
        }
        if let Some(view) = active.and_then(|label| app.get_webview(&label)) {
            view.set_bounds(Rect {
                position: LogicalPosition::new(0.0, strip).into(),
                size: LogicalSize::new(width, (height - strip).max(1.0)).into(),
            })?;
            if changed {
                view.show()?;
            }
        }
        position_chrome(app, width, strip, overlay, changed)
    }

    pub fn update_overlay(app: &AppHandle) -> tauri::Result<()> {
        if !cfg!(windows) {
            return relayout(app);
        }
        let (width, _, strip, overlay) = metrics(app)?;
        position_chrome(app, width, strip, overlay, false)
    }

    fn position_chrome(
        app: &AppHandle,
        width: f64,
        strip: f64,
        overlay: f64,
        _restack: bool,
    ) -> tauri::Result<()> {
        if let Some(ui) = app.get_webview("ui") {
            let state = app.state::<ChromeBounds>();
            let previous = *state.0.lock().unwrap_or_else(|error| error.into_inner());
            if strip == 0.0 {
                if previous.is_some() {
                    ui.hide()?;
                    *state.0.lock().unwrap_or_else(|error| error.into_inner()) = None;
                }
                return Ok(());
            }
            let bounds = (width.max(1.0), strip + overlay);
            #[cfg(windows)]
            if overlay > 0.0 && (_restack || previous != Some(bounds)) {
                crate::windows::raise(&ui);
            }
            if previous != Some(bounds) {
                ui.set_bounds(Rect {
                    position: LogicalPosition::new(0.0, 0.0).into(),
                    size: LogicalSize::new(bounds.0, bounds.1).into(),
                })?;
                if previous.is_none() {
                    ui.show()?;
                }
                *state.0.lock().unwrap_or_else(|error| error.into_inner()) = Some(bounds);
            }
        }
        Ok(())
    }

    pub fn set_theme(app: &AppHandle) {
        if let Some(window) = app.get_window("main") {
            let state = app.state::<AppState>();
            let light = state.store_guard().settings.theme == "light";
            let _ = window.set_theme(Some(if light {
                tauri::Theme::Light
            } else {
                tauri::Theme::Dark
            }));
        }
    }
}

fn run(launch: activation::Launch) -> tauri::Result<()> {
    let invoke: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        commands::navigate,
        commands::tab_action,
        commands::add_tab,
        commands::activate_tab,
        commands::close_tab,
        commands::move_tab,
        commands::edit_tab_group,
        commands::toggle_tab_group,
        commands::reopen_tab,
        commands::ui_state,
        commands::save_settings,
        commands::set_ui_overlay,
        commands::glass_frame,
        commands::toggle_bookmark,
        commands::edit_bookmark,
        commands::move_bookmark,
        commands::remove_bookmark_by_id,
        commands::history_clear,
        commands::suggest,
        commands::plugins_list,
        commands::plugin_set_enabled,
        commands::plugin_get_code,
        commands::plugin_set_code,
        commands::plugins_open_folder,
        commands::export_data,
        commands::import_data,
        commands::browser_sources,
        commands::import_browser,
        commands::import_bookmark_html,
        commands::profiles_list,
        commands::profile_create,
        commands::profile_open,
        commands::window_ctl,
        commands::window_state,
        commands::win_drag,
        commands::focus_chrome,
        commands::downloads_state,
        commands::download_action,
        commands::open_downloads_folder,
    ];
    #[cfg(windows)]
    let vault_invoke: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        commands::import_password_csv,
        commands::password_accounts,
        commands::fill_password,
        commands::browser_extensions,
        commands::password_manage,
        os_integration::system_integration,
        os_integration::set_autostart,
        os_integration::default_browser_settings,
    ];
    #[cfg(all(windows, target_arch = "x86_64"))]
    let update_invoke: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        update::update_check,
        update::update_install,
        update::update_last_result,
    ];
    tauri::Builder::default()
        .register_uri_scheme_protocol("aurora", |ctx, request| {
            pages::handle(ctx.app_handle(), ctx.webview_label(), request)
        })
        .setup(move |app| {
            let root = match std::env::var_os("AURORA_DATA_DIR") {
                Some(path) => std::path::PathBuf::from(path),
                None => app.path().app_data_dir()?,
            };
            let profile = profiles::ActiveProfile::from_args(root)?;
            let path = profile.path();
            let state = match AppState::init(path.clone()) {
                Ok(state) => state,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    activation::send(&path, &launch)?;
                    std::process::exit(0);
                }
                Err(error) => return Err(error.into()),
            };
            if let Some(target) = &launch.target {
                let mut store = state.store_guard();
                if store.tabs.tabs.len() == 1 && store.tabs.tabs[0].url == urls::page_home() {
                    let tab = &mut store.tabs.tabs[0];
                    *tab = core::model::Tab::new(tab.id, target.clone());
                } else {
                    let tab = store.allocate_tab(target.clone());
                    store.tabs.active_id = tab.id;
                    store.tabs.tabs.push(tab);
                }
            }
            let chrome_profile = state.data_dir.join("Chrome");
            app.manage(state);
            app.manage(profile);
            app.manage(layout::ActiveView::default());
            app.manage(layout::ChromeBounds::default());
            #[cfg(all(windows, target_arch = "x86_64"))]
            app.manage(update::Updater::default());
            app.manage(activation::watch(app.handle(), &path)?);

            let mut builder = tauri::window::WindowBuilder::new(app, "main")
                .title("Aurora")
                .inner_size(1280.0, 820.0)
                .min_inner_size(660.0, 450.0)
                .decorations(false)
                .transparent(cfg!(any(windows, target_os = "macos")))
                .resizable(true)
                .center()
                .shadow(true)
                .visible(false);
            if cfg!(windows) {
                builder = builder.effects(WindowEffectsConfig {
                    effects: vec![WindowEffect::Mica],
                    state: Some(WindowEffectState::Active),
                    radius: None,
                    color: None,
                    interactive: false,
                });
            }
            let window = builder.build()?;
            layout::set_theme(app.handle());
            let (width, _, strip, _) = layout::metrics(app.handle())?;
            let _ui = window.add_child(
                tauri::webview::WebviewBuilder::new("ui", WebviewUrl::App("strip.html".into()))
                    .accept_first_mouse(true)
                    .data_directory(chrome_profile)
                    .transparent(true)
                    .on_navigation(urls::is_chrome)
                    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny),
                tauri::LogicalPosition::new(0.0, 0.0),
                tauri::LogicalSize::new(width, strip),
            )?;
            #[cfg(windows)]
            crate::windows::install_chrome_downloads(&_ui);

            let handle = app.handle().clone();
            window.on_window_event(move |event| {
                if matches!(
                    event,
                    tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. }
                ) {
                    let _ = layout::relayout(&handle);
                    browser::emit(&handle, "au://viewport", ());
                }
            });

            window.show()?;
            browser::defer(app.handle(), move |app| {
                let active = app.state::<AppState>().store_guard().tabs.active_id;
                if let Err(error) = browser::activate(&app, active, false) {
                    browser::emit(&app, "au://notice", error);
                }
                activation::drain(&app);
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                window.state::<AppState>().save_before_exit();
            }
        })
        .invoke_handler(move |request: tauri::ipc::Invoke<tauri::Wry>| {
            if request.message.webview_ref().label() == "ui" {
                #[cfg(all(windows, target_arch = "x86_64"))]
                if matches!(
                    request.message.command(),
                    "update_check" | "update_install" | "update_last_result"
                ) {
                    return update_invoke(request);
                }
                #[cfg(windows)]
                if matches!(
                    request.message.command(),
                    "import_password_csv"
                        | "password_accounts"
                        | "fill_password"
                        | "browser_extensions"
                        | "password_manage"
                        | "system_integration"
                        | "set_autostart"
                        | "default_browser_settings"
                ) {
                    return vault_invoke(request);
                }
                invoke(request)
            } else {
                request
                    .resolver
                    .reject("Команда доступна только интерфейсу Aurora");
                true
            }
        })
        .run(tauri::generate_context!())
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    if let Some(command) = args.next() {
        #[cfg(all(windows, target_arch = "x86_64"))]
        if command == "--apply-update" {
            if let Err(error) = update::apply_worker() {
                eprintln!("Aurora: {error}");
                std::process::exit(1);
            }
            return;
        }
        if command == "--import-brave" {
            print_migration_result(migration::migrate_brave());
            return;
        }
        #[cfg(windows)]
        if command == "--import-passwords" {
            let result = args
                .next()
                .ok_or("Укажите файл CSV".into())
                .and_then(|path| migration::passwords::import_file(std::path::Path::new(&path)));
            print_migration_result(result);
            return;
        }
        #[cfg(windows)]
        if command == "--register-browser" || command == "--unregister-browser" {
            let result = if command == "--register-browser" {
                os_integration::register_browser()
            } else {
                os_integration::unregister_browser()
            };
            if let Err(error) = result {
                eprintln!("Aurora: {error}");
                std::process::exit(1);
            }
            return;
        }
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    let _runtime_guard = update::runtime_guard();
    let result = activation::Launch::parse(std::env::args_os().skip(1))
        .and_then(|launch| run(launch).map_err(|error| error.to_string()));
    if let Err(error) = result {
        eprintln!("Aurora: {error}");
        #[cfg(windows)]
        {
            use ::windows::core::HSTRING;
            use ::windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            let message = HSTRING::from(format!("Не удалось запустить Aurora.\n\n{error}"));
            let title = ::windows::core::w!("Aurora");
            // No application window is available after a startup error.
            unsafe { MessageBoxW(None, &message, title, MB_OK | MB_ICONERROR) };
        }
        std::process::exit(1);
    }
}

fn print_migration_result(result: Result<impl serde::Serialize, String>) {
    match result {
        Ok(summary) => println!(
            "{}",
            serde_json::to_string(&summary).expect("valid summary")
        ),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}
