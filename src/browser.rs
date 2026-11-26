use tauri::{AppHandle, Emitter, EventTarget, Manager, Webview, WebviewUrl};

use crate::core::{model::Tab, store::AppState, urls};
use crate::layout;

pub fn emit(app: &AppHandle, name: &str, payload: impl serde::Serialize + Clone) {
    let _ = app.emit_to(EventTarget::webview("ui"), name, payload);
}

pub fn active_webview(app: &AppHandle) -> Option<Webview> {
    let label = app
        .state::<AppState>()
        .store_guard()
        .active()?
        .label
        .clone();
    app.get_webview(&label)
}

pub fn sync(app: &AppHandle) {
    emit(app, "au://state", ());
}

pub fn defer(app: &AppHandle, action: impl FnOnce(AppHandle) + Send + 'static) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || action(handle));
    });
}

pub async fn on_main<T: Send + 'static>(
    app: AppHandle,
    action: impl FnOnce(&AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (send, mut receive) = tauri::async_runtime::channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = send.try_send(action(&handle));
    })
    .map_err(|error| error.to_string())?;
    receive
        .recv()
        .await
        .unwrap_or_else(|| Err("Окно было закрыто".into()))
}

fn update_source(app: &AppHandle, id: u32, url: &url::Url, same_document: bool, record: bool) {
    if !urls::can_visit(url) {
        return;
    }
    let state = app.state::<AppState>();
    {
        let mut store = state.store_guard();
        let Some(tab) = store.tab_mut(id) else { return };
        if tab.url == url.as_str() {
            return;
        }
        if url::Url::parse(&tab.url).is_ok_and(|old| old.origin() != url.origin()) {
            tab.favicon = None;
            tab.favicon_revision = tab.favicon_revision.wrapping_add(1);
        }
        tab.url = url.to_string();
        if !same_document {
            tab.title = urls::title_for_url(url.as_str());
        }
        if record && !tab.private {
            let title = tab.title.clone();
            if store.push_history(url.as_str(), &title) {
                if let Some(entry) = store.history.last() {
                    state.save_visit(entry, store.history.first());
                }
            }
        }
        state.save_tabs(&store);
    }
    sync(app);
}

pub fn update_title(app: &AppHandle, id: u32, url: &url::Url, title: &str) {
    if !urls::is_web(url) || title.trim().is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    {
        let mut store = state.store_guard();
        let Some(tab) = store.tab_mut(id) else { return };
        if tab.url != url.as_str() || tab.failed {
            return;
        }
        let title: String = title.trim().chars().take(300).collect();
        if tab.title == title {
            return;
        }
        tab.title.clone_from(&title);
        let private = tab.private;
        if let Some(entry) = store
            .history
            .iter_mut()
            .rev()
            .find(|entry| !private && entry.url == url.as_str())
        {
            entry.title = title;
            state.save_visit(entry, None);
        }
        state.save_tabs(&store);
    }
    sync(app);
}

pub fn set_loading(app: &AppHandle, id: u32, loading: bool) {
    let state = app.state::<AppState>();
    {
        let mut store = state.store_guard();
        let Some(tab) = store.tab_mut(id) else { return };
        if tab.loading == loading && (!loading || !tab.failed) {
            return;
        }
        tab.loading = loading;
        if loading {
            tab.failed = false;
        }
    }
    sync(app);
}

pub fn completed(app: &AppHandle, id: u32, url: &url::Url, success: bool) {
    update_source(app, id, url, false, false);
    let state = app.state::<AppState>();
    let mut active = false;
    let mut private = false;
    {
        let mut store = state.store_guard();
        let active_id = store.tabs.active_id;
        if let Some(tab) = store.tab_mut(id) {
            tab.loading = false;
            tab.failed = !success;
            active = active_id == id;
            private = tab.private;
            let title = tab.title.clone();
            if success && !private && store.push_history(url.as_str(), &title) {
                if let Some(entry) = store.history.last() {
                    state.save_visit(entry, store.history.first());
                }
            }
        }
    }
    sync(app);
    if success && !private && urls::is_web(url) {
        let tab_label = format!("tab-{id}");
        if let Some(view) = app.get_webview(&tab_label) {
            let script = state.plugins_guard().script().to_owned();
            if !script.is_empty() {
                let _ = view.eval(&script);
            }
        }
    } else if !success && active {
        emit(app, "au://notice", "Не удалось загрузить страницу");
    }
}

pub fn history_changed(app: &AppHandle, id: u32, back: bool, forward: bool) {
    {
        let state = app.state::<AppState>();
        let mut store = state.store_guard();
        let Some(tab) = store.tab_mut(id) else { return };
        if tab.can_back == back && tab.can_forward == forward {
            return;
        }
        tab.can_back = back;
        tab.can_forward = forward;
    }
    sync(app);
}

pub fn source_changed(app: &AppHandle, id: u32, url: &url::Url, same_document: bool) {
    update_source(app, id, url, same_document, same_document);
}

pub fn cancel_loading(app: &AppHandle, id: u32) {
    set_loading(app, id, false);
}

pub fn media_fullscreen(app: &AppHandle, tab_id: u32, entering: bool) {
    use std::sync::atomic::Ordering::Relaxed;

    let state = app.state::<AppState>();
    let Some(window) = app.get_window("main") else {
        return;
    };
    if entering {
        if state.store_guard().tabs.active_id != tab_id {
            return;
        }
        if state.media_fullscreen.swap(tab_id, Relaxed) == 0 {
            state
                .pre_media_fullscreen
                .store(window.is_fullscreen().unwrap_or_default(), Relaxed);
        }
        let _ = window.set_fullscreen(true);
    } else {
        if tab_id == 0
            || state
                .media_fullscreen
                .compare_exchange(tab_id, 0, Relaxed, Relaxed)
                .is_err()
        {
            return;
        }
        let _ = window.set_fullscreen(state.pre_media_fullscreen.load(Relaxed));
    }
    let _ = layout::relayout(app);
}

pub fn is_private(app: &AppHandle, id: u32) -> bool {
    let state = app.state::<AppState>();
    let store = state.store_guard();
    store
        .tabs
        .tabs
        .iter()
        .any(|tab| tab.id == id && tab.private)
}

pub fn zoom_changed(app: &AppHandle, id: u32, zoom: f64) {
    {
        let state = app.state::<AppState>();
        let mut store = state.store_guard();
        let Some(tab) = store.tab_mut(id) else { return };
        tab.zoom = Some(zoom);
    }
    sync(app);
}

pub fn ensure_view(app: &AppHandle, tab: &Tab, start: &str) -> Result<Webview, String> {
    if let Some(view) = app.get_webview(&tab.label) {
        return Ok(view);
    }
    let window = app.get_window("main").ok_or("Окно было закрыто")?;
    let (width, height, strip, _) = layout::metrics(app).map_err(|error| error.to_string())?;
    let url = url::Url::parse(start).map_err(|error| error.to_string())?;
    let state = app.state::<AppState>();
    let builder = tauri::webview::WebviewBuilder::new(&tab.label, WebviewUrl::External(url))
        .on_navigation(urls::can_visit)
        .incognito(tab.private);
    #[cfg(windows)]
    let builder = builder.browser_extensions_enabled(true);
    #[cfg(not(windows))]
    let builder = builder.on_page_load({
        let id = tab.id;
        move |view, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) {
                set_loading(view.app_handle(), id, true);
            } else if !cfg!(windows) {
                completed(view.app_handle(), id, payload.url(), true);
            }
        }
    });

    // A separate test or custom profile includes the engine's cookies and storage as well.
    let builder = if std::env::var_os("AURORA_DATA_DIR").is_some()
        || !app.state::<crate::profiles::ActiveProfile>().id.is_empty()
    {
        builder.data_directory(state.data_dir.join("WebView"))
    } else {
        builder
    };
    #[cfg(not(windows))]
    let builder = builder
        .on_new_window({
            let app = app.clone();
            let private = tab.private;
            move |url, _| {
                if urls::is_web(&url) {
                    defer(&app, move |app| {
                        let _ = new_tab(&app, Some(url.into()), None, private);
                    });
                }
                tauri::webview::NewWindowResponse::Deny
            }
        })
        .on_document_title_changed({
            let id = tab.id;
            move |view, title| {
                if let Ok(url) = view.url() {
                    update_title(view.app_handle(), id, &url, &title);
                }
            }
        });

    let view = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(0.0, strip),
            tauri::LogicalSize::new(width, (height - strip).max(1.0)),
        )
        .map_err(|error| format!("Не удалось открыть вкладку. {error}"))?;
    #[cfg(windows)]
    if let Err(error) = crate::windows::install(&view, tab.id) {
        let _ = view.close();
        return Err(error.to_string());
    }

    Ok(view)
}

pub fn new_tab(
    app: &AppHandle,
    input: Option<String>,
    initial_url: Option<&str>,
    private: bool,
) -> Result<Webview, String> {
    let state = app.state::<AppState>();
    let tab = {
        let mut store = state.store_guard();
        let url = input
            .as_deref()
            .map(|input| urls::resolve_input(input, &store.settings))
            .unwrap_or_else(|| {
                if private {
                    urls::page("private")
                } else {
                    store.settings.homepage.clone()
                }
            });
        let mut tab = store.allocate_tab(url);
        tab.loading = true;
        tab.private = private;
        store.tabs.tabs.push(tab.clone());
        tab
    };
    let view = match ensure_view(app, &tab, initial_url.unwrap_or(&tab.url)) {
        Ok(view) => view,
        Err(error) => {
            state.store_guard().remove_tab(tab.id);
            return Err(error);
        }
    };
    activate(app, tab.id, false)?;
    Ok(view)
}

pub fn activate(app: &AppHandle, id: u32, focus: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (tab, previous) = {
        let store = state.store_guard();
        let tab = store
            .tabs
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .cloned()
            .ok_or("Вкладка закрыта")?;
        (tab, store.tabs.active_id)
    };
    let view = ensure_view(app, &tab, &tab.url)?;
    if previous != id {
        media_fullscreen(app, previous, false);
    }
    {
        let mut store = state.store_guard();
        store.tabs.active_id = id;
        state.save_tabs(&store);
    }
    let _ = layout::relayout(app);
    if focus {
        let _ = view.set_focus();
    }
    sync(app);
    Ok(())
}

pub fn close(app: &AppHandle, id: u32) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (tab, next, was_active) = {
        let store = state.store_guard();
        let Some(index) = store.tabs.tabs.iter().position(|tab| tab.id == id) else {
            return Ok(());
        };
        let was_active = store.tabs.active_id == id;
        let next = if was_active {
            store
                .tabs
                .tabs
                .get(index + 1)
                .or_else(|| index.checked_sub(1).and_then(|i| store.tabs.tabs.get(i)))
        } else {
            store.active()
        };
        (store.tabs.tabs[index].clone(), next.cloned(), was_active)
    };
    if was_active {
        if let Some(next) = &next {
            ensure_view(app, next, &next.url)?;
        } else {
            new_tab(app, None, None, false)?;
        }
        media_fullscreen(app, id, false);
    }
    if let Some(view) = app.get_webview(&tab.label) {
        view.close().map_err(|error| error.to_string())?;
    }
    #[cfg(windows)]
    crate::windows::forget_popup(id);
    {
        let mut store = state.store_guard();
        if let Some(tab) = store.remove_tab(id) {
            store.remember_closed(tab);
            state.save_tabs(&store);
        }
    }
    let _ = layout::relayout(app);
    sync(app);
    Ok(())
}

pub fn reopen(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let url = state
        .store_guard()
        .last_closed()
        .map(|tab| tab.url.clone())
        .ok_or("Нет закрытых вкладок")?;
    new_tab(app, Some(url), None, false)?;
    state.store_guard().forget_last_closed();
    sync(app);
    Ok(())
}

pub fn visit(app: &AppHandle, input: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (tab, target) = {
        let store = state.store_guard();
        let tab = store.active().cloned().ok_or("Нет активной вкладки")?;
        let target = urls::resolve_input(input, &store.settings);
        (tab, target)
    };
    let url = url::Url::parse(&target).map_err(|error| error.to_string())?;
    let view = ensure_view(app, &tab, &target)?;
    view.navigate(url).map_err(|error| error.to_string())?;
    let _ = view.set_focus();
    Ok(())
}

pub fn refresh_internal(app: &AppHandle) {
    if let Some(view) = active_webview(app) {
        if view.url().is_ok_and(|url| urls::is_internal(url.as_str())) {
            let _ = view.eval("location.reload()");
        }
    }
}

fn home_webviews(app: &AppHandle) -> impl Iterator<Item = Webview> {
    app.webviews().into_iter().filter_map(|(label, view)| {
        (label.starts_with("tab-")
            && view
                .url()
                .ok()
                .and_then(|url| urls::internal_path(url.as_str()))
                .is_some_and(|path| matches!(path.as_str(), "" | "home")))
        .then_some(view)
    })
}

pub fn refresh_home(app: &AppHandle) {
    for view in home_webviews(app) {
        let _ = view.eval("document.dispatchEvent(new Event('aurora-bookmarks-changed'))");
    }
}

pub fn set_wallpaper(app: &AppHandle, enabled: bool) {
    let script = format!(
        "document.getElementById('live-wallpaper')?.setAttribute('data-enabled', '{enabled}')"
    );
    for view in home_webviews(app) {
        let _ = view.eval(&script);
    }
}
