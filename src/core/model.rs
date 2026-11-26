//! Модель данных Aurora: настройки, вкладки, история, закладки, плагины.

use serde::{Deserialize, Serialize};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------- Тема / палитра ----------

// ---------- Настройки ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "dark" | "light"
    pub theme: String,
    /// HEX-цвет акцента, например "#7C5CFF"
    pub accent: String,
    /// Непрозрачность стеклянных панелей: 0.05 .. 0.45
    pub glass_opacity: f64,
    /// Сила blur в px: 4 .. 64
    pub blur_strength: f64,
    /// Плавные анимации
    pub animations: bool,
    pub animated_wallpaper: bool,
    /// Компактная строка адреса
    pub compact: bool,
    /// Показывать ряд вкладок
    pub show_tab_row: bool,
    pub show_bookmark_bar: bool,
    /// "google" | "yandex" | "duckduckgo" | "bing"
    pub search_engine: String,
    /// Стартовая страница
    pub homepage: String,
    /// Хранение истории, дней. 0 — вечно
    pub history_retention_days: u32,
    /// Масштаб интерфейса 0.85 .. 1.15
    pub ui_scale: f64,
    pub restore_session: bool,
    pub record_history: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            accent: "#7C5CFF".into(),
            glass_opacity: 0.16,
            blur_strength: 22.0,
            animations: true,
            animated_wallpaper: true,
            compact: false,
            show_tab_row: true,
            show_bookmark_bar: true,
            search_engine: "google".into(),
            homepage: "aurora://home/".into(),
            history_retention_days: 30,
            ui_scale: 1.0,
            restore_session: true,
            record_history: true,
        }
    }
}

impl Settings {
    /// Клампит значения в безопасные диапазоны (после чтения JSON-файла).
    pub fn sanitize(&mut self) {
        self.theme = if self.theme == "light" {
            "light".into()
        } else {
            "dark".into()
        };
        if self.accent.len() != 7
            || !self.accent.starts_with('#')
            || !self.accent[1..].bytes().all(|c| c.is_ascii_hexdigit())
        {
            self.accent = "#7C5CFF".into();
        }
        self.glass_opacity = self.glass_opacity.clamp(0.05, 0.45);
        self.blur_strength = self.blur_strength.clamp(4.0, 64.0);
        self.search_engine = match self.search_engine.as_str() {
            "yandex" | "duckduckgo" | "bing" => self.search_engine.clone(),
            _ => "google".into(),
        };
        self.history_retention_days = self.history_retention_days.min(3650);
        self.ui_scale = self.ui_scale.clamp(0.85, 1.15);
        self.homepage = crate::core::urls::resolve_input(&self.homepage, self);
    }
}

// ---------- Вкладки ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
    pub id: u32,
    /// Label webview-а этой вкладки (у каждой вкладки — свой живой webview).
    /// Поле с default, чтобы старые tabs.json из прошлых версий открывались.
    #[serde(default)]
    pub label: String,
    pub url: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<TabGroup>,
    #[serde(
        default,
        skip_deserializing,
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub private: bool,
    #[serde(default, skip)]
    pub loading: bool,
    #[serde(default, skip)]
    pub failed: bool,
    #[serde(default, skip)]
    pub can_back: bool,
    #[serde(default, skip)]
    pub can_forward: bool,
    #[serde(default, skip)]
    pub zoom: Option<f64>,
    #[serde(skip)]
    pub favicon: Option<std::sync::Arc<[u8]>>,
    #[serde(skip)]
    pub favicon_revision: u32,
}

impl Tab {
    pub fn new(id: u32, url: String) -> Self {
        Self {
            id,
            label: format!("tab-{id}"),
            title: crate::core::urls::title_for_url(&url),
            group: None,
            url,
            private: false,
            loading: false,
            failed: false,
            can_back: false,
            can_forward: false,
            zoom: None,
            favicon: None,
            favicon_revision: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabGroup {
    pub id: String,
    pub name: String,
    pub color: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TabsState {
    pub tabs: Vec<Tab>,
    pub active_id: u32,
}

// ---------- История ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u64,
    pub url: String,
    pub title: String,
    /// Unix-время в секундах
    pub ts: u64,
}

// ---------- Закладки ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: u64,
    pub url: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folder: Vec<String>,
}

// ---------- Плагины ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    /// Где выполняется: "web" (http/https) | "all" (включая внутренние страницы)
    #[serde(default = "default_run_on")]
    pub run_on: String,
    /// Теги: "adblock", "privacy", "cosmetic", "custom"…
    #[serde(default)]
    pub permissions: Vec<String>,
}

fn default_run_on() -> String {
    "web".into()
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub manifest: PluginManifest,
    pub enabled: bool,
    pub has_css: bool,
    pub has_js: bool,
    pub hosts: Vec<String>,
}
