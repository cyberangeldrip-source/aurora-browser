use serde::Serialize;
use tauri::Webview;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    take_pwstr, BrowserExtensionEnableCompletedHandler, BrowserExtensionRemoveCompletedHandler,
    ProfileAddBrowserExtensionCompletedHandler, ProfileGetBrowserExtensionsCompletedHandler,
};
use windows::core::{Interface, BOOL, HSTRING, PWSTR};

#[derive(Serialize)]
pub struct Extension {
    id: String,
    name: String,
    enabled: bool,
}

unsafe fn describe(item: &ICoreWebView2BrowserExtension) -> windows::core::Result<Extension> {
    let mut value = PWSTR::null();
    item.Id(&mut value)?;
    let id = take_pwstr(value);
    item.Name(&mut value)?;
    let name = take_pwstr(value);
    let mut enabled = BOOL::default();
    item.IsEnabled(&mut enabled)?;
    Ok(Extension {
        id,
        name,
        enabled: enabled.as_bool(),
    })
}

pub fn folder(raw: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(raw.trim().trim_matches('"'));
    if !path.is_absolute() {
        return Err("Укажите полный путь к папке расширения".into());
    }
    let path = path
        .canonicalize()
        .map_err(|_| "Папка расширения не найдена")?;
    let manifest = path.join("manifest.json");
    let meta = std::fs::metadata(&manifest).map_err(|_| "В папке нет manifest.json")?;
    if !meta.is_file() || meta.len() > 1024 * 1024 {
        return Err("Недопустимый manifest.json".into());
    }
    Ok(path)
}

pub enum Request {
    List,
    Install(std::path::PathBuf),
    Enable(String, bool),
    Remove(String),
}

pub async fn request(view: Webview, request: Request) -> Result<Vec<Extension>, String> {
    let (send, mut receive) = tauri::async_runtime::channel(1);
    view.with_webview(move |platform| unsafe {
        let response = send.clone();
        let result = (|| -> windows::core::Result<()> {
            let profile = platform
                .controller()
                .CoreWebView2()?
                .cast::<ICoreWebView2_13>()?
                .Profile()?
                .cast::<ICoreWebView2Profile7>()?;
            if let Request::Install(path) = request {
                let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(
                    move |result, extension| {
                        let result = result
                            .and_then(|_| extension.as_ref().map(describe_owned).transpose())
                            .map(|item| item.into_iter().collect())
                            .map_err(|error| error.to_string());
                        let _ = send.try_send(result);
                        Ok(())
                    },
                ));
                return profile.AddBrowserExtension(&HSTRING::from(path.as_os_str()), &handler);
            }
            let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
                move |result, list| {
                    let outcome = (|| -> windows::core::Result<()> {
                        result?;
                        let Some(list) = list else {
                            let _ = send.try_send(Ok(Vec::new()));
                            return Ok(());
                        };
                        let mut count = 0;
                        list.Count(&mut count)?;
                        let mut extensions = Vec::new();
                        for index in 0..count {
                            let item = list.GetValueAtIndex(index)?;
                            let information = describe(&item)?;
                            match &request {
                                Request::Enable(id, enabled) if *id == information.id => {
                                    let done = send.clone();
                                    let cb = BrowserExtensionEnableCompletedHandler::create(
                                        Box::new(move |status| {
                                            let _ = done.try_send(
                                                status
                                                    .map(|_| Vec::new())
                                                    .map_err(|e| e.to_string()),
                                            );
                                            Ok(())
                                        }),
                                    );
                                    return item.Enable(*enabled, &cb);
                                }
                                Request::Remove(id) if *id == information.id => {
                                    let done = send.clone();
                                    let cb = BrowserExtensionRemoveCompletedHandler::create(
                                        Box::new(move |status| {
                                            let _ = done.try_send(
                                                status
                                                    .map(|_| Vec::new())
                                                    .map_err(|e| e.to_string()),
                                            );
                                            Ok(())
                                        }),
                                    );
                                    return item.Remove(&cb);
                                }
                                _ => extensions.push(information),
                            }
                        }
                        let _ = send.try_send(if matches!(request, Request::List) {
                            Ok(extensions)
                        } else {
                            Err("Расширение не найдено".into())
                        });
                        Ok(())
                    })();
                    if let Err(error) = outcome {
                        let _ = send.try_send(Err(error.to_string()));
                    }
                    Ok(())
                },
            ));
            profile.GetBrowserExtensions(&handler)
        })();
        if let Err(error) = result {
            let _ = response.try_send(Err(error.to_string()));
        }
    })
    .map_err(|error| error.to_string())?;
    receive.recv().await.ok_or("WebView2 не ответил")?
}

fn describe_owned(item: &ICoreWebView2BrowserExtension) -> windows::core::Result<Extension> {
    unsafe { describe(item) }
}
