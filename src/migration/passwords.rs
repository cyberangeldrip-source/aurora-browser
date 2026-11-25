use std::collections::BTreeMap;
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
    origin: String,
    username: String,
    password: String,
}

#[derive(Serialize)]
pub struct PasswordImport {
    pub imported: usize,
    pub skipped: usize,
}

#[derive(Serialize)]
pub struct Account {
    pub index: usize,
    pub username: String,
}

pub fn origin(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    (crate::core::urls::is_web(&url) && url.scheme() == "https")
        .then(|| url.origin().ascii_serialization())
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
    let mut reader = csv::Reader::from_reader(input.trim_start_matches('\u{feff}').as_bytes());
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
        let site = record.get(address).and_then(origin);
        let username = record.get(user).unwrap_or_default();
        let password = record.get(secret).unwrap_or_default();
        if let Some(origin) = site
            .filter(|_| !password.is_empty() && password.len() <= 8192 && username.len() <= 4096)
        {
            logins.push(Login {
                origin,
                username: username.into(),
                password: password.into(),
            });
        } else {
            skipped += 1;
        }
    }
    if logins.is_empty() {
        return Err("В CSV нет паролей для поддерживаемых HTTPS-сайтов".into());
    }
    Ok((logins, skipped))
}

pub fn import(profile: &Path, input: Zeroizing<String>) -> Result<PasswordImport, String> {
    let (incoming, skipped) = parse(&input)?;
    let _gate = VAULT_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut merged: BTreeMap<_, _> = load(profile)?
        .into_iter()
        .map(|item| ((item.origin.clone(), item.username.clone()), item))
        .collect();
    let mut imported = 0;
    for item in incoming {
        let key = (item.origin.clone(), item.username.clone());
        if merged
            .get(&key)
            .is_some_and(|old| old.password == item.password)
        {
            continue;
        }
        merged.insert(key, item);
        imported += 1;
    }
    if imported != 0 {
        let entries: Vec<_> = merged.into_values().collect();
        let plaintext = Zeroizing::new(
            serde_json::to_vec(&entries).map_err(|_| "Не удалось подготовить хранилище")?,
        );
        let protected = protect(&plaintext, true)?;
        crate::core::store::write_file(&profile.join(VAULT_FILE), &protected)
            .map_err(|_| "Не удалось сохранить хранилище Aurora")?;
    }
    Ok(PasswordImport { imported, skipped })
}

pub fn accounts(profile: &Path, origin: &str) -> Result<Vec<Account>, String> {
    Ok(load(profile)?
        .into_iter()
        .enumerate()
        .filter(|(_, item)| item.origin == origin)
        .map(|(index, item)| Account {
            index,
            username: item.username.clone(),
        })
        .collect())
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
    index: usize,
    username: &str,
) -> Result<(), String> {
    use webview2_com::ExecuteScriptCompletedHandler;
    let entries = load(profile)?;
    let login = entries
        .get(index)
        .filter(|item| item.origin == site && item.username == username)
        .ok_or("Пароль для текущего сайта не найден. Откройте список заново.")?;
    let encoded = Zeroizing::new(
        serde_json::to_string(login).map_err(|_| "Не удалось подготовить заполнение")?,
    );
    let script = Zeroizing::new(format!(
        r#"((login) => {{
        if (window.location.origin !== login.origin) return false;
        const visible = input => input.getClientRects().length && !input.disabled && !input.readOnly;
        const fields = [...document.querySelectorAll('input[type="password"]')].filter(input =>
            visible(input) && input.autocomplete !== 'new-password');
        if (fields.length !== 1) return false;
        const password = fields[0];
        if (!password.form || new URL(password.form.action, location.href).origin !== login.origin) return false;
        const users = [...password.form.querySelectorAll('input[autocomplete="username"], input[type="email"], input[type="text"], input:not([type])')].filter(visible);
        const user = users.find(input => input.autocomplete === 'username') ?? (users.length === 1 ? users[0] : null);
        const set = (input, value) => {{
            Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, value);
            input.dispatchEvent(new Event('input', {{ bubbles: true }}));
        }};
        if (user && login.username) set(user, login.username);
        set(password, login.password);
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
    #[test]
    fn csv_preserves_quoted_fields_and_encrypts_locally() {
        let (entries, skipped) = super::parse("\u{feff}name,url,username,password\r\nExample,https://example.org/login,qa,\"synthetic,example\"\r\nOther,http://example.net/,sample,skip\r\n").unwrap();
        assert_eq!(skipped, 1);
        assert_eq!(entries[0].origin, "https://example.org");
        assert_eq!(entries[0].password, "synthetic,example");
        let encrypted = super::protect(b"synthetic test data", true).unwrap();
        assert_ne!(encrypted.as_slice(), b"synthetic test data");
        assert_eq!(
            super::protect(&encrypted, false).unwrap().as_slice(),
            b"synthetic test data"
        );
    }
}
