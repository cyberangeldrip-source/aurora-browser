use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::{model::HistoryEntry, store::AppState, urls};

#[cfg(windows)]
pub mod passwords;
mod sessions;

pub type Links = Vec<(String, String, Option<crate::core::model::TabGroup>)>;
pub type BookmarkLinks = Vec<(String, String, Vec<String>)>;
pub const HISTORY_LIMIT: usize = 50_000;
const FILE_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Engine {
    Chromium,
    Firefox,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    id: String,
    pub browser: &'static str,
    profile: String,
    bookmarks: bool,
    history: bool,
    tabs: bool,
    downloads: bool,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    engine: Engine,
}

#[derive(Clone, Copy, Deserialize)]
pub struct Selection {
    pub bookmarks: bool,
    pub history: bool,
    pub tabs: bool,
    #[serde(default)]
    pub downloads: bool,
    #[serde(default)]
    pub keep_history: bool,
}

#[derive(Default)]
pub struct BrowserData {
    pub bookmarks: BookmarkLinks,
    pub history: Vec<HistoryEntry>,
    pub tabs: Links,
    pub downloads: Vec<crate::downloads::Download>,
    pub notices: Vec<&'static str>,
    pub stale_tabs: bool,
    keep_history: bool,
}

#[derive(Default, Serialize)]
pub struct Summary {
    pub bookmarks: usize,
    pub history: usize,
    pub tabs: usize,
    pub downloads: usize,
    pub notices: Vec<&'static str>,
}

pub fn sources() -> Vec<Source> {
    let mut sources = Vec::new();
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            for (key, browser, relative) in [
                ("brave", "Brave", "BraveSoftware/Brave-Browser/User Data"),
                ("chrome", "Google Chrome", "Google/Chrome/User Data"),
                ("edge", "Microsoft Edge", "Microsoft/Edge/User Data"),
                ("vivaldi", "Vivaldi", "Vivaldi/User Data"),
                ("yandex", "Яндекс Браузер", "Yandex/YandexBrowser/User Data"),
                ("chromium", "Chromium", "Chromium/User Data"),
            ] {
                add_sources(
                    &mut sources,
                    key,
                    browser,
                    &local.join(relative),
                    Engine::Chromium,
                );
            }
        }
        if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
            for (key, browser, relative, engine) in [
                (
                    "firefox",
                    "Firefox",
                    "Mozilla/Firefox/Profiles",
                    Engine::Firefox,
                ),
                (
                    "opera",
                    "Opera",
                    "Opera Software/Opera Stable",
                    Engine::Chromium,
                ),
                (
                    "opera-gx",
                    "Opera GX",
                    "Opera Software/Opera GX Stable",
                    Engine::Chromium,
                ),
            ] {
                add_sources(&mut sources, key, browser, &appdata.join(relative), engine);
            }
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(base) =
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    {
        for (key, browser, relative, engine) in [
            (
                "brave",
                "Brave",
                "BraveSoftware/Brave-Browser",
                Engine::Chromium,
            ),
            ("chrome", "Google Chrome", "Google/Chrome", Engine::Chromium),
            ("edge", "Microsoft Edge", "Microsoft Edge", Engine::Chromium),
            ("firefox", "Firefox", "Firefox/Profiles", Engine::Firefox),
        ] {
            add_sources(&mut sources, key, browser, &base.join(relative), engine);
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        for (key, browser, relative) in [
            ("brave", "Brave", "BraveSoftware/Brave-Browser"),
            ("chrome", "Google Chrome", "google-chrome"),
            ("chromium", "Chromium", "chromium"),
            ("edge", "Microsoft Edge", "microsoft-edge"),
        ] {
            add_sources(
                &mut sources,
                key,
                browser,
                &base.join(relative),
                Engine::Chromium,
            );
        }
        add_sources(
            &mut sources,
            "firefox",
            "Firefox",
            &home.join(".mozilla/firefox"),
            Engine::Firefox,
        );
    }
    sources
}

fn add_sources(
    found: &mut Vec<Source>,
    key: &str,
    browser: &'static str,
    root: &Path,
    engine: Engine,
) {
    let info = read_limit(&root.join("Local State"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let mut paths = vec![root.to_path_buf()];
    if let Ok(entries) = fs::read_dir(root) {
        paths.extend(
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path()),
        );
    }
    paths.sort_unstable();
    for path in paths {
        let database = path
            .join(if engine == Engine::Firefox {
                "places.sqlite"
            } else {
                "History"
            })
            .is_file();
        let bookmarks = if engine == Engine::Firefox {
            database
        } else {
            path.join("Bookmarks").is_file()
        };
        let tabs = !sessions::files(&path, engine).is_empty();
        if !(database || bookmarks || tabs) {
            continue;
        }
        let folder = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Default");
        let name = info
            .as_ref()
            .and_then(|info| info["profile"]["info_cache"][folder]["name"].as_str())
            .unwrap_or(folder);
        found.push(Source {
            id: format!("{key}:{folder}"),
            profile: name.chars().take(80).collect(),
            browser,
            bookmarks,
            history: database,
            tabs,
            downloads: database && engine == Engine::Chromium,
            path,
            engine,
        });
    }
}

pub fn read_browser(id: &str, fields: Selection) -> Result<BrowserData, String> {
    let source = sources()
        .into_iter()
        .find(|source| source.id == id || (id == "brave" && source.id.starts_with("brave:")))
        .ok_or("Профиль браузера не найден")?;
    let mut data = BrowserData {
        keep_history: fields.keep_history,
        ..BrowserData::default()
    };
    if fields.bookmarks {
        match bookmarks(&source) {
            Ok(items) => data.bookmarks = items,
            Err(()) => data
                .notices
                .push("Закладки недоступны в выбранном профиле."),
        }
    }
    if fields.history {
        match history(&source) {
            Ok(items) => data.history = items,
            Err(()) => data
                .notices
                .push("История недоступна. Закройте исходный браузер и повторите перенос."),
        }
    }
    if fields.tabs {
        match sessions::load(&source.path, source.engine) {
            Ok((items, stale)) => {
                data.tabs = items;
                data.stale_tabs = stale;
                if stale {
                    data.notices.push("Перенесён предыдущий сохранённый сеанс. Закройте исходный браузер для переноса текущего.");
                }
            }
            Err(()) => data
                .notices
                .push("Сеанс вкладок недоступен. Закройте исходный браузер и повторите перенос."),
        }
    }
    if fields.downloads {
        match download_history(&source) {
            Ok(items) => data.downloads = items,
            Err(()) => data
                .notices
                .push("Журнал загрузок недоступен в выбранном профиле."),
        }
    }
    Ok(data)
}

fn read_limit(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(FILE_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Файл слишком большой",
        ));
    }
    Ok(bytes)
}

fn database(source: &Source) -> rusqlite::Result<Connection> {
    let file = if source.engine == Engine::Firefox {
        "places.sqlite"
    } else {
        "History"
    };
    let db = Connection::open_with_flags(source.path.join(file), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(Duration::from_millis(1500))?;
    Ok(db)
}

fn bookmarks(source: &Source) -> Result<BookmarkLinks, ()> {
    if source.engine == Engine::Firefox {
        let db = database(source).map_err(|_| ())?;
        let mut folders_query = db
            .prepare("SELECT id,parent,COALESCE(title,'') FROM moz_bookmarks WHERE type=2")
            .map_err(|_| ())?;
        let folders: std::collections::HashMap<i64, (i64, String)> = folders_query
            .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))
            .map_err(|_| ())?
            .filter_map(Result::ok)
            .collect();
        let mut query = db.prepare("SELECT p.url, COALESCE(b.title,p.title,''), b.parent FROM moz_bookmarks b JOIN moz_places p ON b.fk=p.id WHERE b.type=1 ORDER BY b.position,b.id").map_err(|_| ())?;
        let rows = query
            .query_map([], |row| {
                let mut folder = Vec::new();
                let mut parent: i64 = row.get(2)?;
                for _ in 0..16 {
                    let Some((next, name)) = folders.get(&parent) else {
                        break;
                    };
                    if !name.trim().is_empty() {
                        folder.push(name.chars().take(90).collect());
                    }
                    parent = *next;
                }
                folder.reverse();
                Ok((row.get(0)?, row.get(1)?, folder))
            })
            .map_err(|_| ())?;
        return Ok(rows.filter_map(Result::ok).collect());
    }
    let bytes = read_limit(&source.path.join("Bookmarks")).map_err(|_| ())?;
    let data: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let roots = data["roots"].as_object().ok_or(())?;
    let mut queue: Vec<_> = roots
        .iter()
        .rev()
        .map(|(key, node)| (node, Vec::<String>::new(), key == "bookmark_bar"))
        .collect();
    let mut links = Vec::new();
    while let Some((node, folder, skip_name)) = queue.pop() {
        if let Some(url) = node["url"].as_str() {
            links.push((
                url.to_owned(),
                node["name"].as_str().unwrap_or_default().to_owned(),
                folder.clone(),
            ));
        }
        if let Some(children) = node["children"].as_array() {
            let mut folder = folder;
            if let Some(name) = node["name"]
                .as_str()
                .filter(|name| !skip_name && !name.trim().is_empty())
            {
                if folder.len() < 16 {
                    folder.push(name.chars().take(90).collect());
                }
            }
            queue.extend(
                children
                    .iter()
                    .rev()
                    .map(|child| (child, folder.clone(), false)),
            );
        }
    }
    Ok(links)
}

fn history(source: &Source) -> Result<Vec<HistoryEntry>, ()> {
    let db = database(source).map_err(|_| ())?;
    let sql = match source.engine {
        Engine::Chromium => "SELECT urls.url, COALESCE(urls.title,''), visits.visit_time FROM visits JOIN urls ON visits.url=urls.id ORDER BY visits.visit_time DESC LIMIT ?1",
        Engine::Firefox => "SELECT moz_places.url, COALESCE(moz_places.title,''), moz_historyvisits.visit_date FROM moz_historyvisits JOIN moz_places ON moz_historyvisits.place_id=moz_places.id ORDER BY moz_historyvisits.visit_date DESC LIMIT ?1",
    };
    let mut query = db.prepare(sql).map_err(|_| ())?;
    let rows = query
        .query_map([HISTORY_LIMIT], |row| {
            let time: i64 = row.get(2)?;
            let epoch = if source.engine == Engine::Chromium {
                11_644_473_600_000_000
            } else {
                0
            };
            Ok(HistoryEntry {
                id: 0,
                url: row.get(0)?,
                title: row.get(1)?,
                ts: time.saturating_sub(epoch).max(0) as u64 / 1_000_000,
            })
        })
        .map_err(|_| ())?;
    Ok(rows
        .filter_map(Result::ok)
        .filter(|row| row.ts != 0)
        .collect())
}

fn download_history(source: &Source) -> Result<Vec<crate::downloads::Download>, ()> {
    use crate::downloads::{Download, Status};
    if source.engine != Engine::Chromium {
        return Err(());
    }
    let db = database(source).map_err(|_| ())?;
    let mut query = db.prepare(
        "SELECT target_path, received_bytes, total_bytes, state FROM downloads WHERE state IN (1,2,3) ORDER BY start_time DESC LIMIT 150"
    ).map_err(|_| ())?;
    let rows = query
        .query_map([], |row| {
            let path = PathBuf::from(row.get::<_, String>(0)?);
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let received: i64 = row.get(1)?;
            let total: i64 = row.get(2)?;
            let status = match row.get::<_, i64>(3)? {
                1 => Status::Complete,
                2 => Status::Canceled,
                _ => Status::Interrupted,
            };
            Ok(Download {
                id: 0,
                name,
                path,
                status,
                received: received.max(0) as u64,
                total: u64::try_from(total).ok(),
                can_resume: false,
                private: false,
            })
        })
        .map_err(|_| ())?;
    Ok(rows
        .filter_map(Result::ok)
        .filter(|item| item.path.is_absolute() && !item.name.is_empty())
        .collect())
}

pub fn html_bookmarks(document: &str) -> Result<BookmarkLinks, String> {
    if document.len() as u64 > FILE_LIMIT {
        return Err("Файл закладок слишком большой".into());
    }
    let document = scraper::Html::parse_document(document);
    let selector = scraper::Selector::parse("a[href]").expect("selector is valid");
    let heading = scraper::Selector::parse(":scope > h3").expect("selector is valid");
    let links: BookmarkLinks = document
        .select(&selector)
        .filter_map(|tag| {
            let url = urls::bookmark_url(tag.value().attr("href")?)?;
            let mut folders: Vec<String> = tag
                .ancestors()
                .filter_map(scraper::ElementRef::wrap)
                .filter(|ancestor| ancestor.value().name() == "dt")
                .filter_map(|ancestor| ancestor.select(&heading).next())
                .map(|folder| {
                    folder
                        .text()
                        .collect::<String>()
                        .trim()
                        .chars()
                        .take(90)
                        .collect()
                })
                .filter(|name: &String| !name.is_empty())
                .take(16)
                .collect();
            folders.reverse();
            Some((
                url,
                tag.text().collect::<String>().trim().to_owned(),
                folders,
            ))
        })
        .collect();
    if links.is_empty() {
        return Err("В файле нет поддерживаемых закладок".into());
    }
    Ok(links)
}

pub fn apply(state: &AppState, data: BrowserData) -> Summary {
    let mut store = state.store_guard();
    let mut summary = Summary {
        notices: data.notices,
        ..Summary::default()
    };
    let mut known: HashSet<_> = store
        .bookmarks
        .iter()
        .map(|item| item.url.clone())
        .collect();
    let mut restored_folders = 0;
    for (url, title, folder) in data.bookmarks {
        if let Some(url) = urls::bookmark_url(&url) {
            if known.insert(url.clone()) {
                store.add_bookmark_in(url, &title, folder);
                summary.bookmarks += 1;
            } else if !folder.is_empty() {
                if let Some(bookmark) = store
                    .bookmarks
                    .iter_mut()
                    .find(|item| item.url == url && item.folder.is_empty())
                {
                    bookmark.folder = folder;
                    restored_folders += 1;
                }
            }
        }
    }
    if summary.bookmarks != 0 || restored_folders != 0 {
        state.save_bookmarks(&store);
    }
    if restored_folders != 0 {
        summary
            .notices
            .push("Обновлены папки ранее импортированных закладок.");
    }
    if data.keep_history
        && store.settings.history_retention_days != 0
        && data.history.iter().any(|entry| {
            entry.ts
                < crate::core::store::now_secs()
                    .saturating_sub(u64::from(store.settings.history_retention_days) * 86_400)
        })
    {
        store.settings.history_retention_days = 0;
        state.save_settings(&store);
        summary
            .notices
            .push("Хранение истории переключено на бессрочное, чтобы сохранить старые посещения.");
    }
    summary.history = store.merge_history(data.history);
    if summary.history != 0 {
        state.save_history(&store);
    }

    let mut known: HashSet<_> = store
        .tabs
        .tabs
        .iter()
        .filter(|tab| !tab.private)
        .map(|tab| tab.url.clone())
        .collect();
    let mut updated_groups = false;
    for (url, title, group) in data.tabs {
        if let Some(url) = urls::bookmark_url(&url) {
            if known.insert(url.clone()) {
                store.add_imported_tab(url, &title, group);
                summary.tabs += 1;
            } else if let Some(group) = group {
                if let Some(tab) = store
                    .tabs
                    .tabs
                    .iter_mut()
                    .find(|tab| !tab.private && tab.url == url && tab.group.is_none())
                {
                    tab.group = Some(group);
                    updated_groups = true;
                }
            }
        }
    }
    if summary.tabs != 0 || updated_groups {
        state.save_tabs(&store);
    }
    if updated_groups {
        summary
            .notices
            .push("Восстановлены группы ранее импортированных вкладок.");
    }
    drop(store);
    summary.downloads = state.downloads_guard().merge_imported(data.downloads);
    if summary.downloads != 0 {
        state.save_downloads();
    }
    summary
}

pub fn migrate_brave() -> Result<Summary, String> {
    let mut data = read_browser(
        "brave",
        Selection {
            bookmarks: true,
            history: true,
            tabs: true,
            downloads: true,
            keep_history: true,
        },
    )?;
    if data.stale_tabs {
        data.tabs.clear();
        data.notices
            .retain(|notice| !notice.starts_with("Перенесён предыдущий"));
        data.notices.push("Текущий сеанс Brave занят. Вкладки пропущены, чтобы не смешивать их с предыдущим сеансом.");
    }
    let state =
        AppState::init(profile_path()?).map_err(|_| "Профиль Aurora занят или недоступен")?;
    let summary = apply(&state, data);
    state.flush_saves();
    Ok(summary)
}

pub fn profile_path() -> Result<PathBuf, String> {
    std::env::var_os("AURORA_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            #[cfg(windows)]
            {
                std::env::var_os("APPDATA")
                    .map(|root| PathBuf::from(root).join("dev.aurora.browser"))
            }
            #[cfg(not(windows))]
            {
                None
            }
        })
        .ok_or_else(|| "Каталог профиля Aurora не найден".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn exported_html_decodes_labels_and_rejects_non_web_targets() {
        let links = super::html_bookmarks(r#"<DL><DT><A HREF="https://example.org/">A &amp; B</A><DT><A HREF="javascript:alert(1)">Skip</A></DL>"#).unwrap();
        assert_eq!(
            links,
            [("https://example.org/".into(), "A & B".into(), Vec::new())]
        );
    }

    #[test]
    fn exported_html_restores_nested_folders_and_root_links() {
        let links = super::html_bookmarks(
            r#"<DL><p>
                <DT><H3>Work &amp; study</H3>
                <DL><p>
                    <DT><H3>Rust</H3>
                    <DL><p><DT><A HREF="https://www.rust-lang.org/">Rust</A></DL><p>
                </DL><p>
                <DT><A HREF="https://example.org/">Home</A>
            </DL><p>"#,
        )
        .unwrap();
        assert_eq!(links[0].2, ["Work & study", "Rust"]);
        assert!(links[1].2.is_empty());
    }
}
