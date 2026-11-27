use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Manager};

use crate::{browser, core::urls};

const MAX_REQUEST: usize = 128 * 1024;
const REQUEST_TTL: Duration = Duration::from_secs(120);

pub struct Launch {
    pub startup: bool,
    pub target: Option<String>,
}

impl Launch {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut argument = args.next();
        if argument.as_deref().is_some_and(|arg| arg == "--profile") {
            args.next();
            argument = args.next();
        }
        let startup = argument.as_deref().is_some_and(|arg| arg == "--startup");
        let explicit = argument.as_deref().is_some_and(|arg| arg == "--open");
        if explicit {
            argument = args.next();
        }
        let target = if startup {
            None
        } else {
            argument.as_deref().and_then(urls::shell_target)
        };
        if explicit && target.is_none() {
            return Err("Aurora может открыть веб-ссылку или локальный документ HTML".into());
        }
        Ok(Self { startup, target })
    }
}

fn request_dir(profile: &Path) -> PathBuf {
    profile.join("activation")
}

pub fn send(profile: &Path, launch: &Launch) -> io::Result<()> {
    if launch.startup {
        return Ok(());
    }
    let directory = request_dir(profile);
    fs::create_dir_all(&directory)?;
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|error| io::Error::other(error.to_string()))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = format!("{now:020}-{:016x}.request", u64::from_le_bytes(nonce));
    let request = serde_json::to_vec(&launch.target)?;
    if request.len() > MAX_REQUEST {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Ссылка слишком длинная",
        ));
    }
    crate::core::store::write_file(&directory.join(name), &request)
}

fn request_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "request")
        && path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|name| {
                let Some((time, nonce)) = name.split_once('-') else {
                    return false;
                };
                time.len() == 20
                    && time.bytes().all(|byte| byte.is_ascii_digit())
                    && nonce.len() == 16
                    && nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
}

fn consume(directory: &Path) -> Vec<Option<String>> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (request_path(&path) && entry.file_type().ok()?.is_file()).then_some(path)
        })
        .collect();
    entries.sort_unstable();
    entries
        .into_iter()
        .filter_map(|path| {
            let message = read_request(&path);
            fs::remove_file(&path).ok()?;
            message
        })
        .collect()
}

fn read_request(path: &Path) -> Option<Option<String>> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.modified().ok()?.elapsed().ok()? > REQUEST_TTL {
        return None;
    }
    let mut data = Vec::new();
    file.take((MAX_REQUEST + 1) as u64)
        .read_to_end(&mut data)
        .ok()?;
    if data.len() > MAX_REQUEST {
        return None;
    }
    match serde_json::from_slice::<Option<String>>(&data).ok()? {
        Some(target) => Some(Some(urls::shell_target(target.as_ref())?)),
        None => Some(None),
    }
}

pub struct ActivationWatcher {
    _watcher: RecommendedWatcher,
}

pub fn watch(app: &AppHandle, profile: &Path) -> notify::Result<ActivationWatcher> {
    let directory = request_dir(profile);
    fs::create_dir_all(&directory).map_err(notify::Error::io)?;
    let app = app.clone();
    let queued = Arc::new(AtomicBool::new(false));
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let Ok(event) = event else { return };
        if !(event.need_rescan()
            || ((event.kind.is_create() || event.kind.is_modify())
                && event.paths.iter().any(|p| request_path(p))))
            || queued.swap(true, Ordering::Relaxed)
        {
            return;
        }
        let ready = queued.clone();
        browser::defer(&app, move |app| {
            ready.store(false, Ordering::Relaxed);
            drain(&app);
        });
    })?;
    watcher.watch(&directory, RecursiveMode::NonRecursive)?;
    Ok(ActivationWatcher { _watcher: watcher })
}

pub fn drain(app: &AppHandle) {
    let Some(window) = app.get_window("main") else {
        return;
    };
    let profile = app.state::<crate::core::store::AppState>();
    if profile.onboarding.pending() {
        return;
    }
    let requests = consume(&request_dir(&profile.data_dir));
    if requests.is_empty() {
        return;
    }
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    for target in requests.into_iter().flatten() {
        match browser::new_tab(app, Some(target), None, false) {
            Ok(view) => {
                let _ = view.set_focus();
            }
            Err(error) => browser::emit(app, "au://notice", error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_explicit_and_legacy_shell_arguments() {
        let source = "https://example.org/a%20b?one=1&two=2#anchor";
        for args in [
            vec![source],
            vec!["--open", source],
            vec!["--profile", "0000000000000001", "--open", source],
        ] {
            assert_eq!(
                Launch::parse(args.into_iter().map(OsString::from))
                    .unwrap()
                    .target
                    .as_deref(),
                Some(source)
            );
        }
        let invalid = [
            "javascript:alert(1)",
            "http://tauri.localhost/strip.html",
            "--startup",
        ];
        for input in invalid {
            assert!(Launch::parse(["--open", input].map(OsString::from)).is_err());
        }
    }

    #[test]
    fn concurrent_requests_are_consumed_once() {
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).unwrap();
        let profile =
            std::env::temp_dir().join(format!("aurora-activation-{:016x}", u64::from_le_bytes(id)));
        let source = "https://example.org/";
        let open = Launch::parse(["--open", source].map(OsString::from)).unwrap();
        let startup = Launch::parse(["--startup"].map(OsString::from)).unwrap();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| send(&profile, &open).unwrap());
            }
        });
        send(&profile, &startup).unwrap();
        let queue = request_dir(&profile);
        assert_eq!(consume(&queue), vec![Some(source.to_owned()); 4]);
        assert!(consume(&queue).is_empty());
        fs::remove_dir(queue).unwrap();
        fs::remove_dir(profile).unwrap();
    }
}
