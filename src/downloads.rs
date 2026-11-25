use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const HISTORY_LIMIT: usize = 150;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Paused,
    Complete,
    Interrupted,
    Canceled,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Download {
    pub id: u64,
    pub name: String,
    pub path: PathBuf,
    pub status: Status,
    pub received: u64,
    pub total: Option<u64>,
    pub can_resume: bool,
    #[serde(default)]
    pub private: bool,
}

#[derive(Default)]
pub struct Downloads {
    entries: Vec<Download>,
    next_id: u64,
}

impl Downloads {
    pub fn load(dir: &Path) -> Self {
        let mut entries: Vec<Download> = std::fs::read(dir.join("downloads.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        entries.retain(|item| !item.private && item.id != 0 && item.id != u64::MAX);
        entries.truncate(HISTORY_LIMIT);
        for item in &mut entries {
            if matches!(item.status, Status::Running | Status::Paused) {
                item.status = Status::Interrupted;
            }
            item.can_resume = false;
        }
        let next_id = entries.iter().map(|item| item.id).max().unwrap_or(0) + 1;
        Self { entries, next_id }
    }

    #[cfg(windows)]
    pub fn start(&mut self, path: PathBuf, total: Option<u64>, private: bool) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let name = file_name(&path);
        self.entries.insert(
            0,
            Download {
                id,
                name,
                path,
                status: Status::Running,
                received: 0,
                total,
                can_resume: false,
                private,
            },
        );
        self.trim();
        id
    }

    fn trim(&mut self) {
        let mut recent = 0;
        self.entries.retain(|item| {
            if item.can_resume || matches!(item.status, Status::Running | Status::Paused) {
                return true;
            }
            recent += 1;
            recent <= HISTORY_LIMIT
        });
    }

    pub fn merge_imported(&mut self, entries: Vec<Download>) -> usize {
        let first = self.next_id.max(1);
        self.next_id = first;
        let mut known: std::collections::HashSet<_> = self
            .entries
            .iter()
            .filter(|item| !item.private)
            .map(|item| item.path.clone())
            .collect();
        for mut item in entries {
            if known.insert(item.path.clone()) {
                item.id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1).max(1);
                self.entries.push(item);
            }
        }
        self.trim();
        self.entries.iter().filter(|item| item.id >= first).count()
    }

    #[cfg(windows)]
    pub fn progress(&mut self, id: u64, received: u64, total: Option<u64>) {
        if let Some(item) = self.entries.iter_mut().find(|item| item.id == id) {
            item.received = received;
            item.total = total;
        }
    }

    #[cfg(windows)]
    pub fn update(&mut self, id: u64, status: Status, can_resume: bool, path: Option<PathBuf>) {
        if let Some(item) = self.entries.iter_mut().find(|item| item.id == id) {
            item.status = status;
            item.can_resume = can_resume;
            if let Some(path) = path {
                item.name = file_name(&path);
                item.path = path;
            }
        }
        self.trim();
    }

    pub fn visible(&self, private: bool) -> Vec<Download> {
        self.entries
            .iter()
            .filter(|item| item.private == private)
            .cloned()
            .collect()
    }

    pub fn persisted(&self) -> Vec<Download> {
        self.entries
            .iter()
            .filter(|item| !item.private)
            .take(HISTORY_LIMIT)
            .cloned()
            .collect()
    }
}

#[cfg(windows)]
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Файл".into())
}
