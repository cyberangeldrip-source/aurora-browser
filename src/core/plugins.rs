use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::core::bundled;
use crate::core::model::{PluginInfo, PluginManifest};
use crate::core::store::write_file;

const MAX_PLUGIN_FILE: usize = 512 * 1024;

fn read_local_file(directory: &Path, name: &str) -> Option<String> {
    let path = directory.join(name);
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_PLUGIN_FILE as u64 {
        return None;
    }
    let mut data = String::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_PLUGIN_FILE as u64 + 1)
        .read_to_string(&mut data)
        .ok()?;
    (data.len() <= MAX_PLUGIN_FILE).then_some(data)
}

pub struct PluginManager {
    dir: PathBuf,
    enabled: HashMap<String, bool>,
    entries: Vec<PluginInfo>,
    script: String,
}

impl PluginManager {
    pub fn init(data_dir: &Path) -> Self {
        let dir = data_dir.join("plugins");
        let _ = fs::create_dir_all(&dir);
        for (id, manifest, css, js) in bundled::bundled_plugins() {
            let plugin_dir = dir.join(id);
            let _ = fs::create_dir_all(&plugin_dir);
            for (file, contents) in [
                ("manifest.json", manifest),
                ("style.css", css),
                ("script.js", js),
            ] {
                let target = plugin_dir.join(file);
                if !target.exists() {
                    let _ = fs::write(target, contents);
                }
            }
        }
        let enabled = fs::read(dir.join("plugins.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut manager = Self {
            dir,
            enabled,
            entries: Vec::new(),
            script: String::new(),
        };
        manager.refresh();
        manager
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn refresh(&mut self) {
        self.entries.clear();
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let id = entry.file_name().to_string_lossy().into_owned();
                if !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
                {
                    continue;
                }
                let Some(location) = self.local_directory(&id) else {
                    continue;
                };
                let Some(document) = read_local_file(&location, "manifest.json") else {
                    continue;
                };
                let Ok(mut manifest) = serde_json::from_str::<PluginManifest>(&document) else {
                    continue;
                };
                manifest.id = id;
                self.entries.push(PluginInfo {
                    enabled: self.enabled.get(&manifest.id).copied().unwrap_or(false),
                    has_css: location.join("style.css").is_file(),
                    has_js: location.join("script.js").is_file(),
                    manifest,
                });
            }
        }
        self.entries
            .sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
        self.compile_script();
    }

    pub fn list(&self) -> &[PluginInfo] {
        &self.entries
    }

    pub fn enabled_count(&self) -> usize {
        self.entries.iter().filter(|plugin| plugin.enabled).count()
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> io::Result<()> {
        if !self.entries.iter().any(|entry| entry.manifest.id == id) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Плагин не найден"));
        }
        let mut updated = self.enabled.clone();
        updated.insert(id.to_owned(), enabled);
        write_file(
            &self.dir.join("plugins.json"),
            &serde_json::to_vec_pretty(&updated)?,
        )?;
        self.enabled = updated;
        self.refresh();
        Ok(())
    }

    fn location(&self, id: &str) -> Option<PathBuf> {
        self.entries
            .iter()
            .find(|entry| entry.manifest.id == id)
            .and_then(|_| self.local_directory(id))
    }

    fn local_directory(&self, id: &str) -> Option<PathBuf> {
        let root = self.dir.canonicalize().ok()?;
        let path = root.join(id);
        (path.canonicalize().ok()? == path && path.is_dir()).then_some(path)
    }

    pub fn get_code(&self, id: &str) -> Option<(String, String)> {
        let path = self.location(id)?;
        Some((
            read_local_file(&path, "style.css").unwrap_or_default(),
            read_local_file(&path, "script.js").unwrap_or_default(),
        ))
    }

    pub fn set_code(&mut self, id: &str, css: &str, js: &str) -> io::Result<()> {
        if css.len() > MAX_PLUGIN_FILE || js.len() > MAX_PLUGIN_FILE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Файл плагина превышает 512 КБ",
            ));
        }
        let path = self
            .location(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Плагин не найден"))?;
        write_file(&path.join("style.css"), css.as_bytes())?;
        write_file(&path.join("script.js"), js.as_bytes())?;
        self.refresh();
        Ok(())
    }

    fn compile_script(&mut self) {
        let mut css = String::new();
        let mut scripts = Vec::new();
        for plugin in &self.entries {
            if !plugin.enabled || !matches!(plugin.manifest.run_on.as_str(), "web" | "all") {
                continue;
            }
            if let Some((style, js)) = self.get_code(&plugin.manifest.id) {
                css.push_str(&style);
                css.push('\n');
                if !js.trim().is_empty() {
                    scripts.push(js);
                }
            }
        }
        if css.trim().is_empty() && scripts.is_empty() {
            self.script.clear();
            return;
        }
        let style = serde_json::to_string(&css).expect("serialization of a string");
        let scripts = serde_json::to_string(&scripts).expect("serialization of strings");
        self.script = format!(
            r#"(function () {{
  if (location.protocol !== 'https:' && location.protocol !== 'http:') return;
  if (location.hostname === 'aurora.localhost' || location.hostname === 'tauri.localhost') return;
  const root = document.head || document.documentElement;
  if (!root || document.getElementById('aurora-plugins-css')) return;
  const style = document.createElement('style');
  style.id = 'aurora-plugins-css';
  style.textContent = {style};
  root.appendChild(style);
  for (const source of {scripts}) {{
    try {{ (0, eval)(source); }} catch (error) {{ console.warn('Aurora plugin', error); }}
  }}
}})();"#
        );
    }

    pub fn script(&self) -> &str {
        &self.script
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_plugins_are_opt_in_and_changes_persist() {
        let path = std::env::temp_dir().join(format!("aurora-plugins-test-{}", std::process::id()));
        let mut manager = PluginManager::init(&path);
        assert!(manager
            .list()
            .iter()
            .any(|p| p.manifest.id == "adblock-cosmetic"));
        assert!(manager.script().is_empty());
        manager.set_enabled("adblock-cosmetic", true).unwrap();
        assert!(manager.script().contains("doubleclick.net"));
        assert!(manager.script().contains("AD_HOSTS"));
        let mut manager = PluginManager::init(&path);
        assert!(manager.script().contains("AD_HOSTS"));
        manager.set_enabled("adblock-cosmetic", false).unwrap();
        assert!(manager.script().is_empty());
        let _ = fs::remove_dir_all(path);
    }
}
