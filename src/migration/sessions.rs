use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{read_limit, Engine, Links, FILE_LIMIT};
use crate::core::model::TabGroup;

pub(super) fn files(profile: &Path, engine: Engine) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if engine == Engine::Chromium {
        files.extend(
            fs::read_dir(profile.join("Sessions"))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("Session_"))
                .map(|entry| entry.path()),
        );
        files.sort_unstable_by(|left, right| right.cmp(left));
        files.extend(
            ["Current Session", "Last Session"]
                .map(|name| profile.join(name))
                .into_iter()
                .filter(|path| path.is_file()),
        );
    } else {
        files.extend(
            [
                "sessionstore.jsonlz4",
                "sessionstore-backups/recovery.jsonlz4",
                "sessionstore-backups/recovery.baklz4",
                "sessionstore-backups/previous.jsonlz4",
            ]
            .map(|name| profile.join(name))
            .into_iter()
            .filter(|path| path.is_file()),
        );
        files.sort_by_cached_key(|path| {
            std::cmp::Reverse(fs::metadata(path).and_then(|meta| meta.modified()).ok())
        });
    }
    files
}

pub(super) fn load(profile: &Path, engine: Engine) -> Result<(Links, bool), ()> {
    for (index, file) in files(profile, engine).into_iter().enumerate() {
        let Ok(bytes) = read_limit(&file) else {
            continue;
        };
        let parsed = match engine {
            Engine::Chromium => chromium(&bytes),
            Engine::Firefox => firefox(&bytes),
        };
        if let Some(tabs) = parsed {
            return Ok((tabs, index != 0));
        }
    }
    Err(())
}

fn firefox(bytes: &[u8]) -> Option<Links> {
    let data = bytes.strip_prefix(b"mozLz40\0")?;
    let size = u32::from_le_bytes(data.get(..4)?.try_into().ok()?) as u64;
    if size > FILE_LIMIT {
        return None;
    }
    let bytes = lz4_flex::decompress_size_prepended(data).ok()?;
    let session: Value = serde_json::from_slice(&bytes).ok()?;
    let mut links = Vec::new();
    for window in session["windows"].as_array()? {
        if window["isPrivate"].as_bool().unwrap_or(false) {
            continue;
        }
        for tab in window["tabs"].as_array().into_iter().flatten() {
            let Some(entries) = tab["entries"].as_array() else {
                continue;
            };
            let index = tab["index"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(entries.len());
            if let Some(entry) = entries.get(index.saturating_sub(1)) {
                if let Some(url) = entry["url"].as_str() {
                    links.push((
                        url.into(),
                        entry["title"].as_str().unwrap_or_default().into(),
                        firefox_group(window, tab),
                    ));
                }
            }
        }
    }
    Some(links)
}

fn firefox_group(window: &Value, tab: &Value) -> Option<TabGroup> {
    let id = tab["groupId"].as_str()?;
    let group = window["groups"]
        .as_array()?
        .iter()
        .find(|entry| entry["id"].as_str() == Some(id))?;
    let palette = [
        "gray", "blue", "red", "yellow", "green", "pink", "purple", "orange", "cyan",
    ];
    Some(TabGroup {
        id: id.chars().take(120).collect(),
        name: group["name"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(100)
            .collect(),
        color: palette
            .iter()
            .position(|name| group["color"].as_str() == Some(*name))
            .unwrap_or(0) as u8,
        collapsed: group["collapsed"].as_bool().unwrap_or(false),
    })
}

#[derive(Default)]
struct SessionTab {
    window: i32,
    position: i32,
    selected: Option<i32>,
    group: Option<[u8; 16]>,
    navigations: BTreeMap<i32, (String, String)>,
}

fn int(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn navigation(mut bytes: &[u8]) -> Option<(i32, i32, String, String)> {
    let size = usize::try_from(int(bytes, 0)?).ok()?;
    bytes = bytes.get(4..4 + size)?;
    let tab = int(bytes, 0)?;
    let index = int(bytes, 4)?;
    let url_size = usize::try_from(int(bytes, 8)?).ok()?;
    let url = std::str::from_utf8(bytes.get(12..12 + url_size)?)
        .ok()?
        .to_owned();
    let title_offset = 12 + url_size.next_multiple_of(4);
    let title_size = usize::try_from(int(bytes, title_offset)?)
        .ok()?
        .checked_mul(2)?;
    let title = char::decode_utf16(
        bytes
            .get(title_offset + 4..title_offset + 4 + title_size)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes(*chunk)),
    )
    .map(|character| character.unwrap_or(char::REPLACEMENT_CHARACTER))
    .collect();
    Some((tab, index, url, title))
}

fn chromium(bytes: &[u8]) -> Option<Links> {
    if bytes.get(..4)? != b"SNSS" || !matches!(int(bytes, 4)?, 1 | 3) {
        return None;
    }
    let mut records = bytes.get(8..)?;
    let mut tabs: HashMap<i32, SessionTab> = HashMap::new();
    let mut windows = HashMap::new();
    let mut groups = HashMap::new();
    let mut valid = false;
    while let Some(header) = records.get(..2) {
        let size = u16::from_le_bytes([header[0], header[1]]) as usize;
        let Some(record) = records.get(2..2 + size) else {
            break;
        };
        let Some((&command, payload)) = record.split_first() else {
            break;
        };
        records = &records[size + 2..];
        let first = int(payload, 0);
        let second = int(payload, 4);
        match (command, first, second) {
            (0, Some(window), Some(id)) => tabs.entry(id).or_default().window = window,
            (2, Some(id), Some(position)) => tabs.entry(id).or_default().position = position,
            (5, Some(id), Some(index)) => tabs
                .entry(id)
                .or_default()
                .navigations
                .retain(|key, _| *key < index),
            (6, _, _) => {
                if let Some((id, index, url, title)) = navigation(payload) {
                    valid = true;
                    tabs.entry(id)
                        .or_default()
                        .navigations
                        .insert(index, (url, title));
                }
            }
            (7, Some(id), Some(index)) => tabs.entry(id).or_default().selected = Some(index),
            (9, Some(id), Some(kind)) => {
                windows.insert(id, kind);
            }
            (16, Some(id), _) => {
                tabs.remove(&id);
            }
            (17, Some(id), _) => tabs.retain(|_, tab| tab.window != id),
            (24, Some(id), Some(index)) => {
                if let Some(count) = int(payload, 8).filter(|count| *count > 0) {
                    let tab = tabs.entry(id).or_default();
                    let end = index.saturating_add(count);
                    tab.navigations = std::mem::take(&mut tab.navigations)
                        .into_iter()
                        .filter_map(|(key, entry)| {
                            if key < index {
                                Some((key, entry))
                            } else if key >= end {
                                Some((key - count, entry))
                            } else {
                                None
                            }
                        })
                        .collect();
                }
            }
            (25, Some(id), _) => {
                let offset = if payload.len() >= 32 { 8 } else { 4 };
                let group = payload
                    .get(offset..offset + 16)
                    .and_then(|bytes| bytes.try_into().ok());
                let present = payload.get(offset + 16) == Some(&1);
                tabs.entry(id).or_default().group = group.filter(|_| present);
            }
            (27, _, _) => {
                if let Some((token, group)) = group_metadata(payload) {
                    groups.insert(token, group);
                }
            }
            _ => {}
        }
    }
    if !valid {
        return None;
    }
    let mut tabs: Vec<_> = tabs.into_iter().collect();
    tabs.sort_by_key(|(id, tab)| (tab.window, tab.position, *id));
    Some(
        tabs.into_iter()
            .filter_map(|(_, mut tab)| {
                if windows
                    .get(&tab.window)
                    .is_some_and(|kind| !matches!(kind, 0 | 1))
                {
                    return None;
                }
                let index = tab
                    .selected
                    .and_then(|selected| {
                        tab.navigations
                            .range(..=selected)
                            .next_back()
                            .map(|(index, _)| *index)
                    })
                    .or_else(|| tab.navigations.last_key_value().map(|(index, _)| *index))?;
                let (url, title) = tab.navigations.remove(&index)?;
                Some((
                    url,
                    title,
                    tab.group.and_then(|id| groups.get(&id).cloned()),
                ))
            })
            .collect(),
    )
}

fn group_metadata(bytes: &[u8]) -> Option<([u8; 16], TabGroup)> {
    let size = usize::try_from(int(bytes, 0)?).ok()?;
    let bytes = bytes.get(4..4 + size)?;
    let token: [u8; 16] = bytes.get(..16)?.try_into().ok()?;
    let length = usize::try_from(int(bytes, 16)?).ok()?.checked_mul(2)?;
    let title = bytes
        .get(20..20 + length)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair));
    let name: String = char::decode_utf16(title)
        .take(100)
        .map(|character| character.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    let color = int(bytes, 20 + length.next_multiple_of(4))?.clamp(0, 8) as u8;
    let id = token.iter().map(|byte| format!("{byte:02x}")).collect();
    let collapsed = int(bytes, 24 + length.next_multiple_of(4)) == Some(1);
    Some((
        token,
        TabGroup {
            id,
            name,
            color,
            collapsed,
        },
    ))
}

#[cfg(test)]
pub(super) mod tests {
    pub(crate) fn append(bytes: &mut Vec<u8>, command: u8, payload: &[u8]) {
        bytes.extend_from_slice(&(payload.len() as u16 + 1).to_le_bytes());
        bytes.push(command);
        bytes.extend_from_slice(payload);
    }

    pub(crate) fn navigation(tab: i32, index: i32, url: &str) -> Vec<u8> {
        let mut data = Vec::new();
        for field in [tab, index, url.len() as i32] {
            data.extend_from_slice(&field.to_le_bytes());
        }
        data.extend_from_slice(url.as_bytes());
        data.resize(data.len().next_multiple_of(4), 0);
        data.extend_from_slice(&0_i32.to_le_bytes());
        let mut result = (data.len() as u32).to_le_bytes().to_vec();
        result.extend(data);
        result
    }

    #[test]
    fn chromium_uses_the_selected_navigation_and_ignores_closed_tabs() {
        let mut bytes = b"SNSS\x03\0\0\0".to_vec();
        append(&mut bytes, 6, &navigation(1, 0, "https://example.org/"));
        append(&mut bytes, 6, &navigation(1, 1, "https://example.net/"));
        append(&mut bytes, 7, &[1, 0, 0, 0, 0, 0, 0, 0]);
        append(&mut bytes, 6, &navigation(2, 0, "https://example.com/"));
        append(&mut bytes, 16, &2_i32.to_le_bytes());
        bytes.extend([8, 0, 6]);
        assert_eq!(
            super::chromium(&bytes).unwrap(),
            [("https://example.org/".into(), String::new(), None)]
        );
    }

    #[test]
    fn chromium_restores_tab_group_metadata() {
        let mut bytes = b"SNSS\x03\0\0\0".to_vec();
        let token = [0x12; 16];
        append(&mut bytes, 6, &navigation(1, 0, "https://example.org/"));

        let mut membership = [0; 32];
        membership[..4].copy_from_slice(&1_i32.to_le_bytes());
        membership[8..24].copy_from_slice(&token);
        membership[24] = 1;
        append(&mut bytes, 25, &membership);

        let mut metadata = 32_i32.to_le_bytes().to_vec();
        metadata.extend(token);
        metadata.extend(3_i32.to_le_bytes());
        for value in "Код".encode_utf16() {
            metadata.extend(value.to_le_bytes());
        }
        metadata.extend([0, 0]);
        metadata.extend(4_i32.to_le_bytes());
        append(&mut bytes, 27, &metadata);

        let tabs = super::chromium(&bytes).unwrap();
        let group = tabs[0].2.as_ref().unwrap();
        assert_eq!(group.name, "Код");
        assert_eq!(group.color, 4);
        assert_eq!(group.id, "12".repeat(16));
    }
}
