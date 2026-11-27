mod install;

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use reqwest::blocking::{Client, Response};
use reqwest::{redirect::Policy, StatusCode};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use url::Url;

use crate::{browser, core::model::APP_VERSION, core::store::AppState, profiles::ActiveProfile};

pub use install::{apply_worker, runtime_guard};

const RELEASES: &str = "https://github.com/cyberangeldrip-source/aurora-browser/releases";
const ARTIFACT: &str = "aurora-browser.exe";
const MANIFEST: &str = "aurora-update.json";
const SIGNATURE: &str = "aurora-update.json.sig";
const PLATFORM: &str = "windows-x86_64";
const PUBLIC_KEY: &str = include_str!("public.key");
const MAX_MANIFEST: usize = 16 * 1024;
const MAX_SIGNATURE: usize = 4 * 1024;
const MAX_PAYLOAD: u64 = 96 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct Manifest {
    version: String,
    platform: String,
    notes: String,
    bytes: u64,
    sha256: String,
}

#[derive(Clone)]
struct Release {
    manifest: Manifest,
    bytes: Vec<u8>,
    signature: Vec<u8>,
}

#[derive(Default)]
pub struct Updater {
    available: Mutex<Option<Release>>,
    installing: AtomicBool,
    result_read: AtomicBool,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn stable_version(value: &str) -> Option<Version> {
    let version = Version::parse(value).ok()?;
    (version.pre.is_empty() && version.build.is_empty() && version.to_string() == value)
        .then_some(version)
}

fn verify_release(bytes: Vec<u8>, signature: Vec<u8>) -> io::Result<Release> {
    let decode = |data: &[u8]| -> io::Result<String> {
        let raw = std::str::from_utf8(data).map_err(|_| invalid("Неверный формат подписи"))?;
        let decoded = STANDARD
            .decode(raw.trim())
            .map_err(|_| invalid("Неверный формат подписи"))?;
        String::from_utf8(decoded).map_err(|_| invalid("Неверный формат подписи"))
    };
    let key = PublicKey::decode(&decode(PUBLIC_KEY.as_bytes())?)
        .map_err(|_| invalid("Неверный открытый ключ Aurora"))?;
    let signature_box =
        Signature::decode(&decode(&signature)?).map_err(|_| invalid("Неверный формат подписи"));
    key.verify(&bytes, &signature_box?, false)
        .map_err(|_| invalid("Подпись обновления Aurora не прошла проверку"))?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if stable_version(&manifest.version).is_none()
        || manifest.platform != PLATFORM
        || !(1..=MAX_PAYLOAD).contains(&manifest.bytes)
        || manifest.sha256.len() != 64
        || !manifest.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || manifest.notes.chars().count() > 3500
    {
        return Err(invalid(
            "Параметры подписанного релиза не подходят для этой сборки",
        ));
    }
    Ok(Release {
        manifest,
        bytes,
        signature,
    })
}

fn newer(manifest: &Manifest) -> bool {
    matches!(
        (stable_version(&manifest.version), stable_version(APP_VERSION)),
        (Some(next), Some(current)) if next > current
    )
}

fn allowed_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
                    | "github-releases.githubusercontent.com"
            )
        )
}

fn client() -> io::Result<Client> {
    Client::builder()
        .https_only(true)
        .user_agent(concat!("Aurora-Updater/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(12))
        .timeout(Duration::from_secs(180))
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() < 8 && allowed_url(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("Недопустимый переход при загрузке обновления")
            }
        }))
        .build()
        .map_err(io::Error::other)
}

fn request(client: &Client, url: &str) -> io::Result<Response> {
    client.get(url).send().map_err(io::Error::other)
}

fn read_small(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut data)?;
    if data.len() > limit {
        return Err(invalid("Описание обновления слишком большое"));
    }
    Ok(data)
}

fn find_release() -> io::Result<Option<Release>> {
    let client = client()?;
    let base = format!("{RELEASES}/latest/download");
    let response = request(&client, &format!("{base}/{MANIFEST}"))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let document = read_small(
        response.error_for_status().map_err(io::Error::other)?,
        MAX_MANIFEST,
    )?;
    let signature = request(&client, &format!("{base}/{SIGNATURE}"))?;
    let signature = read_small(
        signature.error_for_status().map_err(io::Error::other)?,
        MAX_SIGNATURE,
    )?;
    let release = verify_release(document, signature)?;
    Ok(newer(&release.manifest).then_some(release))
}

fn verify_payload(
    manifest: &Manifest,
    mut input: impl Read,
    mut output: impl Write,
    mut progress: impl FnMut(u64),
) -> io::Result<()> {
    let mut hash = Sha256::new();
    let mut received = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        received += read as u64;
        if received > manifest.bytes {
            return Err(invalid("Размер файла обновления не совпадает с релизом"));
        }
        hash.update(&buffer[..read]);
        output.write_all(&buffer[..read])?;
        progress(received);
    }
    if received != manifest.bytes
        || !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&manifest.sha256)
    {
        return Err(invalid(
            "Содержимое файла обновления не совпадает с подписанным релизом",
        ));
    }
    Ok(())
}

fn download(release: &Release, output: impl Write, progress: impl FnMut(u64)) -> io::Result<()> {
    let version = &release.manifest.version;
    let client = client()?;
    let response = request(
        &client,
        &format!("{RELEASES}/download/v{version}/{ARTIFACT}"),
    )?
    .error_for_status()
    .map_err(io::Error::other)?;
    verify_payload(&release.manifest, response, output, progress)
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<Manifest>, String> {
    let release = tauri::async_runtime::spawn_blocking(find_release)
        .await
        .map_err(|_| "Не удалось завершить проверку обновлений")?
        .map_err(|error| format!("Не удалось проверить обновления. {error}"))?;
    let info = release.as_ref().map(|item| item.manifest.clone());
    *app.state::<Updater>()
        .available
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = release;
    Ok(info)
}

#[tauri::command]
pub async fn update_install(app: AppHandle, version: String) -> Result<(), String> {
    let updater = app.state::<Updater>();
    let release = updater
        .available
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .filter(|release| release.manifest.version == version)
        .cloned()
        .ok_or("Проверьте наличие обновления ещё раз")?;
    if updater.installing.swap(true, Ordering::AcqRel) {
        return Err("Обновление уже выполняется".into());
    }
    let profile = app.state::<ActiveProfile>().id.clone();
    let handle = app.clone();
    let stage = tauri::async_runtime::spawn_blocking(move || {
        let bytes = release.manifest.bytes;
        let mut previous = 0;
        install::prepare(release, profile, |received| {
            let percent = (received * 100 / bytes) as u8;
            if percent >= previous + 2 || (percent == 100 && previous < 100) {
                previous = percent;
                browser::emit(&handle, "au://update-progress", percent);
            }
        })
    })
    .await;
    let result = stage
        .map_err(|_| "Загрузка обновления прервалась".to_string())
        .and_then(|prepared| {
            prepared.map_err(|error| format!("Не удалось подготовить обновление. {error}"))
        });
    if let Err(error) = result {
        updater.installing.store(false, Ordering::Release);
        return Err(error);
    }
    let handle = app.clone();
    browser::emit(&app, "au://update-restarting", ());
    app.run_on_main_thread(move || {
        handle.state::<AppState>().save_before_exit();
        handle.exit(0);
    })
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_last_result(app: AppHandle) -> Option<install::Report> {
    if app
        .state::<Updater>()
        .result_read
        .swap(true, Ordering::AcqRel)
    {
        return None;
    }
    install::read_result()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_stable_versions_and_trusted_download_addresses() {
        assert!(stable_version("1.10.0").unwrap() > stable_version("1.9.0").unwrap());
        for invalid in ["v1.9.0", "1.9.0-rc.1", "1.9.0+local", "1.09.0"] {
            assert!(stable_version(invalid).is_none());
        }
        for invalid in [
            "http://github.com/",
            "https://github.com.example.org/",
            "https://account@github.com/",
            "https://github.com:9443/",
        ] {
            assert!(!allowed_url(&Url::parse(invalid).unwrap()));
        }
        assert!(allowed_url(
            &Url::parse("https://release-assets.githubusercontent.com/1").unwrap()
        ));
    }

    #[test]
    fn rejects_wrong_bytes_size_and_signature() {
        let data = b"aurora";
        let manifest = Manifest {
            version: "1.9.1".into(),
            platform: PLATFORM.into(),
            notes: "Test".into(),
            bytes: data.len() as u64,
            sha256: format!("{:x}", Sha256::digest(data)),
        };
        let mut saved = Vec::new();
        verify_payload(&manifest, &data[..], &mut saved, |_| {}).unwrap();
        assert_eq!(saved, data);
        for invalid in [
            b"auroro".as_slice(),
            b"auroraa".as_slice(),
            b"auror".as_slice(),
        ] {
            assert!(verify_payload(&manifest, invalid, io::sink(), |_| {}).is_err());
        }
        assert!(
            verify_release(serde_json::to_vec(&manifest).unwrap(), b"unsigned".to_vec()).is_err()
        );
    }
}
