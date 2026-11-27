use std::io;
use std::path::Path;

use serde::Serialize;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::UI::Shell::{
    AssocQueryStringW, SHChangeNotify, ShellExecuteW, ASSOCF_IS_PROTOCOL, ASSOCSTR_PROGID,
    SHCNE_ASSOCCHANGED, SHCNF_IDLIST,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
use winreg::RegKey;

use crate::core::urls::HTML_EXTENSIONS;

const APP: &str = "Aurora Browser";
const CLIENT: &str = r"Software\Clients\StartMenuInternet\Aurora.Browser";
const CAPABILITIES: &str = r"Software\Clients\StartMenuInternet\Aurora.Browser\Capabilities";
const REGISTERED: &str = r"Software\RegisteredApplications";
const AUTOSTART: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const URL_TYPE: &str = "Aurora.Browser.Url";
const HTML_TYPE: &str = "Aurora.Browser.Html";

#[derive(Serialize)]
pub struct Integration {
    autostart: bool,
    default_browser: bool,
}

fn quoted(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

fn shell_command(exe: &Path) -> String {
    format!("{} --open \"%1\"", quoted(exe))
}

fn startup_command(exe: &Path) -> String {
    format!("{} --startup", quoted(exe))
}

fn set(root: &RegKey, path: &str, name: &str, value: &str) -> io::Result<()> {
    root.create_subkey(path)?.0.set_value(name, &value)
}

fn get(root: &RegKey, path: &str, name: &str) -> Option<String> {
    root.open_subkey(path).ok()?.get_value(name).ok()
}

fn delete_value(root: &RegKey, path: &str, name: &str) -> io::Result<()> {
    let result = root
        .open_subkey_with_flags(path, KEY_SET_VALUE)
        .and_then(|key| key.delete_value(name));
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn progid_path(progid: &str) -> String {
    format!(r"Software\Classes\{progid}")
}

fn register(root: &RegKey, exe: &Path) -> io::Result<()> {
    let command = shell_command(exe);
    let icon = format!("{},0", quoted(exe));

    for (progid, name) in [
        (URL_TYPE, "Веб-ссылка Aurora"),
        (HTML_TYPE, "Документ HTML"),
    ] {
        let class = progid_path(progid);
        set(root, &class, "", name)?;
        set(root, &format!(r"{class}\DefaultIcon"), "", &icon)?;
        set(root, &format!(r"{class}\shell\open\command"), "", &command)?;
    }
    set(root, &progid_path(URL_TYPE), "URL Protocol", "")?;
    set(root, CLIENT, "", "Aurora")?;
    set(root, &format!(r"{CLIENT}\DefaultIcon"), "", &icon)?;
    set(
        root,
        &format!(r"{CLIENT}\shell\open\command"),
        "",
        &quoted(exe),
    )?;
    set(root, CAPABILITIES, "ApplicationName", APP)?;
    set(root, CAPABILITIES, "ApplicationIcon", &icon)?;
    set(
        root,
        CAPABILITIES,
        "ApplicationDescription",
        "Веб-браузер Aurora",
    )?;
    for scheme in ["http", "https"] {
        set(
            root,
            &format!(r"{CAPABILITIES}\UrlAssociations"),
            scheme,
            URL_TYPE,
        )?;
    }
    for extension in HTML_EXTENSIONS {
        set(
            root,
            &format!(r"{CAPABILITIES}\FileAssociations"),
            &format!(".{extension}"),
            HTML_TYPE,
        )?;
        set(
            root,
            &format!(r"Software\Classes\.{extension}\OpenWithProgids"),
            HTML_TYPE,
            "",
        )?;
    }
    set(root, REGISTERED, APP, CAPABILITIES)
}

fn notify_shell() {
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

pub fn register_browser() -> io::Result<()> {
    let root = RegKey::predef(HKEY_CURRENT_USER);
    register(&root, &std::env::current_exe()?)?;
    notify_shell();
    Ok(())
}

pub fn unregister_browser() -> io::Result<()> {
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let exe = std::env::current_exe()?;
    if get(&root, AUTOSTART, APP).as_deref() == Some(startup_command(&exe).as_str()) {
        delete_value(&root, AUTOSTART, APP)?;
    }
    let command = format!(r"{CLIENT}\shell\open\command");
    if get(&root, &command, "").as_deref() != Some(quoted(&exe).as_str()) {
        return Ok(());
    }
    if get(&root, REGISTERED, APP).as_deref() == Some(CAPABILITIES) {
        delete_value(&root, REGISTERED, APP)?;
    }
    for extension in HTML_EXTENSIONS {
        delete_value(
            &root,
            &format!(r"Software\Classes\.{extension}\OpenWithProgids"),
            HTML_TYPE,
        )?;
    }
    for key in [
        progid_path(URL_TYPE),
        progid_path(HTML_TYPE),
        CLIENT.to_owned(),
    ] {
        if let Err(error) = root.delete_subkey_all(&key) {
            if error.kind() != io::ErrorKind::NotFound {
                return Err(error);
            }
        }
    }
    notify_shell();
    Ok(())
}

fn is_default(scheme: &str) -> bool {
    let mut value = [0u16; 256];
    let mut len = value.len() as u32;
    let scheme = HSTRING::from(scheme);
    // The effective association comes from the shell, including the user's choice.
    let result = unsafe {
        AssocQueryStringW(
            ASSOCF_IS_PROTOCOL,
            ASSOCSTR_PROGID,
            &scheme,
            PCWSTR::null(),
            Some(PWSTR(value.as_mut_ptr())),
            &mut len,
        )
    };
    result.is_ok()
        && value
            .split(|ch| *ch == 0)
            .next()
            .is_some_and(|name| String::from_utf16_lossy(name).eq_ignore_ascii_case(URL_TYPE))
}

#[tauri::command]
pub fn system_integration() -> Integration {
    let root = RegKey::predef(HKEY_CURRENT_USER);
    Integration {
        autostart: get(&root, AUTOSTART, APP).is_some(),
        default_browser: is_default("http") && is_default("https"),
    }
}

#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let root = RegKey::predef(HKEY_CURRENT_USER);
    if enabled {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let command = startup_command(&exe);
        if command.encode_utf16().count() >= 260 {
            return Err("Путь к Aurora слишком длинный для автозапуска Windows".into());
        }
        set(&root, AUTOSTART, APP, &command)
    } else {
        delete_value(&root, AUTOSTART, APP)
    }
    .map_err(|error| format!("Не удалось изменить автозапуск. {error}"))
}

#[tauri::command]
pub fn default_browser_settings() -> Result<(), String> {
    register_browser().map_err(|error| format!("Не удалось зарегистрировать Aurora. {error}"))?;
    for uri in [
        "ms-settings:defaultapps?registeredAppUser=Aurora%20Browser",
        "ms-settings:defaultapps",
    ] {
        let uri = HSTRING::from(uri);
        // The selection is made in the standard Windows Settings page.
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR::null(),
                &uri,
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if result.0 as isize > 32 {
            return Ok(());
        }
    }
    Err("Не удалось открыть приложения по умолчанию в настройках Windows".into())
}
