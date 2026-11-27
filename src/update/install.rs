use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

use crate::core::{model::APP_VERSION, store::write_file};

use super::{download, invalid, newer, read_small, verify_payload, verify_release, Release};
use super::{ARTIFACT, MANIFEST, MAX_MANIFEST, MAX_SIGNATURE, SIGNATURE};

const PREFIX: &str = ".aurora-update-";
const LOCK: &str = ".aurora-runtime.lock";
const WORKER: &str = "worker.exe";
const PAYLOAD: &str = "next.exe";
const BACKUP: &str = "previous.exe";
const PLAN: &str = "plan.json";
const REPORT: &str = "result.json";
const READY: &str = "ready";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Deserialize, Serialize)]
struct Plan {
    profile: String,
}

#[derive(Deserialize, Serialize)]
pub struct Report {
    pub version: String,
    pub error: Option<String>,
}

fn parent(path: &Path) -> io::Result<&Path> {
    path.parent()
        .ok_or_else(|| invalid("Каталог установки Aurora недоступен"))
}

fn current_exe() -> io::Result<PathBuf> {
    env::current_exe()?.canonicalize()
}

fn lock_file(directory: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join(LOCK))
}

pub fn runtime_guard() -> Option<File> {
    let exe = current_exe().ok()?;
    let lock = lock_file(parent(&exe).ok()?).ok()?;
    lock.lock_shared().ok()?;
    Some(lock)
}

fn exclusive(directory: &Path) -> io::Result<File> {
    let lock = lock_file(directory)?;
    let deadline = Instant::now() + Duration::from_secs(7);
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(TryLockError::Error(error)) => return Err(error),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Закройте другие окна Aurora и повторите обновление",
                ));
            }
        }
    }
}

fn valid_stage(name: &str) -> bool {
    name.strip_prefix(PREFIX).is_some_and(|suffix| {
        suffix.len() == 16 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn valid_profile(id: &str) -> bool {
    id.is_empty() || (id.len() == 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn write_json(path: &Path, data: &impl Serialize) -> io::Result<()> {
    write_file(path, &serde_json::to_vec(data).map_err(io::Error::other)?)
}

fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    let content = read_small(File::open(path)?, 4096)?;
    serde_json::from_slice(&content).map_err(io::Error::other)
}

fn clean(stage: &Path) -> bool {
    if let Err(error) = fs::remove_file(stage.join(WORKER)) {
        if error.kind() != io::ErrorKind::NotFound {
            return false;
        }
    }
    for name in [PAYLOAD, BACKUP, MANIFEST, SIGNATURE, PLAN, REPORT, READY] {
        let _ = fs::remove_file(stage.join(name));
    }
    let _ = fs::remove_dir(stage);
    true
}

pub(super) fn prepare(
    release: Release,
    profile: String,
    progress: impl FnMut(u64),
) -> io::Result<()> {
    let exe = current_exe()?;
    if exe
        .file_name()
        .is_none_or(|name| !name.eq_ignore_ascii_case(ARTIFACT))
    {
        return Err(invalid(
            "Для обновления верните файлу имя aurora-browser.exe",
        ));
    }
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    let stage = parent(&exe)?.join(format!("{PREFIX}{:016x}", u64::from_le_bytes(random)));
    fs::create_dir(&stage)?;
    let result = (|| {
        let mut payload = File::create(stage.join(PAYLOAD))?;
        download(&release, &mut payload, progress)?;
        payload.sync_all()?;
        write_file(&stage.join(MANIFEST), &release.bytes)?;
        write_file(&stage.join(SIGNATURE), &release.signature)?;
        write_json(&stage.join(PLAN), &Plan { profile })?;
        let worker = stage.join(WORKER);
        fs::copy(exe, &worker)?;
        Command::new(worker)
            .arg("--apply-update")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        Ok(())
    })();
    if result.is_err() {
        clean(&stage);
    }
    result
}

fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = HSTRING::from(source.as_os_str());
    let destination = HSTRING::from(destination.as_os_str());
    // The staged file and destination share a volume.
    unsafe {
        MoveFileExW(
            &source,
            &destination,
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(io::Error::other)
}

fn restart(exe: &Path, stage: &Path, plan: &Plan, report: &Report) -> io::Result<Child> {
    write_json(&stage.join(REPORT), report)?;
    let name = stage
        .file_name()
        .ok_or_else(|| invalid("Недействительный каталог обновления"))?;
    Command::new(exe)
        .args(["--profile", plan.profile.as_str(), "--update-result"])
        .arg(name)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
}

fn report_failure(exe: &Path, stage: &Path, plan: &Plan, error: io::Error) -> io::Result<()> {
    let report = Report {
        version: APP_VERSION.to_owned(),
        error: Some(error.to_string()),
    };
    restart(exe, stage, plan, &report)?;
    Ok(())
}

fn replace_aurora(
    worker: &Path,
    directory: &Path,
    stage: &Path,
    exe: &Path,
) -> io::Result<Release> {
    let bytes = read_small(File::open(stage.join(MANIFEST))?, MAX_MANIFEST)?;
    let signature = read_small(File::open(stage.join(SIGNATURE))?, MAX_SIGNATURE)?;
    let release = verify_release(bytes, signature)?;
    if !newer(&release.manifest) {
        return Err(invalid("Эта версия уже установлена"));
    }
    let payload = stage.join(PAYLOAD);
    verify_payload(&release.manifest, File::open(&payload)?, io::sink(), |_| {})?;
    let _lock = exclusive(directory)?;
    if fs::read(exe)? != fs::read(worker)? {
        return Err(invalid(
            "Версия Aurora изменилась. Проверьте обновления ещё раз",
        ));
    }
    fs::copy(exe, stage.join(BACKUP))?;
    replace(&payload, exe)?;
    Ok(release)
}

pub fn apply_worker() -> io::Result<()> {
    let worker = current_exe()?;
    let stage = parent(&worker)?;
    let name = stage
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if worker.file_name().is_none_or(|value| value != WORKER) || !valid_stage(name) {
        return Err(invalid("Недействительный каталог обновления"));
    }
    let directory = parent(stage)?;
    let exe = directory.join(ARTIFACT);
    let plan: Plan = load_json(&stage.join(PLAN))?;
    if !valid_profile(&plan.profile) {
        return Err(invalid("Недействительный профиль обновления"));
    }
    let release = match replace_aurora(&worker, directory, stage, &exe) {
        Ok(release) => release,
        Err(error) => return report_failure(&exe, stage, &plan, error),
    };
    let report = Report {
        version: release.manifest.version,
        error: None,
    };
    let failure = match restart(&exe, stage, &plan, &report) {
        Ok(mut child) => {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if stage.join(READY).is_file() || Instant::now() >= deadline {
                    return Ok(());
                }
                if child.try_wait()?.is_some() {
                    break invalid("Новая версия завершилась до открытия окна. Восстановлена предыдущая версия");
                }
                thread::sleep(Duration::from_millis(125));
            }
        }
        Err(error) => error,
    };
    {
        let _lock = exclusive(directory)?;
        replace(&stage.join(BACKUP), &exe)?;
    }
    report_failure(&exe, stage, &plan, failure)
}

fn result_directory() -> Option<PathBuf> {
    let mut args = env::args_os().skip(1);
    let mut argument: OsString = args.next()?;
    if argument == "--profile" {
        args.next()?;
        argument = args.next()?;
    }
    if argument != "--update-result" {
        return None;
    }
    let name = args.next()?.into_string().ok()?;
    valid_stage(&name).then_some(())?;
    Some(parent(&current_exe().ok()?).ok()?.join(name))
}

pub(super) fn read_result() -> Option<Report> {
    let stage = result_directory()?;
    let report: Report = load_json(&stage.join(REPORT)).ok()?;
    if report.error.is_none() && report.version != APP_VERSION {
        return None;
    }
    File::create(stage.join(READY)).ok()?;
    thread::spawn(move || {
        for _ in 0..20 {
            if clean(&stage) {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
    });
    Some(report)
}
