use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::{
    model::{Bookmark, HistoryEntry},
    store::AppState,
    urls,
};

mod bookmarks;
pub use bookmarks::html as html_bookmarks;

#[cfg(windows)]
pub mod passwords;
mod sessions;

pub type Links = Vec<(String, String, Option<crate::core::model::TabGroup>)>;
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
    pub bookmarks: Vec<Bookmark>,
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
    pub bookmark_bar: usize,
    #[serde(skip)]
    pub bookmarks_changed: bool,
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
            bookmarks::has_chromium_files(&path)
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
            downloads: database,
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
    Ok(read_source(&source, fields))
}

fn read_source(source: &Source, fields: Selection) -> BrowserData {
    let mut data = BrowserData {
        keep_history: fields.keep_history,
        ..BrowserData::default()
    };
    if fields.bookmarks {
        let result = match source.engine {
            Engine::Chromium => bookmarks::chromium(&source.path),
            Engine::Firefox => database(source)
                .and_then(|db| bookmarks::firefox(&db))
                .map(|items| (items, false))
                .map_err(|_| ()),
        };
        match result {
            Ok((items, partial)) => {
                data.bookmarks = items;
                if partial {
                    data.notices.push("Часть файлов закладок недоступна. Повторите перенос или выберите экспорт HTML.");
                }
            }
            Err(()) => data
                .notices
                .push("Закладки недоступны в выбранном профиле."),
        }
    }
    if fields.history {
        match history(source) {
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
        match download_history(source) {
            Ok(items) => data.downloads = items,
            Err(()) => data
                .notices
                .push("Журнал загрузок недоступен в выбранном профиле."),
        }
    }
    data
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
    use crate::downloads::Status;
    let db = database(source).map_err(|_| ())?;
    if source.engine == Engine::Firefox {
        let mut query = db.prepare(
            "SELECT CAST(d.content AS TEXT), CAST(m.content AS TEXT) FROM moz_annos d
             JOIN moz_anno_attributes a ON a.id=d.anno_attribute_id AND a.name='downloads/destinationFileURI'
             LEFT JOIN moz_annos m ON m.place_id=d.place_id AND m.anno_attribute_id=
                 (SELECT id FROM moz_anno_attributes WHERE name='downloads/metaData')
             ORDER BY d.lastModified DESC, d.id DESC LIMIT 150"
        ).map_err(|_| ())?;
        let rows = query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|_| ())?;
        return Ok(rows
            .filter_map(Result::ok)
            .filter_map(|(uri, metadata)| {
                let path = url::Url::parse(&uri).ok()?.to_file_path().ok()?;
                let meta = metadata
                    .and_then(|data| serde_json::from_str::<Value>(&data).ok())
                    .unwrap_or_default();
                let status = match meta["state"].as_i64() {
                    Some(1) => Status::Complete,
                    Some(3) => Status::Canceled,
                    _ => Status::Interrupted,
                };
                let total = meta["fileSize"].as_u64();
                imported_download(path, status, total.unwrap_or(0), total)
            })
            .collect());
    }
    let mut query = db.prepare(
        "SELECT target_path, received_bytes, total_bytes, state FROM downloads WHERE state IN (1,2,3) ORDER BY start_time DESC LIMIT 150"
    ).map_err(|_| ())?;
    let rows = query
        .query_map([], |row| {
            let path = PathBuf::from(row.get::<_, String>(0)?);
            let received: i64 = row.get(1)?;
            let total: i64 = row.get(2)?;
            let status = match row.get::<_, i64>(3)? {
                1 => Status::Complete,
                2 => Status::Canceled,
                _ => Status::Interrupted,
            };
            Ok(imported_download(
                path,
                status,
                received.max(0) as u64,
                u64::try_from(total).ok(),
            ))
        })
        .map_err(|_| ())?;
    Ok(rows.filter_map(Result::ok).flatten().collect())
}

fn imported_download(
    path: PathBuf,
    status: crate::downloads::Status,
    received: u64,
    total: Option<u64>,
) -> Option<crate::downloads::Download> {
    if !path.is_absolute() {
        return None;
    }
    Some(crate::downloads::Download {
        id: 0,
        name: path.file_name()?.to_string_lossy().into_owned(),
        path,
        status,
        received,
        total,
        can_resume: false,
        private: false,
    })
}

pub fn apply(state: &AppState, data: BrowserData) -> Summary {
    let mut store = state.store_guard();
    let mut summary = Summary {
        notices: data.notices,
        ..Summary::default()
    };
    (
        summary.bookmarks,
        summary.bookmark_bar,
        summary.bookmarks_changed,
    ) = bookmarks::merge(&mut store, data.bookmarks);
    if summary.bookmarks_changed {
        state.save_bookmarks(&store);
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

    let mut remaining = HashMap::new();
    for tab in store.tabs.tabs.iter().filter(|tab| !tab.private) {
        *remaining
            .entry((
                tab.url.clone(),
                tab.group.as_ref().map(|group| group.id.clone()),
            ))
            .or_insert(0_usize) += 1;
    }
    for (url, title, group) in data.tabs {
        if let Some(url) = urls::bookmark_url(&url) {
            let count = remaining
                .entry((url.clone(), group.as_ref().map(|group| group.id.clone())))
                .or_default();
            if *count == 0 {
                store.add_imported_tab(url, &title, group);
                summary.tabs += 1;
            } else {
                *count -= 1;
            }
        }
    }
    if summary.tabs != 0 {
        state.save_tabs(&store);
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
    use super::*;
    use crate::core::store::now_secs;
    use crate::downloads::Status;
    use rusqlite::params;

    fn test_directory() -> PathBuf {
        let mut id = [0; 8];
        getrandom::fill(&mut id).unwrap();
        let path =
            std::env::temp_dir().join(format!("aurora-import-{:016x}", u64::from_le_bytes(id)));
        fs::create_dir(&path).unwrap();
        path
    }

    const ALL: Selection = Selection {
        bookmarks: true,
        history: true,
        tabs: true,
        downloads: true,
        keep_history: true,
    };

    #[test]
    fn exported_html_decodes_labels_and_rejects_non_web_targets() {
        let links = super::html_bookmarks(r#"<DL><DT><A HREF="https://example.org/">A &amp; B</A><DT><A HREF="javascript:alert(1)">Skip</A></DL>"#).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://example.org/");
        assert_eq!(links[0].title, "A & B");
        assert!(links[0].is_on_bar());
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
        assert_eq!(links[0].folder, ["Work & study", "Rust"]);
        assert!(links[1].folder.is_empty());
    }

    #[test]
    fn exported_html_restores_the_toolbar_separately() {
        let links = super::html_bookmarks(r#"<DL><p>
        <DT><H3 PERSONAL_TOOLBAR_FOLDER="true">Bookmarks bar</H3>
        <DL><p><DT><H3>Work</H3><DL><p><DT><A HREF="https://example.org/work">Work link</A></DL><p></DL><p>
        <DT><A HREF="https://example.org/other">Other</A></DL>"#).unwrap();
        assert!(links[0].is_on_bar());
        assert_eq!(links[0].folder, ["Work"]);
        assert!(!links[1].is_on_bar());
    }

    #[test]
    fn chromium_import_preserves_toolbar_order_and_reopens_without_duplicates() {
        let root = test_directory();
        let source_path = root.join("Default");
        fs::create_dir_all(source_path.join("Sessions")).unwrap();
        fs::write(source_path.join("Bookmarks"), r#"{"roots":{
            "other":{"name":"Other bookmarks","children":[{"name":"Reference","url":"https://example.org/b"}]},
            "bookmark_bar":{"name":"Brave bookmarks","children":[
                {"name":"First","url":"https://example.org/a"},
                {"name":"Research","children":[{"name":"Nested","children":[{"name":"Folder link","url":"https://example.org/b"}]}]},
                {"name":"Third","url":"https://example.org/c"}]}}}"#).unwrap();
        fs::write(source_path.join("AccountBookmarks"), r#"{"roots":{"bookmark_bar":{"children":[{"name":"Account","url":"https://example.org/account"}]}}}"#).unwrap();
        let mut session = b"SNSS\x03\0\0\0".to_vec();
        for tab in [1, 2] {
            sessions::tests::append(
                &mut session,
                6,
                &sessions::tests::navigation(tab, 0, "https://example.net/tab"),
            );
        }
        fs::write(source_path.join("Sessions/Session_100"), session).unwrap();
        let db = Connection::open(source_path.join("History")).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT);
            CREATE TABLE visits (url INTEGER, visit_time INTEGER);
            CREATE TABLE downloads (target_path TEXT, received_bytes INTEGER, total_bytes INTEGER, state INTEGER, start_time INTEGER);
            INSERT INTO urls VALUES (1,'https://example.org/visit','Example visit');").unwrap();
        let visit = (now_secs() as i64 - 45 * 86_400) * 1_000_000 + 11_644_473_600_000_000;
        db.execute("INSERT INTO visits VALUES (1,?1)", [visit])
            .unwrap();
        let download = root.join("Архив.zip");
        db.execute(
            "INSERT INTO downloads VALUES (?1,42,42,1,?2)",
            params![download.to_str().unwrap(), visit],
        )
        .unwrap();
        let mut sources = Vec::new();
        add_sources(&mut sources, "brave", "Brave", &root, Engine::Chromium);
        let source = &sources[0];
        assert!(source.tabs && source.history && source.bookmarks && source.downloads);
        let destination = root.join("Aurora");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("bookmarks.json"), r#"[{"id":3,"url":"https://example.org/b","title":"Reference","folder":["Other bookmarks"]},{"id":4,"url":"https://example.org/c","title":"Third"}]"#).unwrap();
        let state = AppState::init(destination.clone()).unwrap();
        let report = apply(&state, read_source(source, ALL));
        assert_eq!(
            (
                report.bookmarks,
                report.bookmark_bar,
                report.tabs,
                report.history,
                report.downloads
            ),
            (3, 4, 2, 1, 1)
        );
        let repeat = apply(&state, read_source(source, ALL));
        assert_eq!(
            (
                repeat.bookmarks,
                repeat.tabs,
                repeat.history,
                repeat.downloads
            ),
            (0, 0, 0, 0)
        );
        assert!(!repeat.bookmarks_changed);
        assert_eq!(
            state.downloads_guard().persisted()[0].status,
            Status::Complete
        );
        state.flush_saves();
        drop(state);
        drop(db);
        let reopened = AppState::init(destination).unwrap();
        let store = reopened.store_guard();
        let bar: Vec<_> = store
            .bookmarks
            .iter()
            .filter(|item| item.is_on_bar())
            .map(|item| item.title.as_str())
            .collect();
        assert_eq!(bar, ["First", "Folder link", "Third", "Account"]);
        assert_eq!(store.bookmarks[1].folder, ["Research", "Nested"]);
        assert_eq!(store.bookmarks.last().unwrap().on_bar, Some(false));
        assert_eq!(store.bookmarks[2].id, 4);
        assert_eq!(
            store.history[0].ts,
            (visit / 1_000_000 - 11_644_473_600) as u64
        );
        assert_eq!(store.settings.history_retention_days, 0);
        assert_eq!(
            store
                .tabs
                .tabs
                .iter()
                .filter(|tab| tab.url == "https://example.net/tab")
                .count(),
            2
        );
        assert_eq!(reopened.downloads_guard().persisted()[0].path, download);
    }

    #[test]
    fn firefox_import_reads_toolbar_order_session_visits_and_downloads() {
        let root = test_directory();
        let db = Connection::open(root.join("places.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE moz_places(id INTEGER PRIMARY KEY,url TEXT,title TEXT);
            CREATE TABLE moz_bookmarks(id INTEGER PRIMARY KEY,parent INTEGER,guid TEXT,type INTEGER,position INTEGER,title TEXT,fk INTEGER);
            CREATE TABLE moz_historyvisits(place_id INTEGER,visit_date INTEGER);
            CREATE TABLE moz_anno_attributes(id INTEGER PRIMARY KEY,name TEXT);
            CREATE TABLE moz_annos(id INTEGER PRIMARY KEY, place_id INTEGER, anno_attribute_id INTEGER, content TEXT, lastModified INTEGER);
            INSERT INTO moz_places VALUES (1,'https://example.org/first','One'),(2,'https://example.org/second','Two');
            INSERT INTO moz_bookmarks VALUES (1,0,'toolbar_____',2,0,'Localized toolbar',NULL),
                (2,0,'unfiled_____',2,1,'Other',NULL),(3,1,'localFolder_',2,0,'Folder',NULL),
                (4,3,'first_______',1,0,'First',1),(5,1,'second______',1,1,'Second',2),
                (6,2,'reference___',1,0,'Reference',1);
            INSERT INTO moz_anno_attributes VALUES (1,'downloads/destinationFileURI'),(2,'downloads/metaData');").unwrap();
        let current = now_secs();
        db.execute(
            "INSERT INTO moz_historyvisits VALUES (1,?1)",
            [current * 1_000_000],
        )
        .unwrap();
        let path = root.join("Firefox package.zip");
        let file_uri = url::Url::from_file_path(&path).unwrap().to_string();
        db.execute(
            "INSERT INTO moz_annos VALUES (1,1,1,?1,?2),(2,1,2,?3,?2)",
            params![file_uri, current, r#"{"state":1,"fileSize":64}"#],
        )
        .unwrap();
        let mut session = b"mozLz40\0".to_vec();
        session.extend(lz4_flex::compress_prepend_size(br#"{"windows":[
            {"tabs":[{"index":1,"entries":[{"url":"https://example.net/public","title":"Public"},{"url":"https://example.net/forward"}]}]},
            {"isPrivate":true,"tabs":[{"index":1,"entries":[{"url":"https://example.net/private"}]}]}]}"#));
        fs::write(root.join("sessionstore.jsonlz4"), session).unwrap();
        let mut sources = Vec::new();
        add_sources(&mut sources, "firefox", "Firefox", &root, Engine::Firefox);
        let data = read_source(&sources[0], ALL);
        assert!(data.notices.is_empty());
        assert_eq!(
            data.bookmarks
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second", "Reference"]
        );
        assert_eq!(data.bookmarks[0].folder, ["Folder"]);
        assert!(data.bookmarks[0].is_on_bar());
        assert!(!data.bookmarks[2].is_on_bar());
        assert_eq!(
            data.tabs,
            [("https://example.net/public".into(), "Public".into(), None)]
        );
        assert_eq!(data.history[0].ts, current);
        assert_eq!(data.downloads[0].path, path);
        assert_eq!(data.downloads[0].total, Some(64));
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }
}
