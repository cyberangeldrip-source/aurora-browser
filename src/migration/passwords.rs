use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::Webview;
use url::Url;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const VAULT_LIMIT: usize = 16 * 1024 * 1024;
const CSV_LIMIT: usize = 8 * 1024 * 1024;
const VAULT_FILE: &str = "passwords.dpapi";
static VAULT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct Login {
    #[serde(default)]
    id: String,
    origin: String,
    username: String,
    password: String,
}

#[derive(Serialize)]
pub struct PasswordImport {
    pub imported: usize,
    pub duplicates: usize,
    pub skipped: usize,
}

#[derive(Serialize)]
pub struct VaultRecord {
    pub id: String,
    pub site: String,
    pub username: String,
    pub variant: Option<usize>,
}

fn stored_origin(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    crate::core::urls::is_web(&url).then(|| url.origin().ascii_serialization())
}

pub fn origin(raw: &str) -> Option<String> {
    stored_origin(raw).filter(|site| site.starts_with("https://"))
}

fn new_id() -> Result<String, String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| "Не удалось создать идентификатор входа")?;
    Ok(format!("{:032x}", u128::from_le_bytes(bytes)))
}

impl Login {
    fn key(&self) -> (&str, &str, &str) {
        (&self.origin, &self.username, &self.password)
    }
}

fn records(entries: &[Login]) -> Vec<VaultRecord> {
    entries
        .chunk_by(|a, b| a.origin == b.origin && a.username == b.username)
        .flat_map(|group| {
            group.iter().enumerate().map(|(index, entry)| VaultRecord {
                id: entry.id.clone(),
                site: entry.origin.clone(),
                username: entry.username.clone(),
                variant: (group.len() > 1).then_some(index + 1),
            })
        })
        .collect()
}

fn protect(data: &[u8], encrypt: bool) -> Result<Zeroizing<Vec<u8>>, String> {
    let limit = if encrypt {
        VAULT_LIMIT
    } else {
        VAULT_LIMIT + 65_536
    };
    if data.is_empty() || data.len() > limit {
        return Err("Недопустимый размер хранилища".into());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        let result = if encrypt {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        result.map_err(|_| "Windows не смогла открыть или зашифровать хранилище".to_owned())?;
        if output.pbData.is_null() {
            return Err("Windows вернула пустое хранилище".into());
        }
        let buffer = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let copy = Zeroizing::new(buffer.to_vec());
        buffer.zeroize();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(copy)
    }
}

fn load(profile: &Path) -> Result<Vec<Login>, String> {
    use std::io::Read;
    let file = match std::fs::File::open(profile.join(VAULT_FILE)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Не удалось прочитать хранилище Aurora".into()),
    };
    let mut bytes = Vec::new();
    file.take(VAULT_LIMIT as u64 + 65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "Не удалось прочитать хранилище Aurora")?;
    let plaintext = protect(&bytes, false)?;
    serde_json::from_slice(&plaintext).map_err(|_| "Хранилище Aurora повреждено".into())
}

fn parse(input: &str) -> Result<(Vec<Login>, usize), String> {
    if input.len() > CSV_LIMIT {
        return Err("Размер CSV превышает 8 МБ".into());
    }
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(input.trim_start_matches('\u{feff}').as_bytes());
    let header = reader
        .headers()
        .map_err(|_| "Не удалось прочитать заголовки CSV")?;
    let column = |names: &[&str]| {
        header.iter().position(|value| {
            names
                .iter()
                .any(|name| value.trim().eq_ignore_ascii_case(name))
        })
    };
    let address = column(&["url", "origin", "website"]).ok_or("В CSV нет столбца url")?;
    let user = column(&["username", "login"]).ok_or("В CSV нет столбца username")?;
    let secret = column(&["password"]).ok_or("В CSV нет столбца password")?;
    let mut logins = Vec::new();
    let mut skipped = 0;
    let mut record = csv::StringRecord::new();
    while reader
        .read_record(&mut record)
        .map_err(|_| "Некорректная строка CSV")?
    {
        let (Some(origin), Some(username), Some(password)) = (
            record.get(address).and_then(stored_origin),
            record.get(user),
            record.get(secret),
        ) else {
            skipped += 1;
            continue;
        };
        if password.is_empty() || password.len() > 8192 || username.len() > 4096 {
            skipped += 1;
            continue;
        }
        logins.push(Login {
            id: String::new(),
            origin,
            username: username.into(),
            password: password.into(),
        });
    }
    Ok((logins, skipped))
}

pub fn import(profile: &Path, input: Zeroizing<String>) -> Result<PasswordImport, String> {
    let (incoming, skipped) = parse(&input)?;
    let _gate = VAULT_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut entries = load(profile)?;
    let mut seen: HashSet<_> = entries.iter().map(Login::key).collect();
    let selected: Vec<_> = incoming
        .iter()
        .map(|item| seen.insert(item.key()))
        .collect();
    drop(seen);
    let mut report = PasswordImport {
        imported: 0,
        duplicates: 0,
        skipped,
    };
    for (mut item, is_new) in incoming.into_iter().zip(selected) {
        if is_new {
            item.id = new_id()?;
            entries.push(item);
            report.imported += 1;
        } else {
            report.duplicates += 1;
        }
    }
    if report.imported != 0 {
        save(profile, &mut entries)?;
    }
    Ok(report)
}

fn save(profile: &Path, entries: &mut [Login]) -> Result<(), String> {
    entries.sort_by(|a, b| a.origin.cmp(&b.origin).then(a.username.cmp(&b.username)));
    let plaintext = Zeroizing::new(
        serde_json::to_vec(entries).map_err(|_| "Не удалось подготовить хранилище")?,
    );
    let protected = protect(&plaintext, true)?;
    crate::core::store::write_file(&profile.join(VAULT_FILE), &protected)
        .map_err(|_| "Не удалось сохранить хранилище Aurora".into())
}

pub fn manage(
    profile: &Path,
    action: &str,
    site: &str,
    username: String,
    password: Option<Zeroizing<String>>,
    id: Option<String>,
) -> Result<Vec<VaultRecord>, String> {
    let _gate = VAULT_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut entries = load(profile)?;
    if action != "list" {
        let site = stored_origin(site).ok_or("Укажите HTTP- или HTTPS-адрес сайта")?;
        let position = id
            .as_deref()
            .map(|id| {
                entries
                    .iter()
                    .position(|entry| {
                        entry.id == id && entry.origin == site && entry.username == username
                    })
                    .ok_or("Сохранённый вход больше недоступен")
            })
            .transpose()?;
        match action {
            "save" => {
                let password = password
                    .filter(|value| !value.is_empty() && value.len() <= 8192)
                    .ok_or("Введите новый пароль")?;
                if username.len() > 4096 {
                    return Err("Имя пользователя слишком длинное".into());
                }
                let duplicate = entries.iter().enumerate().any(|(index, entry)| {
                    Some(index) != position && entry.key() == (&site, &username, &password)
                });
                if duplicate && position.is_some() {
                    return Err("Этот вариант пароля уже есть в хранилище".into());
                }
                if let Some(position) = position {
                    entries[position].password.zeroize();
                    entries[position].password.push_str(&password);
                } else if !duplicate {
                    entries.push(Login {
                        id: new_id()?,
                        origin: site,
                        username,
                        password: password.to_string(),
                    });
                }
            }
            "remove" => {
                entries.remove(position.ok_or("Сохранённый вход не найден")?);
            }
            _ => return Err("Неизвестное действие с хранилищем".into()),
        }
        save(profile, &mut entries)?;
    }
    Ok(records(&entries))
}

pub fn accounts(profile: &Path, origin: &str) -> Result<Vec<VaultRecord>, String> {
    let mut entries = load(profile)?;
    entries.retain(|item| item.origin == origin);
    Ok(records(&entries))
}

pub fn import_file(path: &Path) -> Result<PasswordImport, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| "Не удалось открыть выбранный CSV")?;
    let mut input = Zeroizing::new(String::new());
    file.take(CSV_LIMIT as u64 + 1)
        .read_to_string(&mut input)
        .map_err(|_| "Не удалось прочитать CSV как UTF-8")?;
    let state = crate::core::store::AppState::init(super::profile_path()?)
        .map_err(|_| "Профиль Aurora занят или недоступен. Закройте Aurora перед импортом.")?;
    import(&state.data_dir, input)
}

pub async fn fill(
    view: Webview,
    profile: &Path,
    site: &str,
    id: &str,
    username: &str,
) -> Result<(), String> {
    use webview2_com::ExecuteScriptCompletedHandler;
    let entries = load(profile)?;
    let login = entries
        .iter()
        .find(|item| item.id == id && item.origin == site && item.username == username)
        .ok_or("Пароль для текущего сайта не найден. Откройте список заново.")?;
    let encoded = Zeroizing::new(
        serde_json::to_string(&(&login.origin, &login.username, &login.password))
            .map_err(|_| "Не удалось подготовить заполнение")?,
    );
    let script = Zeroizing::new(format!(
        r#"(([origin, username, secret]) => {{
        if (window.location.origin !== origin) return false;
        const visible = input => input.getClientRects().length && !input.disabled && !input.readOnly;
        const fields = [...document.querySelectorAll('input[type="password"]')].filter(input =>
            visible(input) && input.autocomplete !== 'new-password');
        if (fields.length !== 1) return false;
        const password = fields[0];
        if (password.form && new URL(password.form.action, location.href).origin !== origin) return false;
        const selector = password.form ? 'input[autocomplete="username"], input[type="email"], input[type="text"], input:not([type])' : 'input[autocomplete="username"], input[type="email"]';
        const users = [...(password.form ?? document).querySelectorAll(selector)].filter(visible);
        const user = users.find(input => input.autocomplete === 'username') ?? (users.length === 1 ? users[0] : null);
        const set = (input, value) => {{
            Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, value);
            input.dispatchEvent(new Event('input', {{ bubbles: true }}));
        }};
        if (user && username) set(user, username);
        set(password, secret);
        return true;
    }})({encoded})"#,
        encoded = encoded.as_str()
    ));
    let (send, mut reply) = tauri::async_runtime::channel(1);
    view.with_webview(move |platform| unsafe {
        if let Ok(native) = platform.controller().CoreWebView2() {
            let callback = ExecuteScriptCompletedHandler::create(Box::new(move |result, value| {
                let _ = send.try_send(result.is_ok() && value == "true");
                Ok(())
            }));
            let _ = native.ExecuteScript(&HSTRING::from(script.as_str()), &callback);
        }
    })
    .map_err(|_| "Вкладка закрыта")?;
    if reply.recv().await == Some(true) {
        Ok(())
    } else {
        Err("На странице не найдено однозначное поле входа для этого сайта".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_preserves_quoted_fields_and_encrypts_locally() {
        let (entries, skipped) = parse("\u{feff}name,url,username,password,note\r\nExample,https://example.org/login,qa,\"synthetic,example\",\r\nShort,https://example.net/,missing\r\nOther,http://example.net/,,synthetic-http\r\n").unwrap();
        assert_eq!(skipped, 1);
        assert_eq!(entries[0].origin, "https://example.org");
        assert_eq!(entries[0].password, "synthetic,example");
        assert_eq!(entries[1].origin, "http://example.net");
        assert!(entries[1].username.is_empty());
        assert!(origin(&entries[1].origin).is_none());
        let encrypted = protect(b"synthetic test data", true).unwrap();
        assert_ne!(encrypted.as_slice(), b"synthetic test data");
        assert_eq!(
            protect(&encrypted, false).unwrap().as_slice(),
            b"synthetic test data"
        );
    }

    #[test]
    fn import_keeps_conflicts_and_reuses_legacy_records() {
        let dir = std::env::temp_dir().join(format!("aurora-vault-{}", new_id().unwrap()));
        std::fs::create_dir(&dir).unwrap();
        let legacy =
            br#"[{"origin":"https://example.org","username":"qa","password":"synthetic-a"}]"#;
        crate::core::store::write_file(&dir.join(VAULT_FILE), &protect(legacy, true).unwrap())
            .unwrap();
        let file = || {
            Zeroizing::new(
            "url,username,password,httpRealm,formActionOrigin,guid\nhttps://example.org/a,qa,synthetic-a,,,\nhttps://example.org/b,qa,synthetic-b,,,\nhttp://example.net/,qa,synthetic-http,,,\nandroid://test,qa,incompatible,,,\n".into()
        )
        };
        let report = import(&dir, file()).unwrap();
        assert_eq!(
            (report.imported, report.duplicates, report.skipped),
            (2, 1, 1)
        );
        let repeat = import(&dir, file()).unwrap();
        assert_eq!(
            (repeat.imported, repeat.duplicates, repeat.skipped),
            (0, 3, 1)
        );

        let site = "https://example.org";
        let found = accounts(&dir, site).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found[0].id.is_empty());
        assert_ne!(found[0].id, found[1].id);
        assert_eq!(found[1].variant, Some(2));
        manage(
            &dir,
            "remove",
            site,
            "qa".into(),
            None,
            Some(found[0].id.clone()),
        )
        .unwrap();
        manage(
            &dir,
            "save",
            site,
            "qa".into(),
            Some(Zeroizing::new("synthetic-c".into())),
            Some(found[1].id.clone()),
        )
        .unwrap();
        let entries = load(&dir).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].password, "synthetic-c");
        assert_eq!(entries[1].id, found[1].id);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
