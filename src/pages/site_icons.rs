use std::borrow::Cow;
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};

use url::Url;

const NO_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" fill="none" stroke="#b9cbea" stroke-width="1.7"><circle cx="16" cy="16" r="11"/><ellipse cx="16" cy="16" rx="5" ry="11"/><path d="M5 16h22"/></svg>"##;
pub const MAX_ICON_BYTES: usize = 128 * 1024;

fn builtin(url: &Url, light: bool) -> Option<&'static [u8]> {
    Some(match url.host_str()?.trim_start_matches("www.") {
        "github.com" if light => include_bytes!("../../icons/sites/github-light.png"),
        "github.com" => include_bytes!("../../icons/sites/github.png"),
        "youtube.com" | "m.youtube.com" | "youtu.be" => {
            include_bytes!("../../icons/sites/youtube.png")
        }
        "ru.wikipedia.org" => include_bytes!("../../icons/sites/wikipedia.png"),
        "habr.com" => include_bytes!("../../icons/sites/habr.png"),
        "vk.com" | "m.vk.com" | "vk.ru" => include_bytes!("../../icons/sites/vk.png"),
        "web.telegram.org" => include_bytes!("../../icons/sites/telegram.png"),
        "rust-lang.org" if light => include_bytes!("../../icons/sites/rust.png"),
        "rust-lang.org" => include_bytes!("../../icons/sites/rust-dark.png"),
        "music.yandex.ru" => include_bytes!("../../icons/sites/yandex-music.png"),
        _ => return None,
    })
}

pub fn site_key(url: &Url) -> Option<(&str, Option<u16>)> {
    crate::core::urls::is_web(url).then_some(())?;
    Some((url.host_str()?.trim_start_matches("www."), url.port()))
}

pub fn cache_path(profile: &Path, url: &Url) -> Option<PathBuf> {
    if builtin(url, false).is_some() {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    site_key(url)?.hash(&mut hasher);
    Some(
        profile
            .join("site-icons")
            .join(format!("{:016x}.png", hasher.finish())),
    )
}

pub fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 24 && bytes.len() <= MAX_ICON_BYTES && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

fn read_icon(path: &Path) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take((MAX_ICON_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    is_png(&bytes).then_some(bytes)
}

pub fn image(profile: &Path, address: &str, light: bool) -> (&'static str, Cow<'static, [u8]>) {
    if let Ok(url) = Url::parse(address) {
        if let Some(icon) = builtin(&url, light) {
            return ("image/png", Cow::Borrowed(icon));
        }
        if let Some(icon) = cache_path(profile, &url).as_deref().and_then(read_icon) {
            return ("image/png", Cow::Owned(icon));
        }
    }
    ("image/svg+xml", Cow::Borrowed(NO_ICON))
}

#[cfg(windows)]
pub fn store(path: &Path, bytes: &[u8]) {
    use std::sync::Mutex;
    static WRITER: Mutex<()> = Mutex::new(());

    if !is_png(bytes) {
        return;
    }
    let _guard = WRITER.lock().unwrap_or_else(|error| error.into_inner());
    if path.is_file() {
        return;
    }
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_ok() {
            let _ = crate::core::store::write_file(path, bytes);
            if let Ok(entries) = std::fs::read_dir(dir) {
                let mut cached: Vec<_> = entries
                    .flatten()
                    .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
                    .collect();
                if cached.len() > 512 {
                    cached.sort_unstable_by_key(|entry| {
                        entry.metadata().and_then(|info| info.modified()).ok()
                    });
                    let excess = cached.len() - 512;
                    for entry in cached.into_iter().take(excess) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}
