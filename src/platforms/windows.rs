use crate::common::App;
use crate::AppTrait;
use anyhow::Result;
use lnk::ShellLink;
use parselnk::string_data;
use parselnk::Lnk;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::path::{Path, PathBuf};
use windows_icons::get_icon_by_path;
use std::process::Command;
use walkdir::WalkDir;
use std::collections::HashSet;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerShellLnkParseResult {
    #[serde(rename = "IconLocation")]
    pub icon_location: String,
    #[serde(rename = "Description")]
    pub description: String,
    #[serde(rename = "WorkingDirectory")]
    pub working_directory: String,
    #[serde(rename = "Arguments")]
    pub arguments: String,
    #[serde(rename = "Hotkey")]
    pub hotkey: String,
    #[serde(rename = "WindowStyle")]
    pub window_style: i64,
    #[serde(rename = "TargetPath")]
    pub target_path: String,
}


pub fn parse_lnk_with_powershell_1(lnk_path: PathBuf) -> anyhow::Result<PowerShellLnkParseResult> {
    let lnk_path = "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Docker Desktop.lnk";

    let script = format!(
        r#"
        function Get-Shortcut {{
            param (
                [string]$Path
            )
            
            $shell = New-Object -ComObject WScript.Shell
            $shortcut = $shell.CreateShortcut($Path)
            
            $properties = @{{
                TargetPath = $shortcut.TargetPath
                Arguments  = $shortcut.Arguments
                Description = $shortcut.Description
                Hotkey = $shortcut.Hotkey
                IconLocation = $shortcut.IconLocation
                WindowStyle = $shortcut.WindowStyle
                WorkingDirectory = $shortcut.WorkingDirectory
            }}
            
            return [PSCustomObject]$properties
        }}

        Get-Shortcut -Path "{}" | ConvertTo-Json
    "#,
        lnk_path
    );

    let output = Command::new("powershell")
        .arg("-Command")
        .arg(script)
        .output()
        .unwrap();
    let output = String::from_utf8(output.stdout).unwrap();
    // let result: PowerShellLnkParseResult = serde_json::from_str(&output).unwrap();

    let json: PowerShellLnkParseResult = serde_json::from_str(&output.to_string())?;
    Ok(json)
}

pub fn parse_lnk_with_powershell_2(lnk_path: PathBuf) -> anyhow::Result<App> {
    let parsed_json = parse_lnk_with_powershell_1(lnk_path)?;
    let target_path = PathBuf::from(parsed_json.target_path);
    let desktop_path = if parsed_json.working_directory.len() == 0 {
        PathBuf::from(parsed_json.working_directory)
    } else {
        target_path.parent().unwrap().to_path_buf()
    };
    let icon_path = if parsed_json.icon_location.len() == 0 {
        None
    } else {
        Some(PathBuf::from(parsed_json.icon_location))
    };
    let name = if parsed_json.description.len() == 0 {
        target_path
            .parent()
            .unwrap()
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    } else {
        let desc = parsed_json.description.clone();
        if desc.starts_with("Runs ") {
            // edge case for Tauri apps
            desc[5..].to_string()
        } else {
            desc
        }
    };
    let app = App {
        name: name,
        icon_path: icon_path,
        app_path_exe: Some(target_path),
        app_desktop_path: desktop_path,
    };
    Ok(app)
}

fn parse_lnk(path: PathBuf) -> Option<App> {
    let shortcut = ShellLink::open(&path).unwrap();
    let exe: Option<PathBuf> = match shortcut.link_info() {
        Some(info) => match info.local_base_path() {
            Some(path) => Some(PathBuf::from(path)),
            None => None,
        },
        None => None,
    };
    let work_dir = match shortcut.working_dir() {
        Some(dir) => PathBuf::from(dir),
        None => {
            // if exe is not None, use the exe's parent directory
            match &exe {
                Some(exe) => exe.parent().unwrap().to_path_buf(),
                None => return None,
            }
        }
    };
    let icon_path: Option<PathBuf> = shortcut.icon_location().as_ref().map(PathBuf::from);

    Some(App {
        name: path.file_stem().unwrap().to_str().unwrap().to_string(),
        icon_path,
        app_path_exe: exe,
        app_desktop_path: work_dir,
    })
}

/// Windows have path like this "%windir%\\system32\\mstsc.exe"
/// This function will translate the path to the real path
fn translate_path_alias(path: PathBuf) -> PathBuf {
    let mut path_str = path.to_string_lossy().to_string().to_lowercase();

    // Common Windows environment variables
    let env_vars = vec![
        "%windir%",
        "%systemroot%",
        "%programfiles%",
        "%programfiles(x86)%",
        "%programdata%",
        "%userprofile%",
        "%appdata%",
        "%localappdata%",
        "%public%",
        "%systemdrive%",
    ];

    for var in env_vars {
        if path_str.starts_with(var) {
            let env_name = var.trim_matches('%').to_uppercase();
            if let Ok(value) = std::env::var(env_name) {
                path_str = path_str.replace(var, &value);
                return PathBuf::from(path_str);
            }
        }
    }

    path
}

fn strip_extended_prefix(path: PathBuf) -> PathBuf {
    let path_str = path.to_string_lossy();
    if path_str.starts_with("\\\\?\\") {
        PathBuf::from(&path_str[4..])
    } else {
        path
    }
}

pub(crate) fn parse_lnk2(path: PathBuf) -> Option<App> {
    let Some(lnk) = Lnk::try_from(path.as_path()).ok() else {
        return None;
    };

    let icon = lnk.string_data.icon_location.clone().map(|icon| {
        if icon.to_string_lossy().starts_with("%") {
            translate_path_alias(PathBuf::from(icon))
        } else {
            icon
        }
    });
    let mut app_exe_path: Option<PathBuf> = match lnk.link_info.local_base_path {
        Some(path) => Some(PathBuf::from(path)),
        None => lnk.string_data.relative_path.clone(),
    };
    if app_exe_path.is_none() {
        app_exe_path = lnk.string_data.relative_path.clone();
    }

    if app_exe_path.is_none() {
        if let Some(icon_path) = icon.clone() {
            // Clone here before using
            let icon_path = PathBuf::from(icon_path);
            // if icon_path ends with .exe, then it is the app_exe_path

            if let Some(ext) = icon_path.extension() {
                if ext == "exe" {
                    app_exe_path = Some(translate_path_alias(icon_path));
                } else {
                    return None;
                }
            }
        }
    }
    let Some(app_exe_path) = app_exe_path else {
        return None;
    };
    let app_exe_path = translate_path_alias(app_exe_path);
    let exe_abs_path = match app_exe_path.exists() {
        true => app_exe_path,
        false => path.parent().unwrap().join(&app_exe_path),
    };
    if !exe_abs_path.exists() {
        return None;
    }

    let exe_abs_path = std::fs::canonicalize(exe_abs_path);
    let exe_path = if exe_abs_path.is_ok() {
        strip_extended_prefix(exe_abs_path.unwrap())
    } else {
        return None;
    };

    let work_dir = lnk.string_data.working_dir;
    let work_dir = match work_dir {
        Some(dir) => {
            if dir.to_string_lossy().starts_with("%") {
                translate_path_alias(PathBuf::from(dir))
            } else {
                dir
            }
        }
        None => exe_path.parent().unwrap().to_path_buf(),
    };

    let name = path.file_stem().unwrap().to_str().unwrap().to_string();
    Some(App {
        name,
        icon_path: icon,
        app_path_exe: Some(exe_path),
        app_desktop_path: work_dir,
    })
}

pub fn open_file_with(file_path: PathBuf, app: App) {
    let mut command = Command::new(app.app_path_exe.unwrap());
    command.arg(file_path);
    command
        .spawn()
        .expect("Failed to open file with the specified application.");
}


pub fn get_default_search_paths() -> Vec<PathBuf> {
    vec![
        format!(
            "{}\\Microsoft\\Windows\\Start Menu\\Programs",
            std::env::var("APPDATA").unwrap()
        )
        .into(),
        "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs".into(),
    ]
}

pub fn get_all_apps(search_paths: &[PathBuf]) -> Result<Vec<App>> {
    // Create a HashSet of search paths starting with the default Windows paths
    let mut path_set: HashSet<&PathBuf> = HashSet::new();

    // Add extra search paths
    for path in search_paths.iter() {
        path_set.insert(path);
    }

    let mut apps = vec![];
    for search_path in path_set {
        if !search_path.exists() {
            continue;
        }

        for entry in WalkDir::new(search_path)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                if let Some(extension) = path.extension() {
                    if extension == "lnk" {
                        let result = App::from_path(&path);
                        if let Some(app) = result.ok() {
                            apps.push(app);
                        } else {
                        }
                    }
                }
            }
        }
    }
    Ok(apps)
}


impl AppTrait for App {
    fn from_path(path: &Path) -> Result<Self> {
        if let Some(extension) = path.extension() {
            if extension == "lnk" {
                if let Some(app) = parse_lnk2(path.to_path_buf()) {
                    return Ok(app);
                } 
            }
        }
        Err(anyhow::anyhow!(
            "Failed to create App from path: {:?}",
            path
        ))
    }
}

use winreg::enums::*;
use winreg::RegKey;


fn list_installed_apps() -> anyhow::Result<()> {
    fn hklm() -> RegKey {
        RegKey::predef(HKEY_LOCAL_MACHINE)
    }

    fn hkcu() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }

    // All registry paths to check
    let registry_paths = [
        // Traditional apps
        (hklm(), "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall", KEY_READ | KEY_WOW64_64KEY),
        (hklm(), "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall", KEY_READ | KEY_WOW64_32KEY),
        (hkcu(), "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall", KEY_READ),
        
        // UWP apps
        (hklm(), "SOFTWARE\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Families", KEY_READ | KEY_WOW64_64KEY),
        (hkcu(), "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Families", KEY_READ),

        // AppX system-wide packages
        (hklm(), "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Appx\\AppxAllUserStore\\Applications", KEY_READ | KEY_WOW64_64KEY),
        // AppX per-user packages
        (hkcu(), "Software\\Microsoft\\Windows\\CurrentVersion\\Appx\\PackageUserInformation", KEY_READ),
    ];

    for (hive, path, flags) in registry_paths {
        let key = match hive.open_subkey_with_flags(path, flags) {
            Ok(k) => k,
            Err(_) => continue, // Skip inaccessible keys
        };

        for subkey_name in key.enum_keys().filter_map(Result::ok) {
            let subkey = match key.open_subkey(&subkey_name) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let display_name: String = match subkey.get_value("DisplayName") {
                Ok(name) => name,
                Err(_) => continue,
            };

            // Relaxed filter
            let system_component: u32 = subkey.get_value("SystemComponent").unwrap_or(0);
            if system_component != 1 {
                println!("[Found] {}", display_name);
            }
        }
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_all_apps() {
        let search_paths = vec![PathBuf::from(
            "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs",
        )];
        let apps = get_all_apps(&search_paths).unwrap();
        assert!(!apps.is_empty());
    }

    #[test]
    fn test_path_alias() {
        let path = PathBuf::from("%windir%\\system32\\mstsc.exe");
        let path = translate_path_alias(path);
        assert_eq!(
            path.to_string_lossy().to_lowercase(),
            "c:\\windows\\system32\\mstsc.exe"
        );
    }

    #[test]
    fn test_foobar() {
        list_installed_apps(); 
    }
}
