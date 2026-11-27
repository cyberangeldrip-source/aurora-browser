use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::browser;
use crate::core::{
    model::{Bookmark, PluginInfo, Settings, TabGroupEdit, APP_VERSION},
    store::AppState,
    urls,
};
use crate::downloads::{Download, Status};
use crate::layout;
use crate::migration;

#[tauri::command]
pub fn navigate(app: AppHandle, input: String) -> Result<(), String> {
    browser::visit(&app, &input)
}

#[tauri::command]
pub fn tab_action(app: AppHandle, action: String) -> Result<(), String> {
    if action == "home" {
        let state = app.state::<AppState>();
        let home = {
            let store = state.store_guard();
            if store.active().is_some_and(|tab| tab.private) {
                urls::page("private")
            } else {
                store.settings.homepage.clone()
            }
        };
        return browser::visit(&app, &home);
    }
    let view = browser::active_webview(&app).ok_or("Нет активной страницы")?;
    let state = app.state::<AppState>();
    match action.as_str() {
        "devtools" => view.open_devtools(),
        "zoom-in" | "zoom-out" | "zoom-reset" => {
            let scale = {
                let mut store = state.store_guard();
                let id = store.tabs.active_id;
                let tab = store.tab_mut(id).ok_or("Вкладка закрыта")?;
                let previous = tab.zoom.unwrap_or(1.0);
                let scale = match action.as_str() {
                    "zoom-in" => (previous + 0.1).min(3.0),
                    "zoom-out" => (previous - 0.1).max(0.5),
                    _ => 1.0,
                };
                tab.zoom = Some(scale);
                scale
            };
            view.set_zoom(scale).map_err(|error| error.to_string())?;
            browser::sync(&app);
        }
        "back" | "forward" | "reload" | "stop" => {
            #[cfg(windows)]
            crate::windows::action(&view, &action).map_err(|error| error.to_string())?;
            #[cfg(not(windows))]
            {
                let script = match action.as_str() {
                    "back" => "history.back()",
                    "forward" => "history.forward()",
                    "reload" => "location.reload()",
                    "stop" => "window.stop()",
                    _ => unreachable!(),
                };
                view.eval(script).map_err(|error| error.to_string())?;
            }
        }
        _ => return Err("Неизвестная команда вкладки".into()),
    }
    Ok(())
}

#[tauri::command]
pub async fn add_tab(
    app: AppHandle,
    url: Option<String>,
    private: Option<bool>,
) -> Result<(), String> {
    browser::on_main(app, move |app| {
        browser::new_tab(app, url, None, private.unwrap_or(false)).map(|_| ())
    })
    .await
}

#[tauri::command]
pub async fn activate_tab(app: AppHandle, id: u32) -> Result<(), String> {
    browser::on_main(app, move |app| browser::activate(app, id, true)).await
}

#[tauri::command]
pub async fn close_tab(app: AppHandle, id: u32) -> Result<(), String> {
    browser::on_main(app, move |app| browser::close(app, id)).await
}

#[tauri::command]
pub fn move_tab(app: AppHandle, id: u32, before: Option<u32>) -> bool {
    let state = app.state::<AppState>();
    {
        let mut store = state.store_guard();
        if !store.move_tab(id, before) {
            return false;
        }
        state.save_tabs(&store);
    }
    browser::sync(&app);
    true
}

#[tauri::command]
pub fn edit_tab_group(app: AppHandle, id: u32, edit: TabGroupEdit) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut store = state.store_guard();
        store.edit_tab_group(id, edit)?;
        state.save_tabs(&store);
    }
    browser::sync(&app);
    Ok(())
}

#[tauri::command]
pub fn toggle_tab_group(app: AppHandle, id: u32) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut store = state.store_guard();
        store.toggle_tab_group(id)?;
        state.save_tabs(&store);
    }
    browser::sync(&app);
    Ok(())
}

#[tauri::command]
pub async fn reopen_tab(app: AppHandle) -> Result<(), String> {
    browser::on_main(app, browser::reopen).await
}

#[tauri::command]
pub fn ui_state(app: AppHandle) -> Value {
    let state = app.state::<AppState>();
    let store = state.store_guard();
    let tab = store.active();
    let url = tab.map_or("", |tab| &tab.url);
    let tabs: Vec<_> = store
        .tabs
        .tabs
        .iter()
        .map(|tab| {
            json!({
                "id": tab.id,
                "url": tab.url,
                "title": tab.title,
                "private": tab.private,
                "group": tab.group,
                "faviconRevision": tab.favicon_revision,
            })
        })
        .collect();
    json!({
        "settings": store.settings,
        "tabs": tabs,
        "activeTabId": store.tabs.active_id,
        "private": tab.is_some_and(|tab| tab.private),
        "currentUrl": url,
        "loading": tab.is_some_and(|tab| tab.loading),
        "failed": tab.is_some_and(|tab| tab.failed),
        "canBack": tab.is_some_and(|tab| tab.can_back),
        "canForward": tab.is_some_and(|tab| tab.can_forward),
        "zoom": tab.and_then(|tab| tab.zoom).unwrap_or(1.0),
        "bookmarked": store.bookmarks.iter().any(|bookmark| bookmark.url == url),
        "canBookmark": urls::bookmark_url(url).is_some(),
        "canReopen": store.last_closed().is_some(),
        "bookmarks": store.bookmarks,
        "meta": {
            "version": APP_VERSION,
            "dataDir": state.data_dir.display().to_string(),
            "updates": cfg!(all(windows, target_arch = "x86_64")),
        },
    })
}

#[tauri::command]
pub fn save_settings(app: AppHandle, mut settings: Settings) -> Settings {
    settings.sanitize();
    let state = app.state::<AppState>();
    let (update_page, update_layout, update_theme, update_wallpaper) = {
        let mut store = state.store_guard();
        let new_theme = settings.theme != store.settings.theme;
        let animations_changed = settings.animations != store.settings.animations;
        let update_wallpaper =
            animations_changed || settings.animated_wallpaper != store.settings.animated_wallpaper;
        let update_page = new_theme
            || settings.accent != store.settings.accent
            || animations_changed
            || settings.search_engine != store.settings.search_engine;
        let update_layout = settings.compact != store.settings.compact
            || settings.show_tab_row != store.settings.show_tab_row
            || settings.show_bookmark_bar != store.settings.show_bookmark_bar;
        let changed_retention =
            settings.history_retention_days != store.settings.history_retention_days;
        let changed_restore = settings.restore_session != store.settings.restore_session;
        store.settings = settings.clone();
        if changed_retention {
            store.prune_history();
            state.save_history(&store);
        }
        if changed_restore {
            state.save_tabs(&store);
        }
        state.save_settings(&store);
        (update_page, update_layout, new_theme, update_wallpaper)
    };
    if update_theme {
        layout::set_theme(&app);
    }
    if update_layout {
        let _ = layout::relayout(&app);
    }
    if update_wallpaper {
        browser::set_wallpaper(&app, settings.animated_wallpaper && settings.animations);
    }
    if update_page {
        browser::refresh_internal(&app);
    }
    settings
}

#[tauri::command]
pub fn set_ui_overlay(app: AppHandle, pixels: u32) -> Result<(), String> {
    let pixels = pixels.min(10_000);
    let state = app.state::<AppState>();
    let previous = state.ui_overlay.swap(pixels, Relaxed);
    if previous == pixels {
        return Ok(());
    }
    if let Err(error) = layout::update_overlay(&app) {
        state.ui_overlay.store(previous, Relaxed);
        return Err(error.to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn glass_frame(app: AppHandle, id: u32) -> Option<Value> {
    #[cfg(windows)]
    {
        let (send, mut receive) = tauri::async_runtime::channel(1);
        let handle = app.clone();
        app.run_on_main_thread(move || {
            if handle.state::<AppState>().store_guard().tabs.active_id != id {
                return;
            }
            let Ok((width, height, top, _)) = layout::metrics(&handle) else { return };
            let Some(view) = browser::active_webview(&handle) else { return };
            if top > 0.0 && width > 0.0 && height > top {
                crate::windows::capture_glass_frame(&view, move |image| {
                    let _ = send.try_send(json!({ "image": image, "width": width, "height": height - top, "top": top }));
                });
            }
        }).ok()?;
        receive.recv().await
    }
    #[cfg(not(windows))]
    {
        let _ = (app, id);
        None
    }
}

#[tauri::command]
pub fn toggle_bookmark(app: AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let bookmarked = {
        let mut store = state.store_guard();
        let tab = store.active().ok_or("Нет активной вкладки")?;
        let url = urls::bookmark_url(&tab.url).ok_or("Можно сохранять закладки на сайты")?;
        let title = tab.title.clone();
        let previous = store
            .bookmarks
            .iter()
            .find(|bookmark| bookmark.url == url)
            .map(|bookmark| bookmark.id);
        let bookmarked = if let Some(id) = previous {
            store.remove_bookmark(id);
            false
        } else {
            store.add_bookmark(url, &title);
            true
        };
        state.save_bookmarks(&store);
        bookmarked
    };
    #[cfg(windows)]
    if bookmarked {
        let active_id = state.store_guard().tabs.active_id;
        if let Some(view) = browser::active_webview(&app) {
            crate::windows::capture_bookmarked_page(&view, active_id);
        }
    }
    browser::sync(&app);
    browser::refresh_home(&app);
    Ok(bookmarked)
}

#[tauri::command]
pub fn edit_bookmark(
    app: AppHandle,
    id: u64,
    title: String,
    folder: Vec<String>,
    whole_folder: Option<bool>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut store = state.store_guard();
        if !store.edit_bookmark(id, title, folder, whole_folder.unwrap_or(false))? {
            return Ok(());
        }
        state.save_bookmarks(&store);
    }
    browser::sync(&app);
    browser::refresh_home(&app);
    Ok(())
}

#[tauri::command]
pub fn move_bookmark(app: AppHandle, id: u64, before: Option<u64>) -> bool {
    let state = app.state::<AppState>();
    {
        let mut store = state.store_guard();
        if !store.move_bookmark(id, before) {
            return false;
        }
        state.save_bookmarks(&store);
    }
    browser::sync(&app);
    browser::refresh_home(&app);
    true
}

#[tauri::command]
pub fn remove_bookmark_by_id(app: AppHandle, id: u64) -> bool {
    let state = app.state::<AppState>();
    let removed = {
        let mut store = state.store_guard();
        let removed = store.remove_bookmark(id);
        if removed {
            state.save_bookmarks(&store);
        }
        removed
    };
    if removed {
        browser::sync(&app);
        browser::refresh_home(&app);
    }
    removed
}

#[tauri::command]
pub fn history_clear(app: AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut store = state.store_guard();
        store.history.clear();
        state.save_history(&store);
    }
    browser::refresh_internal(&app);
}

#[tauri::command]
pub fn suggest(app: AppHandle, q: String) -> Vec<Value> {
    let needle = q.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let state = app.state::<AppState>();
    let store = state.store_guard();
    let mut seen = HashSet::new();
    let mut matches = |url: &str, title: &str| {
        (url.to_lowercase().contains(&needle) || title.to_lowercase().contains(&needle))
            && seen.insert(url.to_owned())
    };
    let mut result = Vec::with_capacity(8);
    result.extend(
        store
            .bookmarks
            .iter()
            .filter(|item| matches(&item.url, &item.title))
            .take(3)
            .map(|item| json!({ "kind": "bookmark", "title": item.title, "url": item.url })),
    );
    result.extend(
        store
            .history
            .iter()
            .rev()
            .filter(|item| matches(&item.url, &item.title))
            .take(5)
            .map(|item| json!({ "kind": "history", "title": item.title, "url": item.url })),
    );
    result
}

#[tauri::command]
pub async fn plugins_list(app: AppHandle) -> Vec<PluginInfo> {
    let state = app.state::<AppState>();
    let mut plugins = state.plugins_guard();
    plugins.refresh();
    plugins.list().to_vec()
}

#[tauri::command]
pub async fn plugin_set_enabled(app: AppHandle, id: String, enabled: bool) -> Result<(), String> {
    app.state::<AppState>()
        .plugins_guard()
        .set_enabled(&id, enabled)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn plugin_get_code(app: AppHandle, id: String) -> Result<(String, String), String> {
    app.state::<AppState>()
        .plugins_guard()
        .get_code(&id)
        .ok_or_else(|| "Плагин не найден".into())
}

#[tauri::command]
pub async fn plugin_set_code(
    app: AppHandle,
    id: String,
    css: String,
    js: String,
    hosts: Option<String>,
) -> Result<(), String> {
    app.state::<AppState>()
        .plugins_guard()
        .set_code(&id, &css, &js, hosts.as_deref())
        .map_err(|error| error.to_string())
}

fn open_folder(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let executable = "explorer.exe";
    #[cfg(target_os = "macos")]
    let executable = "open";
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let executable = "xdg-open";
    std::process::Command::new(executable)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn downloads_state(app: AppHandle) -> Vec<Download> {
    let state = app.state::<AppState>();
    let private = state.store_guard().active().is_some_and(|tab| tab.private);
    let entries = state.downloads_guard().visible(private);
    entries
}

#[tauri::command]
pub fn open_downloads_folder(app: AppHandle) -> Result<(), String> {
    let folder = app
        .path()
        .download_dir()
        .map_err(|error| error.to_string())?;
    open_folder(&folder)
}

#[tauri::command]
pub async fn download_action(app: AppHandle, id: u64, action: String) -> Result<(), String> {
    browser::on_main(app, move |app| {
        let item = downloads_state(app.clone())
            .into_iter()
            .find(|item| item.id == id)
            .ok_or("Загрузка недоступна в текущем сеансе")?;
        let action = action.as_str();
        if action == "folder" {
            let folder = item
                .path
                .parent()
                .filter(|path| path.is_absolute() && path.is_dir())
                .ok_or("Папка файла недоступна")?;
            return open_folder(folder);
        }
        let allowed = match action {
            "pause" => item.status == Status::Running,
            "resume" => item.can_resume,
            "cancel" => matches!(item.status, Status::Running | Status::Paused) || item.can_resume,
            _ => false,
        };
        if !allowed {
            return Err("Эта команда сейчас недоступна".into());
        }
        #[cfg(windows)]
        return crate::windows::control_download(id, action);
        #[cfg(not(windows))]
        Err("Управление загрузками доступно в Windows".into())
    })
    .await
}

#[tauri::command]
pub fn plugins_open_folder(app: AppHandle) -> Result<(), String> {
    let path = app.state::<AppState>().plugins_guard().dir().to_owned();
    open_folder(&path)
}

#[tauri::command]
pub fn export_data(app: AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let store = state.store_guard();
    serde_json::to_string_pretty(&json!({
        "app": "aurora-browser",
        "version": 1,
        "settings": store.settings,
        "bookmarks": store.bookmarks,
    }))
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn import_data(app: AppHandle, payload: String) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Import {
        settings: Option<Settings>,
        bookmarks: Option<Vec<Bookmark>>,
    }
    if payload.len() > 8 * 1024 * 1024 {
        return Err("Допустимый размер файла — до 8 МБ".into());
    }
    let data: Import =
        serde_json::from_str(&payload).map_err(|error| format!("Некорректный JSON. {error}"))?;
    let mut results = Vec::new();
    if let Some(settings) = data.settings {
        save_settings(app.clone(), settings);
        results.push("Настройки обновлены".to_owned());
    }
    if let Some(bookmarks) = data.bookmarks {
        let state = app.state::<AppState>();
        let mut store = state.store_guard();
        store.replace_bookmarks(bookmarks);
        state.save_bookmarks(&store);
        results.push(format!(
            "Закладок импортировано — {}",
            store.bookmarks.len()
        ));
        drop(store);
        browser::refresh_home(&app);
    }
    if results.is_empty() {
        return Err("В файле нет настроек или закладок Aurora".into());
    }
    browser::sync(&app);
    Ok(results.join(". "))
}

#[tauri::command]
pub async fn browser_sources() -> Result<Vec<migration::Source>, String> {
    tauri::async_runtime::spawn_blocking(migration::sources)
        .await
        .map_err(|_| "Не удалось определить браузеры".into())
}

#[tauri::command]
pub async fn import_browser(
    app: AppHandle,
    id: String,
    fields: migration::Selection,
) -> Result<migration::Summary, String> {
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let data = migration::read_browser(&id, fields)?;
        let state = handle.state::<AppState>();
        let report = migration::apply(&state, data);
        state.flush_saves();
        Ok::<_, String>(report)
    })
    .await
    .map_err(|_| "Не удалось завершить перенос")??;
    browser::sync(&app);
    if result.bookmarks != 0 {
        browser::refresh_home(&app);
    }
    Ok(result)
}

#[tauri::command]
pub async fn import_bookmark_html(
    app: AppHandle,
    payload: String,
) -> Result<migration::Summary, String> {
    let handle = app.clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        let mut data = migration::BrowserData::default();
        data.bookmarks = migration::html_bookmarks(&payload)?;
        let state = handle.state::<AppState>();
        let summary = migration::apply(&state, data);
        state.flush_saves();
        Ok::<_, String>(summary)
    })
    .await
    .map_err(|_| "Не удалось прочитать закладки")??;
    browser::sync(&app);
    if report.bookmarks != 0 {
        browser::refresh_home(&app);
    }
    Ok(report)
}

#[cfg(windows)]
#[tauri::command]
pub async fn import_password_csv(
    app: AppHandle,
    payload: String,
) -> Result<migration::passwords::PasswordImport, String> {
    let profile = app.state::<AppState>().data_dir.clone();
    let text = zeroize::Zeroizing::new(payload);
    tauri::async_runtime::spawn_blocking(move || migration::passwords::import(&profile, text))
        .await
        .map_err(|_| "Не удалось завершить импорт паролей")?
}

#[cfg(windows)]
fn password_target(app: &AppHandle) -> Option<(String, tauri::Webview)> {
    let state = app.state::<AppState>();
    let store = state.store_guard();
    let tab = store.active().filter(|tab| !tab.private && !tab.failed)?;
    Some((
        migration::passwords::origin(&tab.url)?,
        app.get_webview(&tab.label)?,
    ))
}

#[cfg(windows)]
#[tauri::command]
pub async fn password_manage(
    app: AppHandle,
    action: String,
    site: String,
    username: String,
    password: Option<String>,
) -> Result<Vec<(String, String)>, String> {
    let dir = app.state::<AppState>().data_dir.clone();
    let password = password.map(zeroize::Zeroizing::new);
    tauri::async_runtime::spawn_blocking(move || {
        migration::passwords::manage(&dir, &action, &site, username, password)
    })
    .await
    .map_err(|_| "Ошибка хранилища")?
}

#[cfg(windows)]
#[tauri::command]
pub async fn browser_extensions(
    app: AppHandle,
    action: String,
    value: String,
    enabled: Option<bool>,
) -> Result<Vec<crate::windows_extensions::Extension>, String> {
    use crate::windows_extensions::{self, Request};
    let request = match action.as_str() {
        "list" => Request::List,
        "install" => Request::Install(windows_extensions::folder(&value)?),
        "enable" => Request::Enable(value, enabled.unwrap_or(false)),
        "remove" => Request::Remove(value),
        _ => return Err("Неизвестное действие с расширением".into()),
    };
    let state = app.state::<AppState>();
    let view = state
        .store_guard()
        .tabs
        .tabs
        .iter()
        .filter(|tab| !tab.private)
        .find_map(|tab| app.get_webview(&tab.label))
        .ok_or("Откройте обычную вкладку для управления расширениями")?;
    windows_extensions::request(view, request).await
}

#[cfg(windows)]
#[tauri::command]
pub async fn password_accounts(
    app: AppHandle,
) -> Result<Vec<migration::passwords::Account>, String> {
    let Some((site, _)) = password_target(&app) else {
        return Ok(Vec::new());
    };
    let profile = app.state::<AppState>().data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || migration::passwords::accounts(&profile, &site))
        .await
        .map_err(|_| "Не удалось открыть хранилище")?
}

#[cfg(windows)]
#[tauri::command]
pub async fn fill_password(app: AppHandle, index: usize, username: String) -> Result<(), String> {
    let (site, view) =
        password_target(&app).ok_or("Откройте обычную вкладку с HTTPS-страницей входа")?;
    let profile = app.state::<AppState>().data_dir.clone();
    migration::passwords::fill(view, &profile, &site, index, &username).await
}

#[tauri::command]
pub fn window_ctl(app: AppHandle, action: &str) -> Result<(), String> {
    let window = app.get_window("main").ok_or("Окно было закрыто")?;
    let result = match action {
        "minimize" => window.minimize(),
        "maximize" => {
            if window.is_maximized().unwrap_or_default() {
                window.unmaximize()
            } else {
                window.maximize()
            }
        }
        "fullscreen" => {
            let state = app.state::<AppState>();
            if state.media_fullscreen.load(Relaxed) != 0 {
                if let Some(view) = browser::active_webview(&app) {
                    let _ = view.eval("document.exitFullscreen().catch(() => {})");
                }
                return Ok(());
            }
            window.set_fullscreen(!window.is_fullscreen().unwrap_or_default())
        }
        "close" => window.close(),
        _ => return Err("Неизвестная команда окна".into()),
    };
    result.map_err(|error| error.to_string())
}

#[tauri::command]
pub fn profiles_list(app: AppHandle) -> Value {
    let active = app.state::<crate::profiles::ActiveProfile>();
    json!({ "current": active.id, "profiles": crate::profiles::list(&active.root) })
}

#[tauri::command]
pub fn profile_create(app: AppHandle, name: String) -> Result<crate::profiles::Profile, String> {
    crate::profiles::create(&app.state::<crate::profiles::ActiveProfile>().root, &name)
}

#[tauri::command]
pub fn profile_open(app: AppHandle, id: String) -> Result<(), String> {
    crate::profiles::launch(&app.state::<crate::profiles::ActiveProfile>(), &id)
}

#[tauri::command]
pub fn window_state(app: AppHandle) -> Value {
    let window = app.get_window("main");
    json!({
        "maximized": window.as_ref().and_then(|window| window.is_maximized().ok()).unwrap_or_default(),
        "fullscreen": window.and_then(|window| window.is_fullscreen().ok()).unwrap_or_default(),
    })
}

#[tauri::command]
pub fn win_drag(app: AppHandle) -> Result<(), String> {
    app.get_window("main")
        .ok_or("Окно было закрыто")?
        .start_dragging()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn focus_chrome(app: AppHandle) -> Result<(), String> {
    app.get_webview("ui")
        .ok_or_else(|| "Окно было закрыто".to_owned())?
        .set_focus()
        .map_err(|error| error.to_string())
}
