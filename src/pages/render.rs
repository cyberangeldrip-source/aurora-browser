use crate::core::{
    model::{Bookmark, HistoryEntry, Settings, APP_VERSION},
    urls,
};

pub struct PageData<'a> {
    pub settings: Settings,
    pub bookmarks: Vec<Bookmark>,
    pub history: Vec<HistoryEntry>,
    pub history_page: HistoryPage,
    pub token: &'a str,
    pub private: bool,
    pub enabled: usize,
    pub total: usize,
}

#[derive(Default)]
pub struct HistoryPage {
    pub query: String,
    pub page: usize,
    pub more: bool,
    pub has_history: bool,
}

const SEARCH_ICON: &str = r#"<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><circle cx="10.5" cy="10.5" r="6"/><path d="m15 15 5 5"/></svg>"#;
const ARROW_ICON: &str = r#"<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h13m-5-5 5 5-5 5"/></svg>"#;

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn shell(title: &str, body: &str, data: &PageData<'_>, page: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="ru" data-theme="{theme}" class="{animations}" style="--accent:{accent}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="referrer" content="no-referrer">
<title>{title} · Aurora</title>
<link rel="icon" href="{base}/favicon">
<link rel="stylesheet" href="{base}/res/content.css">
<link rel="stylesheet" href="{base}/res/tooltip.css">
<script defer src="{base}/res/pages.js"></script>
<script defer src="{base}/res/tooltip.js"></script>
</head>
<body class="{page}" data-token="{token}">
{body}
</body>
</html>"#,
        theme = esc(&data.settings.theme),
        animations = if data.settings.animations {
            ""
        } else {
            "no-motion"
        },
        accent = esc(&data.settings.accent),
        title = esc(title),
        token = if page == "history-page" {
            esc(data.token)
        } else {
            String::new()
        },
        base = urls::url_base(),
    )
}

pub fn render(path: &str, query: &[(String, String)], data: PageData<'_>) -> (bool, String) {
    let (title, class, body) = match path {
        "" | "home" if !data.private => ("Новая вкладка", "home-page", home(&data)),
        "" | "home" | "private" => ("Приватная вкладка", "home-page", private_home(&data)),
        "history" => ("История", "history-page", history(&data)),
        "offline" => ("Ошибка загрузки", "system-page", offline(query)),
        "about" => ("Об Aurora", "system-page", about(&data)),
        _ => (
            "Страница не найдена",
            "system-page",
            "<main class='info'><h1>Страница не найдена</h1></main>".to_owned(),
        ),
    };
    (
        matches!(
            path,
            "" | "home" | "private" | "history" | "offline" | "about"
        ),
        shell(title, &body, &data, class),
    )
}

fn home(data: &PageData<'_>) -> String {
    let cards: String = data.bookmarks.iter().filter_map(|bookmark| {
        let url = urls::bookmark_url(&bookmark.url)?;
        Some(format!(
            r#"<a class="shortcut" href="{href}" title="{label}"><span class="site-mark"><img src="{base}/res/favorite/{id}" alt="" width="26" height="26" decoding="async"></span><span>{label}</span></a>"#,
            href = esc(&url),
            label = esc(&bookmark.title),
            base = urls::url_base(),
            id = bookmark.id,
        ))
    }).collect();

    format!(
        r#"{wallpaper}<main class="home">
  <div class="hero">
    <div class="hero-lockup"><img class="brand-mark" src="{base}/brand" width="78" height="78" alt=""><h1>Aurora</h1></div>
    {search}
  </div>
  <section class="favorites" aria-label="Избранное">
    <div class="section-head"><h2>Избранное</h2><a href="{base}/history">История {ARROW_ICON}</a></div>
    <div class="shortcuts">{cards}{empty}</div>
  </section>
</main>"#,
        base = urls::url_base(),
        wallpaper = format_args!(
            r#"<canvas id="live-wallpaper" aria-hidden="true" data-enabled="{}" hidden></canvas><script defer src="{}/res/wallpaper.js"></script>"#,
            data.settings.animated_wallpaper && data.settings.animations,
            urls::url_base()
        ),
        search = search_form(data),
        empty = if cards.is_empty() {
            "<p class='muted'>Добавьте сайт в закладки кнопкой со звездой у адресной строки.</p>"
        } else {
            ""
        }
    )
}

fn search_form(data: &PageData<'_>) -> String {
    let engine = match data.settings.search_engine.as_str() {
        "yandex" => "Яндекс",
        "bing" => "Bing",
        "duckduckgo" => "DuckDuckGo",
        _ => "Google",
    };
    format!(
        r#"<form id="start-search" class="search-form" action="{base}/search" method="get" role="search">
      {SEARCH_ICON}
      <input type="text" id="query" name="q" aria-label="Поиск или адрес сайта" placeholder="Поиск в {engine} или адрес" autocomplete="off" spellcheck="false" required>
      <button type="submit" aria-label="Перейти">{ARROW_ICON}</button>
    </form>
    <p id="search-error" class="search-error" role="status" hidden></p>"#,
        base = urls::url_base(),
    )
}

fn private_home(data: &PageData<'_>) -> String {
    let details = if data.private {
        "Aurora не добавляет посещения этой вкладки в историю и не открывает её в следующем сеансе. Скачанные файлы остаются на диске."
    } else {
        "Нажмите Ctrl+Shift+N или выберите «Приватная вкладка» в меню Aurora."
    };
    let search = if data.private {
        search_form(data)
    } else {
        String::new()
    };
    format!(
        r#"<main class="home private-home"><div class="hero">
<h1>Приватная вкладка</h1>
<p class="muted">{details}</p>{search}
</div></main>"#
    )
}

fn history(data: &PageData<'_>) -> String {
    let rows: String = data.history.iter().map(|entry| format!(
        r#"<article class="history-row" data-time="{time}">
           <span class="history-icon"><img src="{base}/res/history-icon/{id}" alt="" width="21" height="21" loading="lazy" decoding="async"></span>
           <a href="{url}" class="row-link"><strong>{title}</strong><small>{url}</small></a>
           <time></time>
           <button data-remove="{id}" class="delete-button" aria-label="Удалить из истории" title="Удалить из истории">×</button>
         </article>"#,
        url = esc(&entry.url),
        title = esc(&entry.title),
        base = urls::url_base(),
        time = entry.ts,
        id = entry.id,
    )).collect();
    let has_entries = !rows.is_empty();
    let page = &data.history_page;
    let page_link = |number: usize, label: &str| {
        let args = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("q", &page.query)
            .append_pair("page", &number.to_string())
            .finish();
        format!(
            r#"<a class="secondary" href="{}/history?{}">{label}</a>"#,
            urls::url_base(),
            esc(&args)
        )
    };
    let previous = page
        .page
        .checked_sub(1)
        .map(|number| page_link(number, "Назад"))
        .unwrap_or_default();
    let next = if page.more {
        page_link(page.page + 1, "Дальше")
    } else {
        String::new()
    };
    format!(
        r#"<main class="library">
  <a class="back-link" href="{base}/home">{ARROW_ICON} Новая вкладка</a>
  <div class="history-head">
    <h1>История</h1>
    <button type="button" class="secondary" id="clear-history" {disabled}>Очистить историю</button>
  </div>
  <form class="history-filter" action="{base}/history" method="get">{SEARCH_ICON}<input type="search" name="q" maxlength="200" id="history-query" placeholder="Поиск по истории" aria-label="Поиск по истории" value="{query}">
    <button class="secondary" type="submit">Найти</button>
  </form>
  <p id="history-message" role="status" class="muted" {hide_message}>Здесь появятся посещённые сайты.</p>
  <div id="history-list">{rows}</div>
  <nav class="history-pagination" id="history-pagination" aria-label="Страницы истории">{previous}{next}</nav>
  <dialog id="clear-dialog">
    <form method="dialog">
      <h2>Очистить историю?</h2>
      <p>Из списка будут удалены все посещения.</p>
      <div><button class="secondary" value="cancel" autofocus>Отмена</button><button class="secondary danger" value="clear">Очистить</button></div>
    </form>
  </dialog>
</main>"#,
        base = urls::url_base(),
        disabled = if page.has_history { "" } else { "disabled" },
        hide_message = if has_entries { "hidden" } else { "" },
        query = esc(&page.query),
    )
}

fn offline(query: &[(String, String)]) -> String {
    let retry = query
        .iter()
        .find(|(key, _)| key == "u")
        .and_then(|(_, value)| urls::bookmark_url(value))
        .map(|url| format!("<a class='secondary' href=\"{}\">Повторить</a>", esc(&url)))
        .unwrap_or_default();
    format!(
        r#"<main class="info"><h1>Не удалось открыть страницу</h1>
<p>Проверьте соединение и попробуйте ещё раз.</p>
<div>{retry} <a class="secondary" href="{}">Новая вкладка</a></div></main>"#,
        urls::page_home()
    )
}

fn about(data: &PageData<'_>) -> String {
    let engine = if cfg!(windows) {
        "WebView2 (Chromium)"
    } else if cfg!(target_os = "macos") {
        "WKWebView (WebKit)"
    } else {
        "WebKitGTK"
    };
    format!(
        r#"<main class="info">
<img class="brand-mark" src="{}/brand" width="54" height="54" alt="">
<h1>Aurora {APP_VERSION}</h1>
<dl><dt>Движок</dt><dd>{engine}</dd><dt>Приложение</dt><dd>Rust и Tauri</dd><dt>Плагины</dt><dd>{} из {}</dd></dl>
<a class="secondary" href="{}">Новая вкладка</a>
</main>"#,
        urls::url_base(),
        data.enabled,
        data.total,
        urls::page_home()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_renders_safe_bookmark_links() {
        let data = PageData {
            settings: Settings::default(),
            bookmarks: vec![
                Bookmark {
                    id: 1,
                    title: "<GitHub>".into(),
                    url: "https://github.com/".into(),
                    folder: Vec::new(),
                },
                Bookmark {
                    id: 2,
                    title: "Unknown".into(),
                    url: "javascript:void(0)".into(),
                    folder: Vec::new(),
                },
            ],
            history: Vec::new(),
            history_page: HistoryPage::default(),
            token: "preview",
            private: false,
            enabled: 0,
            total: 3,
        };
        let (_, html) = render("home", &[], data);
        assert!(html.contains("/res/favorite/1"));
        assert!(html.contains("&lt;GitHub&gt;"));
        assert!(html.contains("https://github.com/"));
        assert!(!html.contains("javascript:"));
    }
}
