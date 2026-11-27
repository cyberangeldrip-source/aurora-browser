use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::{
    activation, browser, commands,
    core::store::{write_file, AppState},
    layout,
};

const MARKER: &str = "welcome-state";
const PENDING: &[u8] = b"pending";
const COMPLETE: &[u8] = b"complete";

pub struct Onboarding(AtomicBool);

impl Onboarding {
    pub fn load(dir: &Path) -> io::Result<Self> {
        let marker = dir.join(MARKER);
        let pending = match std::fs::read(&marker) {
            Ok(value) => value == PENDING,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let existing = [
                    "settings.json",
                    "tabs.json",
                    "bookmarks.json",
                    "downloads.json",
                    "history.sqlite3",
                    "history.json",
                    "WebView",
                    "Chrome",
                ]
                .iter()
                .any(|name| dir.join(name).exists());
                if !existing {
                    write_file(&marker, PENDING)?;
                }
                !existing
            }
            Err(error) => return Err(error),
        };
        Ok(Self(AtomicBool::new(pending)))
    }

    pub fn pending(&self) -> bool {
        self.0.load(Relaxed)
    }

    fn finish(&self, dir: &Path) -> io::Result<()> {
        write_file(&dir.join(MARKER), COMPLETE)?;
        self.0.store(false, Relaxed);
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct Preferences {
    theme: String,
    search_engine: String,
    show_bookmark_bar: bool,
    animated_wallpaper: bool,
}

#[tauri::command]
pub async fn finish_onboarding(app: AppHandle, preferences: Preferences) -> Result<(), String> {
    if !app.state::<AppState>().onboarding.pending() {
        return Ok(());
    }
    browser::on_main(app.clone(), move |app| {
        let state = app.state::<AppState>();
        let mut settings = state.store_guard().settings.clone();
        settings.theme = preferences.theme;
        settings.search_engine = preferences.search_engine;
        settings.show_bookmark_bar = preferences.show_bookmark_bar;
        settings.animated_wallpaper = preferences.animated_wallpaper;
        commands::save_settings(app.clone(), settings);
        Ok(())
    })
    .await?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<AppState>();
        state.flush_saves();
        state.onboarding.finish(&state.data_dir)
    })
    .await
    .map_err(|_| "Не удалось завершить настройку")?
    .map_err(|error| format!("Не удалось сохранить первый запуск. {error}"))?;

    browser::on_main(app, |app| {
        let id = app.state::<AppState>().store_guard().tabs.active_id;
        if let Err(error) = browser::activate(app, id, false) {
            let _ = layout::relayout(app);
            browser::emit(app, "au://notice", error);
        }
        activation::drain(app);
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn fresh_setup_resumes_and_existing_profiles_bypass_it() {
        let mut id = [0; 8];
        getrandom::fill(&mut id).unwrap();
        let root =
            std::env::temp_dir().join(format!("aurora-welcome-{:016x}", u64::from_le_bytes(id)));
        let fresh = root.join("fresh");
        let existing = root.join("existing");
        fs::create_dir_all(&fresh).unwrap();
        fs::create_dir_all(&existing).unwrap();

        let setup = Onboarding::load(&fresh).unwrap();
        assert!(setup.pending());
        fs::write(fresh.join("settings.json"), b"{}").unwrap();
        assert!(Onboarding::load(&fresh).unwrap().pending());
        setup.finish(&fresh).unwrap();
        assert!(!setup.pending());
        assert!(!Onboarding::load(&fresh).unwrap().pending());

        fs::write(existing.join("tabs.json"), b"{}").unwrap();
        assert!(!Onboarding::load(&existing).unwrap().pending());
        assert!(!existing.join(MARKER).exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
