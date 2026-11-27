use std::path::Path;

use url::Url;

use crate::core::model::Settings;

pub const LOCALHOST_HOST: &str = "aurora.localhost";
const CHROME_HOST: &str = "tauri.localhost";

pub fn url_base() -> &'static str {
    if cfg!(windows) {
        "http://aurora.localhost"
    } else {
        "aurora://app"
    }
}

pub fn page(path: &str) -> String {
    format!("{}/{path}", url_base())
}

pub fn page_home() -> String {
    page("home")
}

pub fn internal_path(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    let path = url.path().trim_matches('/');
    let host = url.host_str()?;
    match (url.scheme(), host, url.port()) {
        ("aurora", "app", None) | ("http", LOCALHOST_HOST | "aurora.home", None) => {
            Some(path.to_owned())
        }
        ("aurora", legacy, None) if path.is_empty() => Some(legacy.to_owned()),
        _ => None,
    }
}

pub fn is_internal(raw: &str) -> bool {
    internal_path(raw).is_some()
}

pub fn normalize_maybe(raw: &str) -> Option<String> {
    let url = Url::parse(raw.trim()).ok()?;
    let path = internal_path(url.as_str())?;
    let mut normalized = Url::parse(&page(match path.as_str() {
        "history" | "offline" | "about" | "private" => &path,
        _ => "home",
    }))
    .expect("valid built-in URL");
    if path == "offline" {
        normalized.set_query(url.query());
    }
    Some(normalized.into())
}

pub fn is_chrome(url: &Url) -> bool {
    url.path() == "/strip.html"
        && matches!(
            (url.scheme(), url.host_str(), url.port()),
            ("http", Some(CHROME_HOST), None) | ("tauri", Some("localhost"), None)
        )
}

pub fn is_web(url: &Url) -> bool {
    matches!(url.scheme(), "https" | "http")
        && url.host_str().is_some_and(|host| {
            !matches!(
                host.trim_end_matches('.'),
                CHROME_HOST | "ipc.localhost" | "asset.localhost" | LOCALHOST_HOST | "aurora.home"
            )
        })
}

pub fn can_visit(url: &Url) -> bool {
    is_web(url)
        || url.as_str() == "about:blank"
        || (url.as_str().starts_with(url_base())
            && matches!(
                internal_path(url.as_str()).as_deref(),
                Some("" | "home" | "history" | "offline" | "about" | "private")
            ))
}

pub fn bookmark_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    is_web(&url).then(|| url.into())
}

pub const HTML_EXTENSIONS: &[&str] = &["htm", "html", "shtml", "xht", "xhtml"];

pub fn local_html(url: &Url) -> bool {
    url.scheme() == "file"
        && url.host_str().is_none()
        && url.to_file_path().ok().is_some_and(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    HTML_EXTENSIONS
                        .iter()
                        .any(|allowed| ext.eq_ignore_ascii_case(allowed))
                })
        })
}

fn typed_file(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    local_html(&url).then(|| url.into())
}

pub fn shell_target(raw: &std::ffi::OsStr) -> Option<String> {
    if let Some(url) = raw
        .to_str()
        .and_then(|raw| bookmark_url(raw).or_else(|| typed_file(raw)))
    {
        return Some(url);
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        return None;
    }
    let url = Url::from_file_path(path).ok()?;
    (local_html(&url) && path.is_file()).then(|| url.into())
}

pub fn restore_url(raw: &str) -> String {
    normalize_maybe(raw)
        .or_else(|| bookmark_url(raw))
        .or_else(|| typed_file(raw))
        .unwrap_or_else(page_home)
}

pub fn resolve_input(input: &str, settings: &Settings) -> String {
    let raw = input.trim();
    if raw.is_empty()
        || matches!(
            raw.to_ascii_lowercase().as_str(),
            "aurora" | "aurora.home" | "aurora:home"
        )
    {
        return page_home();
    }
    if let Some(internal) = normalize_maybe(raw) {
        return internal;
    }
    if raw.eq_ignore_ascii_case("about:blank") {
        return "about:blank".into();
    }
    if let Some(url) = bookmark_url(raw) {
        return url;
    }
    if let Some(url) = typed_file(raw) {
        return url;
    }
    if !raw.contains(char::is_whitespace) && !raw.contains("://") {
        if let Ok(mut url) = Url::parse(&format!("https://{raw}")) {
            if url.username().is_empty() && url.password().is_none() && is_web(&url) {
                let local = match url.host() {
                    Some(url::Host::Ipv4(_) | url::Host::Ipv6(_)) => true,
                    Some(url::Host::Domain(host)) => {
                        host == "localhost" || host.ends_with(".localhost")
                    }
                    None => false,
                };
                if local {
                    let _ = url.set_scheme("http");
                    return url.into();
                }
                if url
                    .host_str()
                    .is_some_and(|host| host.trim_end_matches('.').contains('.'))
                {
                    return url.into();
                }
            }
        }
    }
    search_url(raw, settings)
}

pub fn search_url(query: &str, settings: &Settings) -> String {
    let (endpoint, field) = match settings.search_engine.as_str() {
        "yandex" => ("https://yandex.ru/search/", "text"),
        "duckduckgo" => ("https://duckduckgo.com/", "q"),
        "bing" => ("https://www.bing.com/search", "q"),
        _ => ("https://www.google.com/search", "q"),
    };
    let mut url = Url::parse(endpoint).expect("valid search engine URL");
    url.query_pairs_mut().append_pair(field, query);
    url.into()
}

pub fn title_for_url(raw: &str) -> String {
    if let Some(path) = internal_path(raw) {
        return match path.as_str() {
            "" | "home" => "Новая вкладка",
            "history" => "История",
            "private" => "Приватная вкладка",
            "offline" => "Ошибка загрузки",
            "about" => "Об Aurora",
            _ => "Aurora",
        }
        .into();
    }
    Url::parse(raw)
        .ok()
        .and_then(|url| {
            if local_html(&url) {
                return url
                    .to_file_path()
                    .ok()?
                    .file_name()?
                    .to_str()
                    .map(str::to_owned);
            }
            url.host_str()
                .map(|host| host.trim_start_matches("www.").to_owned())
        })
        .unwrap_or_else(|| "Вкладка".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_domains_and_loopback() {
        let s = Settings::default();
        assert_eq!(resolve_input("example.com", &s), "https://example.com/");
        assert_eq!(
            resolve_input("http://example.com", &s),
            "http://example.com/"
        );
        assert_eq!(
            resolve_input("localhost:3000", &s),
            "http://localhost:3000/"
        );
        assert_eq!(resolve_input("[::1]:8080", &s), "http://[::1]:8080/");
        assert_eq!(
            resolve_input("sub.site.io/path", &s),
            "https://sub.site.io/path"
        );
        assert_eq!(
            resolve_input("пример.рф", &s),
            "https://xn--e1afmkfd.xn--p1ai/"
        );
    }

    #[test]
    fn resolves_search() {
        let s = Settings::default();
        assert_eq!(
            resolve_input("котики видео", &s),
            "https://www.google.com/search?q=%D0%BA%D0%BE%D1%82%D0%B8%D0%BA%D0%B8+%D0%B2%D0%B8%D0%B4%D0%B5%D0%BE"
        );
        let y = Settings {
            search_engine: "yandex".into(),
            ..s
        };
        assert_eq!(
            resolve_input("rust", &y),
            "https://yandex.ru/search/?text=rust"
        );
    }

    #[test]
    fn internal_urls_and_reserved_origins() {
        let s = Settings::default();
        assert_eq!(resolve_input("aurora://home", &s), page_home());
        assert_eq!(resolve_input("", &s), page_home());
        assert_eq!(resolve_input("aurora://history", &s), page("history"));
        assert_eq!(normalize_maybe("http://aurora.home/").unwrap(), page_home());
        assert_eq!(
            internal_path("aurora://history/").as_deref(),
            Some("history")
        );
        assert_eq!(internal_path("http://aurora.localhost.evil.test/"), None);
        assert_eq!(bookmark_url("https://tauri.localhost./strip.html"), None);
        assert!(!can_visit(&Url::parse("file:///C:/example.txt").unwrap()));
        assert_eq!(restore_url("javascript:void(0)"), page_home());
    }

    #[test]
    fn titles() {
        assert_eq!(
            title_for_url("https://www.youtube.com/watch?v=x"),
            "youtube.com"
        );
        assert_eq!(title_for_url(&page_home()), "Новая вкладка");
        assert_eq!(title_for_url("aurora://history/"), "История");
    }

    #[test]
    fn local_html_is_separate_from_web_navigation() {
        let path = if cfg!(windows) {
            Path::new(r"C:\Users\Public\Documents\Пример HTML #1.HTML")
        } else {
            Path::new("/tmp/Пример HTML #1.HTML")
        };
        let url = Url::from_file_path(path).unwrap();
        assert!(local_html(&url));
        assert!(!can_visit(&url));
        assert_eq!(
            shell_target(url.as_str().as_ref()).as_deref(),
            Some(url.as_str())
        );
        assert_eq!(title_for_url(url.as_str()), "Пример HTML #1.HTML");
        assert!(!local_html(
            &Url::parse("file://fileserver/share/test.html").unwrap()
        ));
    }
}
