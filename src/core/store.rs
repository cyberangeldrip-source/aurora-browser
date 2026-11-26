use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::core::history_db;
use crate::core::model::{Bookmark, HistoryEntry, Settings, Tab, TabsState};
use crate::core::plugins::PluginManager;
use crate::core::urls;
use crate::downloads::{Download, Downloads};

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

enum SaveJob {
    Settings(Settings),
    Tabs(TabsState),
    History(Vec<HistoryEntry>),
    Visit(HistoryEntry, Option<(u64, u64)>),
    Bookmarks(Vec<Bookmark>),
    Downloads(Vec<Download>),
    Flush(Sender<()>),
}

pub fn write_file(path: &Path, data: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or(io::ErrorKind::InvalidInput)?
        .to_string_lossy();
    let temp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    {
        let mut file = File::create(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    fs::rename(temp, path)
}

fn write_json(dir: &Path, name: &str, value: impl serde::Serialize) {
    let result = serde_json::to_vec(&value)
        .map_err(io::Error::other)
        .and_then(|bytes| write_file(&dir.join(name), &bytes));
    if let Err(error) = result {
        eprintln!("Aurora: {name}: {error}");
    }
}

fn saver_loop(dir: PathBuf, rx: Receiver<SaveJob>, mut db: rusqlite::Connection) {
    while let Ok(first) = rx.recv() {
        let mut settings = None;
        let mut tabs = None;
        let mut history = Vec::new();
        let mut bookmarks = None;
        let mut downloads = None;
        let mut flushes = Vec::new();
        for job in std::iter::once(first).chain(rx.try_iter()) {
            match job {
                SaveJob::Settings(value) => settings = Some(value),
                SaveJob::Tabs(value) => tabs = Some(value),
                job @ (SaveJob::History(_) | SaveJob::Visit(_, _)) => history.push(job),
                SaveJob::Bookmarks(value) => bookmarks = Some(value),
                SaveJob::Downloads(value) => downloads = Some(value),
                SaveJob::Flush(reply) => flushes.push(reply),
            }
        }
        if let Some(value) = settings {
            write_json(&dir, "settings.json", value);
        }
        if let Some(value) = tabs {
            write_json(&dir, "tabs.json", value);
        }
        if !history.is_empty() {
            let saved = (|| -> rusqlite::Result<()> {
                let transaction = db.transaction()?;
                for job in &history {
                    match job {
                        SaveJob::History(entries) => {
                            transaction.execute("DELETE FROM visits", [])?;
                            for entry in entries {
                                history_db::write_visit(&transaction, entry)?;
                            }
                        }
                        SaveJob::Visit(entry, oldest) => {
                            history_db::write_visit(&transaction, entry)?;
                            if let Some((ts, id)) = oldest {
                                transaction.execute(
                                    "DELETE FROM visits WHERE (ts,id) < (?1,?2)",
                                    rusqlite::params![ts, id],
                                )?;
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                transaction.commit()
            })();
            if let Err(error) = saved {
                eprintln!("Aurora: history: {error}");
            }
        }
        if let Some(value) = bookmarks {
            write_json(&dir, "bookmarks.json", value);
        }
        if let Some(value) = downloads {
            write_json(&dir, "downloads.json", value);
        }
        for reply in flushes {
            let _ = reply.send(());
        }
    }
}

fn default_bookmarks() -> Vec<Bookmark> {
    [
        ("https://github.com/", "GitHub"),
        ("https://www.youtube.com/", "YouTube"),
        ("https://ru.wikipedia.org/", "Википедия"),
        ("https://habr.com/", "Хабр"),
        ("https://vk.com/", "VK"),
        ("https://web.telegram.org/", "Telegram"),
        ("https://www.rust-lang.org/", "Rust"),
        ("https://music.yandex.ru/", "Яндекс Музыка"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (url, title))| Bookmark {
        id: index as u64 + 1,
        url: url.into(),
        title: title.into(),
        folder: Vec::new(),
    })
    .collect()
}

pub struct Store {
    pub settings: Settings,
    pub tabs: TabsState,
    pub history: Vec<HistoryEntry>,
    pub bookmarks: Vec<Bookmark>,
    recently_closed: Vec<Tab>,
    next_tab_id: u32,
    next_history_id: u64,
    next_bookmark_id: u64,
    last_history_prune_day: u64,
}

fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

impl Store {
    fn default_tabs(homepage: &str) -> TabsState {
        TabsState {
            tabs: vec![Tab::new(1, homepage.to_owned())],
            active_id: 1,
        }
    }

    fn load(dir: &Path, mut history: Vec<HistoryEntry>) -> Self {
        let mut settings: Settings = load_json(&dir.join("settings.json")).unwrap_or_default();
        settings.sanitize();

        let mut tabs: TabsState = if settings.restore_session {
            load_json(&dir.join("tabs.json")).unwrap_or_default()
        } else {
            TabsState::default()
        };
        let mut seen = std::collections::HashSet::new();
        tabs.tabs
            .retain(|tab| tab.id != 0 && tab.id != u32::MAX && seen.insert(tab.id));
        for tab in &mut tabs.tabs {
            tab.label = format!("tab-{}", tab.id);
            let restored = urls::restore_url(&tab.url);
            if urls::is_internal(&restored) || tab.title.trim().is_empty() {
                tab.title = urls::title_for_url(&restored);
            }
            tab.url = restored;
        }
        if tabs.tabs.is_empty() {
            tabs = Self::default_tabs(&settings.homepage);
        }
        if tabs.tabs.iter().all(|tab| tab.id != tabs.active_id) {
            tabs.active_id = tabs.tabs[0].id;
        }

        history
            .retain(|entry| entry.id < i64::MAX as u64 && urls::bookmark_url(&entry.url).is_some());
        if history.len() > crate::migration::HISTORY_LIMIT {
            history.drain(..history.len() - crate::migration::HISTORY_LIMIT);
        }

        let bookmarks_file = dir.join("bookmarks.json");
        let bookmarks = match load_json::<Vec<Bookmark>>(&bookmarks_file) {
            Some(bookmarks) => bookmarks,
            None if !bookmarks_file.exists() => default_bookmarks(),
            None => Vec::new(),
        };

        let next_tab_id = tabs.tabs.iter().map(|tab| tab.id).max().unwrap_or(0) + 1;
        let next_history_id = history.iter().map(|entry| entry.id).max().unwrap_or(0) + 1;
        let mut store = Self {
            settings,
            tabs,
            history,
            bookmarks: Vec::new(),
            recently_closed: Vec::new(),
            next_tab_id,
            next_history_id,
            next_bookmark_id: 1,
            last_history_prune_day: 0,
        };
        store.replace_bookmarks(bookmarks);
        store.prune_history();
        store
    }

    pub fn active(&self) -> Option<&Tab> {
        self.tabs
            .tabs
            .iter()
            .find(|tab| tab.id == self.tabs.active_id)
    }

    pub fn tab_mut(&mut self, id: u32) -> Option<&mut Tab> {
        self.tabs.tabs.iter_mut().find(|tab| tab.id == id)
    }

    pub fn prune_history(&mut self) {
        let now = now_secs();
        self.last_history_prune_day = now / 86_400;
        let days = self.settings.history_retention_days;
        if days != 0 {
            let cutoff = now.saturating_sub(u64::from(days) * 86_400);
            self.history.retain(|entry| entry.ts >= cutoff);
        }
    }

    pub fn push_history(&mut self, url: &str, title: &str) -> bool {
        if !self.settings.record_history || urls::bookmark_url(url).is_none() {
            return false;
        }
        let now = now_secs();
        if let Some(last) = self.history.last_mut() {
            if last.url == url && now.saturating_sub(last.ts) < 5 {
                if last.title == title {
                    return false;
                }
                last.title = title.to_owned();
                return true;
            }
        }
        if now / 86_400 != self.last_history_prune_day {
            self.prune_history();
        }
        self.history.push(HistoryEntry {
            id: self.next_history_id,
            url: url.into(),
            title: title.into(),
            ts: now,
        });
        self.next_history_id = self.next_history_id.wrapping_add(1).max(1);
        if self.history.len() > crate::migration::HISTORY_LIMIT {
            self.history.remove(0);
        }
        true
    }

    pub fn merge_history(&mut self, entries: Vec<HistoryEntry>) -> usize {
        use std::collections::HashSet;
        if entries.is_empty() {
            return 0;
        }
        let days = self.settings.history_retention_days;
        let cutoff = if days == 0 {
            0
        } else {
            now_secs().saturating_sub(u64::from(days) * 86_400)
        };
        let first = self.next_history_id;
        let mut seen: HashSet<_> = self
            .history
            .iter()
            .map(|entry| (entry.url.clone(), entry.ts))
            .collect();
        for mut entry in entries {
            let Some(url) = urls::bookmark_url(&entry.url) else {
                continue;
            };
            if entry.ts < cutoff || !seen.insert((url.clone(), entry.ts)) {
                continue;
            }
            entry.url = url;
            if entry.title.trim().is_empty() {
                entry.title = urls::title_for_url(&entry.url);
            }
            entry.id = self.next_history_id;
            self.next_history_id = self.next_history_id.wrapping_add(1).max(1);
            self.history.push(entry);
        }
        self.history
            .sort_unstable_by_key(|entry| (entry.ts, entry.id));
        self.history.drain(
            ..self
                .history
                .len()
                .saturating_sub(crate::migration::HISTORY_LIMIT),
        );
        self.history
            .iter()
            .filter(|entry| entry.id >= first)
            .count()
    }

    pub fn add_imported_tab(
        &mut self,
        url: String,
        title: &str,
        group: Option<crate::core::model::TabGroup>,
    ) {
        let mut tab = self.allocate_tab(url);
        if !title.trim().is_empty() {
            tab.title = title.trim().to_owned();
        }
        tab.group = group;
        self.tabs.tabs.push(tab);
    }

    pub fn replace_bookmarks(&mut self, mut bookmarks: Vec<Bookmark>) {
        let mut urls = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        bookmarks.retain_mut(|bookmark| {
            let Some(url) = urls::bookmark_url(&bookmark.url) else {
                return false;
            };
            bookmark.url = url;
            bookmark.id != u64::MAX && urls.insert(bookmark.url.clone())
        });
        let mut next_id = bookmarks
            .iter()
            .map(|bookmark| bookmark.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        for bookmark in &mut bookmarks {
            if bookmark.id == 0 || !ids.insert(bookmark.id) {
                while ids.contains(&next_id) || next_id == u64::MAX {
                    next_id = next_id.wrapping_add(1).max(1);
                }
                bookmark.id = next_id;
                ids.insert(next_id);
                next_id = next_id.wrapping_add(1).max(1);
            }
        }
        self.next_bookmark_id = next_id;
        self.bookmarks = bookmarks;
    }

    pub fn add_bookmark(&mut self, url: String, title: &str) {
        self.add_bookmark_in(url, title, Vec::new());
    }

    pub fn add_bookmark_in(&mut self, url: String, title: &str, folder: Vec<String>) {
        while self.next_bookmark_id == u64::MAX
            || self
                .bookmarks
                .iter()
                .any(|bookmark| bookmark.id == self.next_bookmark_id)
        {
            self.next_bookmark_id = self.next_bookmark_id.wrapping_add(1).max(1);
        }
        let title = if title.trim().is_empty() {
            urls::title_for_url(&url)
        } else {
            title.trim().to_owned()
        };
        self.bookmarks.push(Bookmark {
            id: self.next_bookmark_id,
            url,
            title,
            folder,
        });
        self.next_bookmark_id = self.next_bookmark_id.wrapping_add(1).max(1);
    }

    pub fn remove_bookmark(&mut self, id: u64) -> bool {
        let before = self.bookmarks.len();
        self.bookmarks.retain(|bookmark| bookmark.id != id);
        self.bookmarks.len() != before
    }

    pub fn allocate_tab(&mut self, url: String) -> Tab {
        while self.tabs.tabs.iter().any(|tab| tab.id == self.next_tab_id) {
            self.next_tab_id = self.next_tab_id.wrapping_add(1).max(1);
        }
        let tab = Tab::new(self.next_tab_id, url);
        self.next_tab_id = self.next_tab_id.wrapping_add(1).max(1);
        tab
    }

    pub fn remove_tab(&mut self, id: u32) -> Option<Tab> {
        let index = self.tabs.tabs.iter().position(|tab| tab.id == id)?;
        let tab = self.tabs.tabs.remove(index);
        if self.tabs.active_id == id {
            self.tabs.active_id = self
                .tabs
                .tabs
                .get(index)
                .or_else(|| self.tabs.tabs.last())
                .map_or(0, |next| next.id);
        }
        Some(tab)
    }

    pub fn remember_closed(&mut self, tab: Tab) {
        if tab.private {
            return;
        }
        self.recently_closed.push(tab);
        if self.recently_closed.len() > 25 {
            self.recently_closed.remove(0);
        }
    }

    pub fn move_tab(&mut self, id: u32, before: Option<u32>) -> bool {
        let tabs = &mut self.tabs.tabs;
        let Some(from) = tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        let spot = match before {
            Some(id) => match tabs.iter().position(|tab| tab.id == id) {
                Some(spot) => spot,
                None => return false,
            },
            None => tabs.len(),
        };
        let to = spot - usize::from(from < spot);
        if from == to {
            return false;
        }
        let moved = tabs.remove(from);
        tabs.insert(to, moved);
        true
    }

    pub fn last_closed(&self) -> Option<&Tab> {
        self.recently_closed.last()
    }

    pub fn forget_last_closed(&mut self) {
        self.recently_closed.pop();
    }
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub store: Mutex<Store>,
    pub plugins: Mutex<PluginManager>,
    downloads: Mutex<Downloads>,
    pub session_token: String,
    pub ui_overlay: AtomicU32,
    pub media_fullscreen: AtomicU32,
    pub pre_media_fullscreen: AtomicBool,
    saver: Sender<SaveJob>,
    _profile_lock: File,
}

impl AppState {
    pub fn init(data_dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&data_dir)?;
        let profile_lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(data_dir.join("profile.lock"))?;
        profile_lock.try_lock().map_err(io::Error::other)?;
        let legacy = data_dir.join("history.json");
        let mut imported_legacy = false;
        let db = history_db::open(&data_dir, || {
            let entries = load_json::<Vec<HistoryEntry>>(&legacy);
            imported_legacy = entries.is_some();
            let mut entries = entries.unwrap_or_default();
            entries.retain(|entry| entry.id < i64::MAX as u64 && entry.ts <= i64::MAX as u64);
            entries
        })
        .map_err(io::Error::other)?;
        let store = Store::load(&data_dir, history_db::read(&db).map_err(io::Error::other)?);
        if imported_legacy {
            if let Err(error) = fs::remove_file(&legacy) {
                eprintln!("Aurora: не удалось удалить прежний файл истории: {error}");
            }
        }
        let plugins = PluginManager::init(&data_dir);
        let downloads = Downloads::load(&data_dir);
        let mut nonce = [0u8; 32];
        getrandom::fill(&mut nonce).map_err(|error| io::Error::other(error.to_string()))?;
        let session_token = nonce.iter().map(|byte| format!("{byte:02x}")).collect();

        let (saver, receiver) = channel();
        let dir = data_dir.clone();
        std::thread::Builder::new()
            .name("aurora-saver".into())
            .spawn(move || saver_loop(dir, receiver, db))?;

        Ok(Self {
            data_dir,
            store: Mutex::new(store),
            plugins: Mutex::new(plugins),
            downloads: Mutex::new(downloads),
            session_token,
            ui_overlay: AtomicU32::new(0),
            media_fullscreen: AtomicU32::new(0),
            pre_media_fullscreen: AtomicBool::new(false),
            saver,
            _profile_lock: profile_lock,
        })
    }

    pub fn store_guard(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub fn plugins_guard(&self) -> MutexGuard<'_, PluginManager> {
        self.plugins
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub fn save_tabs(&self, store: &Store) {
        let mut session = TabsState::default();
        if store.settings.restore_session {
            session.tabs = store
                .tabs
                .tabs
                .iter()
                .filter(|tab| !tab.private)
                .cloned()
                .collect();
            session.active_id = if session
                .tabs
                .iter()
                .any(|tab| tab.id == store.tabs.active_id)
            {
                store.tabs.active_id
            } else {
                session.tabs.last().map_or(0, |tab| tab.id)
            };
        }
        let _ = self.saver.send(SaveJob::Tabs(session));
    }

    pub fn save_settings(&self, store: &Store) {
        let _ = self.saver.send(SaveJob::Settings(store.settings.clone()));
    }

    pub fn save_history(&self, store: &Store) {
        let _ = self.saver.send(SaveJob::History(store.history.clone()));
    }

    pub fn save_visit(&self, entry: &HistoryEntry, oldest: Option<&HistoryEntry>) {
        let _ = self.saver.send(SaveJob::Visit(
            entry.clone(),
            oldest.map(|entry| (entry.ts, entry.id)),
        ));
    }

    pub fn save_bookmarks(&self, store: &Store) {
        let _ = self.saver.send(SaveJob::Bookmarks(store.bookmarks.clone()));
    }

    pub fn downloads_guard(&self) -> MutexGuard<'_, Downloads> {
        self.downloads
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub fn save_downloads(&self) {
        let _ = self
            .saver
            .send(SaveJob::Downloads(self.downloads_guard().persisted()));
    }

    pub fn flush_saves(&self) {
        let (reply, done) = channel();
        if self.saver.send(SaveJob::Flush(reply)).is_ok() {
            let _ = done.recv_timeout(Duration::from_secs(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_large_history_and_updates_one_visit() {
        let dir = std::env::temp_dir().join(format!("aurora-history-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let total = 50_000u64;
        let now = now_secs();
        let entries: Vec<_> = (1..=total)
            .map(|id| HistoryEntry {
                id,
                title: format!("Visit {id}"),
                url: format!("https://example.org/{id}"),
                ts: now - total + id,
            })
            .collect();
        write_json(&dir, "history.json", &entries);
        let state = AppState::init(dir.clone()).unwrap();
        assert!(!dir.join("history.json").exists());
        {
            let mut store = state.store_guard();
            assert_eq!(store.history.len(), total as usize);
            assert!(store.push_history("https://example.org/latest", "Latest"));
            state.save_visit(store.history.last().unwrap(), store.history.first());
        }
        state.flush_saves();
        let db = rusqlite::Connection::open(dir.join("history.sqlite3")).unwrap();
        let entries = history_db::read(&db).unwrap();
        assert_eq!(entries.len(), total as usize);
        assert_eq!(entries.first().unwrap().id, 2);
        assert_eq!(entries.last().unwrap().url, "https://example.org/latest");
        drop(db);
        drop(state);
    }

    #[test]
    fn moving_tabs_preserves_the_active_tab_and_rejects_stale_targets() {
        let mut store = Store::load(
            &std::env::temp_dir().join("aurora-reorder-test-empty-profile"),
            Vec::new(),
        );
        let first = store.tabs.tabs[0].id;
        let second = store.allocate_tab("https://example.com/".into());
        let third = store.allocate_tab("https://example.org/".into());
        let (second_id, third_id) = (second.id, third.id);
        store.tabs.tabs.extend([second, third]);
        store.tabs.active_id = second_id;
        assert!(store.move_tab(third_id, Some(first)));
        assert!(store.move_tab(first, None));
        assert!(!store.move_tab(second_id, Some(u32::MAX)));
        assert_eq!(store.tabs.active_id, second_id);
        let order: Vec<_> = store.tabs.tabs.iter().map(|tab| tab.id).collect();
        assert_eq!(order, [third_id, second_id, first]);
    }
}
