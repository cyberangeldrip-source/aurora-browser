use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use tauri::{Manager, Webview};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{take_pwstr, *};
use windows::core::{Interface, BOOL, PWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
};

use crate::{browser, core::urls};

fn source(view: &ICoreWebView2) -> Option<url::Url> {
    let mut raw = PWSTR::null();
    // WebView2 allocates the returned string with CoTaskMemAlloc.
    unsafe { view.Source(&mut raw).ok()? };
    url::Url::parse(&take_pwstr(raw)).ok()
}

fn title(view: &ICoreWebView2) -> windows::core::Result<String> {
    let mut raw = PWSTR::null();
    unsafe { view.DocumentTitle(&mut raw)? };
    Ok(take_pwstr(raw))
}

pub fn install(view: &Webview, id: u32) -> tauri::Result<()> {
    let app = view.app_handle().clone();
    view.with_webview(move |platform| {
        // The callbacks and controller stay on WebView2's STA UI thread.
        let result = unsafe { attach_events(platform.controller(), &app, id) };
        if let Err(error) = result {
            eprintln!("Aurora: tab {id}: {error}");
        }
    })
}

unsafe fn attach_events(
    controller: ICoreWebView2Controller,
    app: &tauri::AppHandle,
    id: u32,
) -> windows::core::Result<()> {
    let view = controller.CoreWebView2()?;
    let mut token = 0;

    let navigation = Rc::new(Cell::new(0));
    let changed_document = Rc::new(Cell::new(false));
    let starting_navigation = navigation.clone();
    let starting_document = changed_document.clone();
    let start_app = app.clone();
    view.add_NavigationStarting(
        &NavigationStartingEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                let mut navigation_id = 0;
                args.NavigationId(&mut navigation_id)?;
                if starting_navigation.replace(navigation_id) != navigation_id {
                    starting_document.set(false);
                }
                browser::set_loading(&start_app, id, true);
            }
            Ok(())
        })),
        &mut token,
    )?;

    let address_app = app.clone();
    let source_document = changed_document.clone();
    view.add_SourceChanged(
        &SourceChangedEventHandler::create(Box::new(move |sender, args| {
            if let (Some(sender), Some(args)) = (sender, args) {
                if let Some(url) = source(&sender) {
                    let mut new_document = BOOL::default();
                    args.IsNewDocument(&mut new_document)?;
                    if new_document.as_bool() {
                        source_document.set(true);
                    }
                    browser::source_changed(&address_app, id, &url, !new_document.as_bool());
                }
            }
            Ok(())
        })),
        &mut token,
    )?;

    let history_app = app.clone();
    view.add_HistoryChanged(
        &HistoryChangedEventHandler::create(Box::new(move |sender, _| {
            if let Some(sender) = sender {
                let (mut back, mut forward) = (BOOL::default(), BOOL::default());
                sender.CanGoBack(&mut back)?;
                sender.CanGoForward(&mut forward)?;
                browser::history_changed(&history_app, id, back.as_bool(), forward.as_bool());
            }
            Ok(())
        })),
        &mut token,
    )?;

    let title_app = app.clone();
    view.add_DocumentTitleChanged(
        &DocumentTitleChangedEventHandler::create(Box::new(move |sender, _| {
            if let Some(sender) = sender {
                if let Some(url) = source(&sender) {
                    browser::update_title(&title_app, id, &url, &title(&sender)?);
                }
            }
            Ok(())
        })),
        &mut token,
    )?;

    let load_app = app.clone();
    view.add_NavigationCompleted(
        &NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
            if let (Some(sender), Some(args)) = (sender, args) {
                let mut navigation_id = 0;
                args.NavigationId(&mut navigation_id)?;
                if navigation.get() != 0 && navigation.get() != navigation_id {
                    return Ok(());
                }
                let mut success = BOOL::default();
                args.IsSuccess(&mut success)?;
                if let Some(url) = source(&sender) {
                    if success.as_bool() {
                        browser::update_title(&load_app, id, &url, &title(&sender)?);
                        browser::completed(&load_app, id, &url, true);
                    } else {
                        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                        args.WebErrorStatus(&mut status)?;
                        if status == COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED
                            || (status == COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN
                                && !changed_document.get())
                        {
                            browser::cancel_loading(&load_app, id);
                        } else {
                            browser::completed(&load_app, id, &url, false);
                        }
                    }
                }
            }
            Ok(())
        })),
        &mut token,
    )?;

    let zoom_app = app.clone();
    controller.add_ZoomFactorChanged(
        &ZoomFactorChangedEventHandler::create(Box::new(move |sender, _| {
            if let Some(sender) = sender {
                let mut zoom = 1.0;
                sender.ZoomFactor(&mut zoom)?;
                browser::zoom_changed(&zoom_app, id, zoom);
            }
            Ok(())
        })),
        &mut token,
    )?;

    let media_app = app.clone();
    view.add_ContainsFullScreenElementChanged(
        &ContainsFullScreenElementChangedEventHandler::create(Box::new(move |sender, _| {
            if let Some(sender) = sender {
                let mut fullscreen = BOOL::default();
                sender.ContainsFullScreenElement(&mut fullscreen)?;
                browser::defer(&media_app, move |app| {
                    browser::media_fullscreen(&app, id, fullscreen.as_bool())
                });
            }
            Ok(())
        })),
        &mut token,
    )?;

    attach_shortcuts(&controller, app, false)?;
    attach_popups(&view, app, id, &mut token)?;
    attach_downloads(&view, app, id, &mut token)?;
    if let Ok(icons) = view.cast::<ICoreWebView2_15>() {
        let icon_app = app.clone();
        let _ = icons.add_FaviconChanged(
            &FaviconChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(sender) = sender {
                    capture_site_icon(&sender, &icon_app, id);
                }
                Ok(())
            })),
            &mut token,
        );
    }
    Ok(())
}

unsafe fn attach_shortcuts(
    controller: &ICoreWebView2Controller,
    app: &tauri::AppHandle,
    downloads_only: bool,
) -> windows::core::Result<()> {
    let keyboard_app = app.clone();
    controller.add_AcceleratorKeyPressed(
        &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
            args.KeyEventKind(&mut kind)?;
            if !matches!(
                kind,
                COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN | COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
            ) {
                return Ok(());
            }
            let mut key = 0;
            args.VirtualKey(&mut key)?;
            let ctrl = GetKeyState(i32::from(VK_CONTROL.0)) < 0;
            let alt = GetKeyState(i32::from(VK_MENU.0)) < 0;
            let shift = GetKeyState(i32::from(VK_SHIFT.0)) < 0;
            let Some(action) = shortcut(key, ctrl, alt, shift) else {
                return Ok(());
            };
            if downloads_only && action != "downloads" {
                return Ok(());
            }
            args.SetHandled(true)?;
            let mut physical = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
            args.PhysicalKeyStatus(&mut physical)?;
            if !physical.WasKeyDown.as_bool() {
                browser::defer(&keyboard_app, move |app| {
                    if matches!(action, "address" | "settings" | "tab-list") {
                        if let Some(chrome) = app.get_webview("ui") {
                            let _ = chrome.set_focus();
                        }
                    }
                    browser::emit(&app, "au://shortcut", action);
                });
            }
            Ok(())
        })),
        &mut 0,
    )
}

fn capture_site_icon(view: &ICoreWebView2, app: &tauri::AppHandle, id: u32) {
    use crate::core::store::AppState;
    use crate::pages::site_icons;

    let Some(url) = source(view) else { return };
    if !urls::is_web(&url) {
        return;
    }
    let Ok(native) = view.cast::<ICoreWebView2_15>() else {
        return;
    };
    let app = app.clone();
    let callback = GetFaviconCompletedHandler::create(Box::new(move |result, stream| {
        result?;
        if let Some(stream) = stream {
            let mut bytes = vec![0; site_icons::MAX_ICON_BYTES + 1];
            let mut copied = 0;
            unsafe {
                stream
                    .Read(
                        bytes.as_mut_ptr().cast(),
                        bytes.len() as u32,
                        Some(&mut copied),
                    )
                    .ok()?
            };
            bytes.truncate(copied as usize);
            if !site_icons::is_png(&bytes) {
                return Ok(());
            }
            let state = app.state::<AppState>();
            let bytes: std::sync::Arc<[u8]> = bytes.into();
            let private = {
                let mut store = state.store_guard();
                let Some(tab) = store.tab_mut(id) else {
                    return Ok(());
                };
                if tab.url != url.as_str() || tab.favicon.as_deref() == Some(&*bytes) {
                    return Ok(());
                }
                tab.favicon = Some(bytes.clone());
                tab.favicon_revision = tab.favicon_revision.wrapping_add(1);
                tab.private
            };
            browser::sync(&app);
            if !private {
                if let Some(path) = site_icons::cache_path(&state.data_dir, &url) {
                    tauri::async_runtime::spawn_blocking(move || site_icons::store(&path, &bytes));
                }
            }
        }
        Ok(())
    }));
    let _ = unsafe { native.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &callback) };
}

pub fn capture_bookmarked_page(view: &Webview, id: u32) {
    let app = view.app_handle().clone();
    let _ = view.with_webview(move |platform| {
        if let Ok(view) = unsafe { platform.controller().CoreWebView2() } {
            capture_site_icon(&view, &app, id);
        }
    });
}

fn shortcut(key: u32, ctrl: bool, alt: bool, shift: bool) -> Option<&'static str> {
    match (key, ctrl, alt, shift) {
        (0x4C, true, false, false) | (0x44, false, true, false) => Some("address"),
        (0x54, true, false, false) => Some("newtab"),
        (0x54, true, false, true) => Some("reopen"),
        (0x4E, true, false, true) => Some("private"),
        (0x57, true, false, false) => Some("close"),
        (0x52, true, false, _) | (0x74, false, false, _) => Some("reload"),
        (0x44, true, false, false) => Some("bookmark"),
        (0x48, true, false, false) => Some("history"),
        (0x4A, true, false, false) => Some("downloads"),
        (0xBC, true, false, false) => Some("settings"),
        (0x09, true, false, false) => Some("next-tab"),
        (0x09, true, false, true) => Some("previous-tab"),
        (0x31..=0x39, true, false, false) => Some(
            [
                "tab-1", "tab-2", "tab-3", "tab-4", "tab-5", "tab-6", "tab-7", "tab-8", "tab-9",
            ][(key - 0x31) as usize],
        ),
        (0x25, false, true, false) => Some("back"),
        (0x27, false, true, false) => Some("forward"),
        (0x7A, false, false, false) => Some("fullscreen"),
        (0x49, true, false, true) | (0x7B, false, false, false) => Some("devtools"),
        _ => None,
    }
}

type PopupRequest = (
    ICoreWebView2NewWindowRequestedEventArgs,
    ICoreWebView2Deferral,
);

thread_local! {
    static NEXT_POPUP: Cell<u64> = const { Cell::new(0) };
    static PENDING_POPUPS: RefCell<HashMap<u64, PopupRequest>> = RefCell::new(HashMap::new());
}

unsafe fn attach_popups(
    view: &ICoreWebView2,
    app: &tauri::AppHandle,
    opener_id: u32,
    token: &mut i64,
) -> windows::core::Result<()> {
    let popup_app = app.clone();
    view.add_NewWindowRequested(
        &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut initiated = BOOL::default();
            args.IsUserInitiated(&mut initiated)?;
            if !initiated.as_bool() {
                return Ok(());
            }
            let mut raw = PWSTR::null();
            args.Uri(&mut raw)?;
            let raw = take_pwstr(raw);
            let Ok(url) = url::Url::parse(&raw) else {
                return Ok(());
            };
            if !urls::is_web(&url) && raw != "about:blank" {
                return Ok(());
            }
            let deferral = args.GetDeferral()?;
            let key = NEXT_POPUP.with(|next| {
                let key = next.get();
                next.set(key.wrapping_add(1));
                key
            });
            PENDING_POPUPS.with_borrow_mut(|pending| pending.insert(key, (args, deferral)));
            browser::defer(&popup_app, move |app| {
                complete_popup(&app, opener_id, raw, key)
            });
            Ok(())
        })),
        token,
    )?;

    let close_app = app.clone();
    view.add_WindowCloseRequested(
        &WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
            // Only windows created by web content may be closed by the site.
            if is_popup(opener_id) {
                browser::defer(&close_app, move |app| {
                    let _ = browser::close(&app, opener_id);
                });
            }
            Ok(())
        })),
        token,
    )
}

thread_local! {
    static POPUPS: RefCell<std::collections::HashSet<u32>> = RefCell::new(std::collections::HashSet::new());
}

fn is_popup(id: u32) -> bool {
    POPUPS.with_borrow(|popups| popups.contains(&id))
}

pub fn forget_popup(id: u32) {
    POPUPS.with_borrow_mut(|popups| {
        popups.remove(&id);
    });
}

fn complete_popup(app: &tauri::AppHandle, opener_id: u32, address: String, key: u64) {
    if app.get_webview(&format!("tab-{opener_id}")).is_some() {
        let private = browser::is_private(app, opener_id);
        match browser::new_tab(app, Some(address.clone()), Some("about:blank"), private) {
            Ok(view) => {
                if let Ok(id) = view.label().trim_start_matches("tab-").parse() {
                    POPUPS.with_borrow_mut(|popups| {
                        popups.insert(id);
                    });
                }
                let fallback = view.clone();
                if view
                    .with_webview(move |platform| unsafe {
                        let Some((args, deferral)) =
                            PENDING_POPUPS.with_borrow_mut(|pending| pending.remove(&key))
                        else {
                            return;
                        };
                        let attached = platform
                            .controller()
                            .CoreWebView2()
                            .and_then(|native| args.SetNewWindow(&native));
                        let _ = deferral.Complete();
                        if attached.is_err() {
                            if let Ok(url) = url::Url::parse(&address) {
                                let _ = fallback.navigate(url);
                            }
                        }
                        let _ = fallback.set_focus();
                    })
                    .is_ok()
                {
                    return;
                }
            }
            Err(message) => browser::emit(app, "au://notice", message),
        }
    }
    if let Some((_, deferral)) = PENDING_POPUPS.with_borrow_mut(|pending| pending.remove(&key)) {
        let _ = unsafe { deferral.Complete() };
    }
}

thread_local! {
    static ACTIVE_DOWNLOADS: RefCell<HashMap<u64, ICoreWebView2DownloadOperation>> = RefCell::new(HashMap::new());
}

unsafe fn attach_downloads(
    view: &ICoreWebView2,
    app: &tauri::AppHandle,
    tab_id: u32,
    token: &mut i64,
) -> windows::core::Result<()> {
    use crate::core::store::AppState;

    let view = view.cast::<ICoreWebView2_4>()?;
    let app = app.clone();
    view.add_DownloadStarting(
        &DownloadStartingEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let operation = args.DownloadOperation()?;
            let path = download_path(&operation).unwrap_or_default();
            let (_, total) = download_size(&operation);
            let state = app.state::<AppState>();
            let private = browser::is_private(&app, tab_id);
            let id = state.downloads_guard().start(path, total, private);
            let progress_app = app.clone();
            operation.add_BytesReceivedChanged(
                &BytesReceivedChangedEventHandler::create(Box::new(move |operation, _| {
                    if let Some(operation) = operation {
                        let (received, total) = download_size(&operation);
                        progress_app
                            .state::<AppState>()
                            .downloads_guard()
                            .progress(id, received, total);
                    }
                    Ok(())
                })),
                &mut 0,
            )?;
            let status_app = app.clone();
            operation.add_StateChanged(
                &StateChangedEventHandler::create(Box::new(move |operation, _| {
                    if let Some(operation) = operation {
                        update_download(&status_app, id, &operation)?;
                    }
                    Ok(())
                })),
                &mut 0,
            )?;
            ACTIVE_DOWNLOADS.with_borrow_mut(|items| items.insert(id, operation));
            args.SetHandled(true)?;
            state.save_downloads();
            browser::emit(&app, "au://downloads", ());
            browser::emit(&app, "au://notice", "Загрузка началась");
            Ok(())
        })),
        token,
    )
}

fn download_path(operation: &ICoreWebView2DownloadOperation) -> Option<std::path::PathBuf> {
    let mut path = PWSTR::null();
    unsafe { operation.ResultFilePath(&mut path).ok()? };
    let path = std::path::PathBuf::from(take_pwstr(path));
    path.is_absolute().then_some(path)
}

fn download_size(operation: &ICoreWebView2DownloadOperation) -> (u64, Option<u64>) {
    let mut received = 0;
    let mut total = -1;
    unsafe {
        let _ = operation.BytesReceived(&mut received);
        let _ = operation.TotalBytesToReceive(&mut total);
    }
    (received.max(0) as u64, u64::try_from(total).ok())
}

fn update_download(
    app: &tauri::AppHandle,
    id: u64,
    operation: &ICoreWebView2DownloadOperation,
) -> windows::core::Result<()> {
    use crate::core::store::AppState;
    use crate::downloads::Status;

    let mut native_status = COREWEBVIEW2_DOWNLOAD_STATE::default();
    unsafe { operation.State(&mut native_status)? };
    let mut can_resume = BOOL::default();
    let status = match native_status {
        COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED => Status::Complete,
        COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED => {
            let mut reason = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON::default();
            unsafe {
                let _ = operation.InterruptReason(&mut reason);
                let _ = operation.CanResume(&mut can_resume);
            }
            match reason {
                COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_PAUSED => Status::Paused,
                COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED => Status::Canceled,
                _ => Status::Interrupted,
            }
        }
        _ => Status::Running,
    };
    let can_resume = can_resume.as_bool() && status != Status::Canceled;
    let state = app.state::<AppState>();
    let (received, total) = download_size(operation);
    {
        let mut downloads = state.downloads_guard();
        downloads.progress(id, received, total);
        downloads.update(id, status, can_resume, download_path(operation));
    }
    if status != Status::Running && status != Status::Paused && !can_resume {
        ACTIVE_DOWNLOADS.with_borrow_mut(|items| items.remove(&id));
    }
    state.save_downloads();
    browser::emit(app, "au://downloads", ());
    match status {
        Status::Complete => browser::emit(app, "au://notice", "Файл загружен"),
        Status::Interrupted => browser::emit(app, "au://notice", "Загрузка прервана"),
        _ => {}
    }
    Ok(())
}

pub fn control_download(id: u64, action: &str) -> std::result::Result<(), String> {
    let operation = ACTIVE_DOWNLOADS
        .with_borrow(|items| items.get(&id).cloned())
        .ok_or("Загрузка уже завершилась")?;
    unsafe {
        match action {
            "pause" => operation.Pause(),
            "resume" => operation.Resume(),
            "cancel" => operation.Cancel(),
            _ => return Err("Неизвестное действие".into()),
        }
    }
    .map_err(|error| error.to_string())
}

pub fn install_chrome_downloads(view: &Webview) {
    let app = view.app_handle().clone();
    let _ = view.with_webview(move |platform| unsafe {
        let controller = platform.controller();
        let _ = attach_shortcuts(&controller, &app, true);
        if let Ok(view) = controller.CoreWebView2() {
            let _ = attach_downloads(&view, &app, 0, &mut 0);
        }
    });
}

pub fn action(view: &Webview, action: &str) -> tauri::Result<()> {
    let action = action.to_owned();
    let app = view.app_handle().clone();
    view.with_webview(move |platform| unsafe {
        let result =
            platform
                .controller()
                .CoreWebView2()
                .and_then(|native| match action.as_str() {
                    "back" => native.GoBack(),
                    "forward" => native.GoForward(),
                    "reload" => native.Reload(),
                    "stop" => native.Stop(),
                    _ => Ok(()),
                });
        if let Err(error) = result {
            browser::emit(&app, "au://notice", error.to_string());
        }
    })
}

pub fn raise(view: &Webview) {
    let _ = view.with_webview(|platform| unsafe {
        let mut container = HWND::default();
        if platform.controller().ParentWindow(&mut container).is_ok() && !container.is_invalid() {
            let _ = SetWindowPos(
                container,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    });
}

pub fn capture_glass_frame(view: &Webview, on_image: impl FnOnce(&str) + Send + 'static) {
    let _ = view.with_webview(move |platform| unsafe {
        let Ok(native) = platform.controller().CoreWebView2() else {
            return;
        };
        let callback =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, payload| {
                if result.is_ok() {
                    #[derive(serde::Deserialize)]
                    struct Screenshot<'a> {
                        data: &'a str,
                    }
                    if let Ok(frame) = serde_json::from_str::<Screenshot<'_>>(&payload) {
                        on_image(frame.data);
                    }
                }
                Ok(())
            }));
        // Sample the WebView surface once when the chrome opens a lens.
        let _ = native.CallDevToolsProtocolMethod(
            windows::core::w!("Page.captureScreenshot"),
            windows::core::w!(r#"{"format":"jpeg","quality":75,"captureBeyondViewport":false}"#),
            &callback,
        );
    });
}
