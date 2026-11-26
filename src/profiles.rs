use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
}

pub struct ActiveProfile {
    pub root: PathBuf,
    pub id: String,
}

impl ActiveProfile {
    pub fn from_args(root: PathBuf) -> std::io::Result<Self> {
        let mut args = std::env::args().skip(1);
        let id = if args.next().as_deref() == Some("--profile") {
            args.next().unwrap_or_default()
        } else {
            String::new()
        };
        if !id.is_empty() && read(&root, &id).is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Профиль Aurora не найден",
            ));
        }
        Ok(Self { root, id })
    }

    pub fn path(&self) -> PathBuf {
        if self.id.is_empty() {
            self.root.clone()
        } else {
            self.root.join("profiles").join(&self.id)
        }
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn read(root: &Path, id: &str) -> Option<Profile> {
    if !valid_id(id) {
        return None;
    }
    let profiles = root.join("profiles").canonicalize().ok()?;
    let directory = profiles.join(id);
    if directory.canonicalize().ok()? != directory {
        return None;
    }
    let file = directory.join("profile.json");
    if std::fs::metadata(&file).ok()?.len() > 2048 {
        return None;
    }
    let profile: Profile = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    (profile.id == id).then_some(profile)
}

pub fn list(root: &Path) -> Vec<Profile> {
    let mut profiles: Vec<_> = std::fs::read_dir(root.join("profiles"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| read(root, entry.file_name().to_str()?))
        .collect();
    profiles.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    profiles.insert(
        0,
        Profile {
            id: String::new(),
            name: "Основной профиль".into(),
        },
    );
    profiles
}

pub fn create(root: &Path, input: &str) -> Result<Profile, String> {
    let name = input.trim();
    if name.is_empty() || name.chars().count() > 45 || name.chars().any(char::is_control) {
        return Err("Введите имя профиля до 45 символов".into());
    }
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| "Не удалось создать идентификатор профиля")?;
    let id = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let profiles = root.join("profiles");
    std::fs::create_dir_all(&profiles).map_err(|_| "Не удалось создать каталог профилей")?;
    let directory = profiles.join(&id);
    std::fs::create_dir(&directory).map_err(|_| "Не удалось создать профиль")?;
    let profile = Profile {
        id,
        name: name.to_owned(),
    };
    let json = serde_json::to_vec(&profile).map_err(|_| "Не удалось записать профиль")?;
    crate::core::store::write_file(&directory.join("profile.json"), &json)
        .map_err(|_| "Не удалось сохранить профиль")?;
    Ok(profile)
}

pub fn launch(active: &ActiveProfile, id: &str) -> Result<(), String> {
    if id == active.id {
        return Err("Этот профиль уже открыт".into());
    }
    if !id.is_empty() && read(&active.root, id).is_none() {
        return Err("Профиль Aurora не найден".into());
    }
    let path = if id.is_empty() {
        active.root.clone()
    } else {
        active.root.join("profiles").join(id)
    };
    if path.exists() {
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.join("profile.lock"))
            .map_err(|_| "Каталог профиля недоступен")?;
        lock.try_lock()
            .map_err(|_| "Этот профиль уже открыт в другом окне")?;
    }
    let program = std::env::current_exe().map_err(|_| "Не удалось найти Aurora")?;
    std::process::Command::new(program)
        .args(["--profile", id])
        .spawn()
        .map_err(|_| "Не удалось открыть профиль")?;
    Ok(())
}
