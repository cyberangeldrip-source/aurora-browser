mod render;
pub mod site_icons;

use tauri::http::{header, Method, Request, Response, StatusCode};
use tauri::{AppHandle, Manager};

use crate::core::{store::AppState, urls};

const INTERNAL_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; frame-src 'none'; base-uri 'none'; object-src 'none'; form-action 'self'";

fn response(status: StatusCode, media_type: &str, body: Vec<u8>, cache: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, media_type)
        .header(header::CACHE_CONTROL, cache)
        .header(header::CONTENT_SECURITY_POLICY, INTERNAL_CSP)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::X_FRAME_OPTIONS, "DENY")
        .header(header::REFERRER_POLICY, "no-referrer")
        .body(body)
        .expect("valid internal response")
}

fn asset(media_type: &str, body: &[u8]) -> Response<Vec<u8>> {
    response(
        StatusCode::OK,
        media_type,
        body.to_vec(),
        "public, max-age=3600",
    )
}

fn json(status: StatusCode, value: serde_json::Value) -> Response<Vec<u8>> {
    response(
        status,
        "application/json; charset=utf-8",
        value.to_string().into_bytes(),
        "no-store",
    )
}

pub fn handle(app: &AppHandle, webview: &str, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let path = request.uri().path().trim_matches('/');
    if path == "do" {
        return action(app, &request);
    }
    if request.method() != Method::GET {
        return json(
            StatusCode::METHOD_NOT_ALLOWED,
            serde_json::json!({ "ok": false }),
        );
    }
    if webview == "ui" {
        if let Some(id) = path
            .strip_prefix("res/tab-icon/")
            .and_then(|id| id.parse::<u32>().ok())
        {
            let state = app.state::<AppState>();
            let (tab, light) = {
                let store = state.store_guard();
                let tab = store.tabs.tabs.iter().find(|tab| tab.id == id);
                (
                    tab.map(|tab| (tab.url.clone(), tab.favicon.clone())),
                    store.settings.theme == "light",
                )
            };
            if let Some((address, image)) = tab {
                let (media_type, bytes) = image.map_or_else(
                    || {
                        let (media_type, bytes) =
                            site_icons::image(&state.data_dir, &address, light);
                        (media_type, bytes.into_owned())
                    },
                    |bytes| ("image/png", bytes.to_vec()),
                );
                return response(StatusCode::OK, media_type, bytes, "no-store");
            }
            return response(StatusCode::NOT_FOUND, "text/plain", Vec::new(), "no-store");
        }
    }
    if let Some((collection, id)) = path
        .strip_prefix("res/")
        .and_then(|path| path.split_once('/'))
        .filter(|(collection, _)| matches!(*collection, "favorite" | "history-icon"))
        .and_then(|(collection, id)| Some((collection, id.parse::<u64>().ok()?)))
    {
        if collection == "history-icon"
            && request
                .headers()
                .get("sec-fetch-site")
                .is_some_and(|site| site != "same-origin")
        {
            return response(StatusCode::FORBIDDEN, "text/plain", Vec::new(), "no-store");
        }
        let state = app.state::<AppState>();
        let (address, light) = {
            let store = state.store_guard();
            let address = if collection == "favorite" {
                store
                    .bookmarks
                    .iter()
                    .find(|bookmark| bookmark.id == id)
                    .map(|bookmark| bookmark.url.clone())
            } else {
                store
                    .history
                    .iter()
                    .rev()
                    .find(|entry| entry.id == id)
                    .map(|entry| entry.url.clone())
            }
            .unwrap_or_default();
            (address, store.settings.theme == "light")
        };
        let (media_type, bytes) = site_icons::image(&state.data_dir, &address, light);
        let mut icon = response(StatusCode::OK, media_type, bytes.into_owned(), "no-store");
        icon.headers_mut().insert(
            "Cross-Origin-Resource-Policy",
            "same-origin".parse().expect("valid resource policy"),
        );
        return icon;
    }
    match path {
        "favicon" | "favicon.ico" => asset("image/png", include_bytes!("../../icons/32x32.png")),
        "brand" => asset("image/png", include_bytes!("../../icons/128x128.png")),
        "res/content.css" => asset(
            "text/css; charset=utf-8",
            include_bytes!("../../ui/content.css"),
        ),
        "res/pages.js" => asset(
            "text/javascript; charset=utf-8",
            include_bytes!("../../ui/pages.js"),
        ),
        "res/wallpaper.js" => asset(
            "text/javascript; charset=utf-8",
            include_bytes!("../../ui/wallpaper.js"),
        ),
        "res/aurora-glass.jpg" => asset("image/jpeg", include_bytes!("../../ui/aurora-glass.jpg")),
        "search" => {
            let query = request.uri().query().unwrap_or_default();
            let input = url::form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "q")
                .map(|(_, input)| input)
                .unwrap_or_default();
            let resolved =
                urls::resolve_input(&input, &app.state::<AppState>().store_guard().settings);
            json(StatusCode::OK, serde_json::json!({ "url": resolved }))
        }
        _ => {
            let state = app.state::<AppState>();
            let query = url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
                .into_owned()
                .collect::<Vec<_>>();
            let mut history_page = render::HistoryPage::default();
            let (settings, bookmarks, history) = {
                let store = state.store_guard();
                let history = if path == "history" {
                    const PAGE_SIZE: usize = 250;
                    history_page.query = query
                        .iter()
                        .find(|(key, _)| key == "q")
                        .map(|(_, value)| value.trim().chars().take(200).collect::<String>())
                        .unwrap_or_default();
                    history_page.page = query
                        .iter()
                        .find(|(key, _)| key == "page")
                        .and_then(|(_, value)| value.parse::<usize>().ok())
                        .unwrap_or(0)
                        .min(crate::migration::HISTORY_LIMIT / PAGE_SIZE);
                    history_page.has_history = !store.history.is_empty();
                    let needle = history_page.query.to_lowercase();
                    let mut items: Vec<_> = store
                        .history
                        .iter()
                        .rev()
                        .filter(|entry| {
                            needle.is_empty()
                                || entry.url.to_lowercase().contains(&needle)
                                || entry.title.to_lowercase().contains(&needle)
                        })
                        .skip(history_page.page * PAGE_SIZE)
                        .take(PAGE_SIZE + 1)
                        .cloned()
                        .collect();
                    history_page.more = items.len() > PAGE_SIZE;
                    items.truncate(PAGE_SIZE);
                    items
                } else {
                    Vec::new()
                };
                (
                    store.settings.clone(),
                    if matches!(path, "" | "home") {
                        store.bookmarks.iter().take(12).cloned().collect()
                    } else {
                        Vec::new()
                    },
                    history,
                )
            };
            let (enabled, total) = if path == "about" {
                let plugins = state.plugins_guard();
                (plugins.enabled_count(), plugins.list().len())
            } else {
                (0, 0)
            };
            let data = render::PageData {
                settings,
                bookmarks,
                history,
                history_page,
                token: &state.session_token,
                private: webview
                    .strip_prefix("tab-")
                    .and_then(|id| id.parse().ok())
                    .is_some_and(|id| crate::browser::is_private(app, id)),
                enabled,
                total,
            };
            let (found, html) = render::render(path, &query, data);
            response(
                if found {
                    StatusCode::OK
                } else {
                    StatusCode::NOT_FOUND
                },
                "text/html; charset=utf-8",
                html.into_bytes(),
                "no-store",
            )
        }
    }
}

fn action(app: &AppHandle, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    if request.method() != Method::POST || request.body().len() > 1024 {
        return json(
            StatusCode::METHOD_NOT_ALLOWED,
            serde_json::json!({ "ok": false }),
        );
    }

    #[derive(serde::Deserialize)]
    struct Action {
        token: String,
        action: String,
        id: Option<String>,
    }
    let Ok(action) = serde_json::from_slice::<Action>(request.body()) else {
        return json(StatusCode::BAD_REQUEST, serde_json::json!({ "ok": false }));
    };
    let state = app.state::<AppState>();
    if action.token != state.session_token {
        return json(StatusCode::FORBIDDEN, serde_json::json!({ "ok": false }));
    }
    let mut store = state.store_guard();
    match action.action.as_str() {
        "remove-history" => {
            let Some(id) = action.id.and_then(|raw| raw.parse::<u64>().ok()) else {
                return json(StatusCode::BAD_REQUEST, serde_json::json!({ "ok": false }));
            };
            store.history.retain(|entry| entry.id != id);
        }
        "clear-history" => store.history.clear(),
        _ => return json(StatusCode::BAD_REQUEST, serde_json::json!({ "ok": false })),
    }
    state.save_history(&store);
    json(StatusCode::OK, serde_json::json!({ "ok": true }))
}
