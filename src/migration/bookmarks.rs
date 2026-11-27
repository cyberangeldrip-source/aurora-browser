use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;

use super::{read_limit, FILE_LIMIT};
use crate::core::{model::Bookmark, store::Store, urls};

const CHROMIUM_FILES: [&str; 2] = ["Bookmarks", "AccountBookmarks"];
const MAX_DEPTH: usize = 32;

fn folder_name(value: &str) -> Option<String> {
    let name: String = value
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(256)
        .collect();
    (!name.is_empty()).then_some(name)
}

fn bookmark(url: &str, title: &str, folder: Vec<String>, on_bar: bool) -> Option<Bookmark> {
    Some(Bookmark {
        id: 0,
        url: urls::bookmark_url(url)?,
        title: title.trim().to_owned(),
        folder,
        on_bar: Some(on_bar),
    })
}

pub(super) fn has_chromium_files(profile: &Path) -> bool {
    CHROMIUM_FILES
        .iter()
        .any(|file| profile.join(file).is_file())
}

pub(super) fn chromium(profile: &Path) -> Result<(Vec<Bookmark>, bool), ()> {
    let mut links = Vec::new();
    let mut read = false;
    let mut partial = false;
    for file in CHROMIUM_FILES
        .map(|name| profile.join(name))
        .into_iter()
        .filter(|path| path.is_file())
    {
        let Some(data) = read_limit(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        else {
            partial = true;
            continue;
        };
        let Some(roots) = data["roots"].as_object() else {
            partial = true;
            continue;
        };
        read = true;
        if let Some(bar) = roots.get("bookmark_bar") {
            collect_chromium(bar, Vec::new(), true, &mut links);
        }
        for (key, root) in roots
            .iter()
            .filter(|(key, _)| key.as_str() != "bookmark_bar")
        {
            let label = match key.as_str() {
                "other" => "Другие закладки",
                "synced" => "Мобильные закладки",
                _ => key,
            };
            let name = root["name"]
                .as_str()
                .and_then(folder_name)
                .unwrap_or_else(|| label.to_owned());
            collect_chromium(root, vec![name], false, &mut links);
        }
    }
    read.then_some((links, partial)).ok_or(())
}

fn collect_chromium(root: &Value, folder: Vec<String>, on_bar: bool, links: &mut Vec<Bookmark>) {
    let mut pending = Vec::new();
    if let Some(children) = root["children"].as_array() {
        pending.extend(children.iter().rev().map(|child| (child, folder.clone())));
    }
    while let Some((node, mut folder)) = pending.pop() {
        if let Some(url) = node["url"].as_str() {
            links.extend(bookmark(
                url,
                node["name"].as_str().unwrap_or_default(),
                folder,
                on_bar,
            ));
        } else if let Some(children) = node["children"].as_array() {
            if let Some(name) = node["name"]
                .as_str()
                .and_then(folder_name)
                .filter(|_| folder.len() < MAX_DEPTH)
            {
                folder.push(name);
            }
            pending.extend(children.iter().rev().map(|child| (child, folder.clone())));
        }
    }
}

struct FirefoxNode {
    id: i64,
    title: String,
    url: Option<String>,
}

pub(super) fn firefox(db: &Connection) -> rusqlite::Result<Vec<Bookmark>> {
    let mut query = db.prepare(
        "SELECT b.id,b.parent,b.guid,b.type,COALESCE(b.title,p.title,''),p.url
         FROM moz_bookmarks b LEFT JOIN moz_places p ON b.fk=p.id
         WHERE b.type IN (1,2) ORDER BY b.parent,b.position,b.id",
    )?;
    let rows = query.query_map([], |row| {
        let kind: i64 = row.get(3)?;
        let node = FirefoxNode {
            id: row.get(0)?,
            title: row.get(4)?,
            url: row.get(5)?,
        };
        Ok((row.get::<_, i64>(1)?, row.get::<_, String>(2)?, kind, node))
    })?;
    let mut children: HashMap<i64, Vec<FirefoxNode>> = HashMap::new();
    let mut roots = HashMap::new();
    let known = [
        ("toolbar_____", None),
        ("menu________", Some("Меню закладок")),
        ("unfiled_____", Some("Другие закладки")),
        ("mobile______", Some("Мобильные закладки")),
    ];
    for row in rows.filter_map(Result::ok) {
        let (parent, guid, kind, node) = row;
        if kind == 2 && known.iter().any(|(key, _)| *key == guid) {
            roots.insert(guid, node);
        } else if kind == 2 || node.url.is_some() {
            children.entry(parent).or_default().push(node);
        }
    }
    let mut links = Vec::new();
    for (key, label) in known {
        let Some(root) = roots.remove(key) else {
            continue;
        };
        let folder: Vec<_> = label
            .map(|name| folder_name(&root.title).unwrap_or_else(|| name.to_owned()))
            .into_iter()
            .collect();
        let mut pending = Vec::new();
        pending.extend(
            children
                .remove(&root.id)
                .into_iter()
                .flatten()
                .rev()
                .map(|node| (node, folder.clone())),
        );
        while let Some((node, mut folder)) = pending.pop() {
            if let Some(url) = node.url {
                links.extend(bookmark(&url, &node.title, folder, label.is_none()));
            } else if let Some(items) = children.remove(&node.id) {
                if let Some(name) = folder_name(&node.title).filter(|_| folder.len() < MAX_DEPTH) {
                    folder.push(name);
                }
                pending.extend(items.into_iter().rev().map(|node| (node, folder.clone())));
            }
        }
    }
    Ok(links)
}

pub fn html(document: &str) -> Result<Vec<Bookmark>, String> {
    if document.len() as u64 > FILE_LIMIT {
        return Err("Файл закладок слишком большой".into());
    }
    let document = Html::parse_document(document);
    let selector = Selector::parse("a[href]").expect("valid selector");
    let heading = Selector::parse(":scope > h3").expect("valid selector");
    let toolbar = Selector::parse("h3[personal_toolbar_folder]").expect("valid selector");
    let has_toolbar = document.select(&toolbar).next().is_some();
    let links: Vec<_> = document
        .select(&selector)
        .filter_map(|tag| {
            let mut on_bar = !has_toolbar;
            let mut folder = Vec::new();
            for item in tag
                .ancestors()
                .filter_map(ElementRef::wrap)
                .filter_map(|ancestor| {
                    (ancestor.value().name() == "dt")
                        .then(|| ancestor.select(&heading).next())
                        .flatten()
                })
            {
                if item.value().attr("personal_toolbar_folder").is_some() {
                    on_bar = true;
                } else if folder.len() < MAX_DEPTH {
                    if let Some(name) = folder_name(&item.text().collect::<String>()) {
                        folder.push(name);
                    }
                }
            }
            folder.reverse();
            bookmark(
                tag.value().attr("href")?,
                &tag.text().collect::<String>(),
                folder,
                on_bar,
            )
        })
        .collect();
    if links.is_empty() {
        return Err("В файле нет поддерживаемых закладок".into());
    }
    Ok(links)
}

pub(super) fn merge(store: &mut Store, mut links: Vec<Bookmark>) -> (usize, usize, bool) {
    if links.is_empty() {
        return (0, 0, false);
    }
    links.sort_by_key(|item| !item.is_on_bar());
    let key = |item: &Bookmark| (item.url.clone(), item.folder.clone(), item.is_on_bar());
    let existing: HashMap<_, _> = store
        .bookmarks
        .iter()
        .enumerate()
        .map(|(index, item)| (key(item), index))
        .collect();
    let mut seen = HashSet::new();
    let mut rank = HashMap::new();
    let mut added = 0;
    let mut changed = false;
    for item in links {
        let on_bar = item.is_on_bar();
        let placement = key(&item);
        if !seen.insert(placement.clone()) {
            continue;
        }
        let index = existing.get(&placement).copied().or_else(|| {
            if on_bar {
                return None;
            }
            let (url, folder, _) = &placement;
            existing
                .get(&(url.clone(), folder.clone(), true))
                .copied()
                .filter(|&index| store.bookmarks[index].on_bar.is_none())
        });
        let id = if let Some(index) = index {
            let current = &mut store.bookmarks[index];
            if current.on_bar != Some(on_bar) {
                current.on_bar = Some(on_bar);
                changed = true;
            }
            current.id
        } else {
            added += 1;
            store.insert_bookmark(item)
        };
        if on_bar {
            rank.insert(id, rank.len());
        }
    }
    let order = |item: &Bookmark| rank.get(&item.id).copied().unwrap_or(usize::MAX);
    if store
        .bookmarks
        .windows(2)
        .any(|items| order(&items[0]) > order(&items[1]))
    {
        store.bookmarks.sort_by_key(order);
        changed = true;
    }
    (added, rank.len(), changed || added != 0)
}
