use crate::common::App;
use crate::AppTrait;
use anyhow::Result;
use glob;
use lnk::ShellLink;
use parselnk::Lnk;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;
use windows_icons::get_icon_by_path;
use winreg::enums::*;
use winreg::RegKey;

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
        localized_app_names: BTreeMap::new(),
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
        localized_app_names: BTreeMap::new(),
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
            if let std::result::Result::Ok(value) = std::env::var(env_name) {
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
        localized_app_names: BTreeMap::new(),
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

pub fn get_apps_from_registry() -> Result<Vec<App>> {
    let mut apps = Vec::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // Read from both HKLM and HKCU App Paths
    for root in &[hklm, hkcu] {
        if let Ok(app_paths_key) =
            root.open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths")
        {
            for subkey_name in app_paths_key.enum_keys().flatten() {
                let subkey_name: String = subkey_name;
                if let Ok(subkey) = app_paths_key.open_subkey(&subkey_name) {
                    if let Ok(path) = subkey.get_value::<String, _>("") {
                        let clean_path = path.trim_matches('"').to_string();
                        let path_buf = PathBuf::from(&clean_path);
                        if path_buf.exists() {
                            let name = path_buf
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or(&subkey_name)
                                .to_string();

                            // Try to get icon from registry, otherwise use executable
                            let icon_path = subkey
                                .get_value::<String, _>("DefaultIcon")
                                .ok()
                                .or_else(|| subkey.get_value::<String, _>("Icon").ok())
                                .map(|icon| {
                                    let clean_icon = icon.trim_matches('"').to_string();
                                    translate_path_alias(PathBuf::from(clean_icon))
                                })
                                .or_else(|| extract_icon_path(&path_buf));

                            apps.push(App {
                                name,
                                localized_app_names: BTreeMap::new(),
                                icon_path,
                                app_path_exe: Some(path_buf.clone()),
                                app_desktop_path: path_buf.parent().unwrap().to_path_buf(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(apps)
}

/// Helper function to extract icon path from various sources
pub fn extract_icon_path(app_path: &Path) -> Option<PathBuf> {
    if !app_path.exists() {
        return None;
    }

    // Priority 1: If it's a .exe, use it directly for icon extraction
    if app_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("exe"))
        .unwrap_or(false)
    {
        return Some(app_path.to_path_buf());
    }

    // Priority 2: Look for .ico files in the same directory
    let parent_dir = app_path.parent()?;
    let stem = app_path.file_stem()?.to_str()?;

    // Common icon file names
    let icon_names = [
        format!("{}.ico", stem),
        "app.ico".to_string(),
        "icon.ico".to_string(),
        "application.ico".to_string(),
    ];

    for icon_name in icon_names {
        let icon_path = parent_dir.join(icon_name);
        if icon_path.exists() {
            return Some(icon_path);
        }
    }

    // Priority 3: Use the executable itself
    Some(app_path.to_path_buf())
}

/// Helper function to find UWP app icons in the package directory
pub fn find_uwp_icon(install_path: &Path) -> Option<PathBuf> {
    if !install_path.exists() {
        return None;
    }

    // Common UWP icon locations and patterns
    let icon_patterns = [
        "Assets\\*.png",
        "Assets\\*.ico",
        "*.png",
        "Images\\*.png",
        "Assets\\Square44x44Logo.png",
        "Assets\\Square150x150Logo.png",
        "Assets\\Logo.png",
    ];

    for pattern in icon_patterns {
        let search_path = install_path.join(pattern);
        if let Ok(matches) = glob::glob(search_path.to_string_lossy().as_ref()) {
            for entry in matches.flatten() {
                if entry.is_file() {
                    return Some(entry);
                }
            }
        }
    }

    None
}

pub fn get_apps_from_path_env() -> Result<Vec<App>> {
    let mut apps = Vec::new();

    if let Ok(path_var) = std::env::var("PATH") {
        for path_str in path_var.split(';') {
            let path_str: String = path_str.to_string();
            let path = PathBuf::from(&path_str);
            if !path.exists() {
                continue;
            }

            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    let file_path = entry.path();
                    if file_path.is_file() {
                        if let Some(ext) = file_path.extension() {
                            if ext.eq_ignore_ascii_case("exe") {
                                let name = file_path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Unknown")
                                    .to_string();

                                // Use the executable itself as icon source
                                let icon_path = Some(file_path.clone());

                                apps.push(App {
                                    name,
                                    localized_app_names: BTreeMap::new(),
                                    icon_path,
                                    app_path_exe: Some(file_path.clone()),
                                    app_desktop_path: path.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(apps)
}

pub fn get_uwp_apps_powershell() -> Result<Vec<App>> {
    let mut apps = Vec::new();

    let script = r#"
        $apps = Get-AppxPackage | ForEach-Object {
            $package = $_
            $manifestPath = Join-Path $package.InstallLocation "AppxManifest.xml"
            if (Test-Path $manifestPath) {
                [xml]$manifest = Get-Content $manifestPath
                $applications = $manifest.Package.Applications.Application
                if ($applications) {
                    foreach ($app in $applications) {
                        $displayName = if ($app.VisualElements.DisplayName) { $app.VisualElements.DisplayName } else { $package.Name }
                        $appId = $app.Id
                        $fullAppId = "$($package.PackageFamilyName)!$appId"
                        
                        [PSCustomObject]@{
                            Name = $displayName
                            AppUserModelId = $fullAppId
                            InstallLocation = $package.InstallLocation
                            Description = $package.Description
                        }
                    }
                }
            }
        }
        $apps | ConvertTo-Json -Depth 3
    "#;

    let output = Command::new("powershell")
        .arg("-Command")
        .arg(script)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let json_output = String::from_utf8_lossy(&output.stdout);
            if let Ok(uwp_data) = serde_json::from_str::<Vec<serde_json::Value>>(&json_output) {
                for app_data in uwp_data {
                    if let (Some(name), Some(app_id)) = (
                        app_data["Name"].as_str(),
                        app_data["AppUserModelId"].as_str(),
                    ) {
                        let install_location = app_data["InstallLocation"].as_str().unwrap_or("");

                        // Try to find UWP app icon
                        let install_path = PathBuf::from(install_location);
                        let icon_path = find_uwp_icon(&install_path);

                        let app = App {
                            name: name.to_string(),
                            localized_app_names: BTreeMap::new(),
                            icon_path,
                            app_path_exe: None, // UWP apps use shell:AppsFolder\AppId
                            app_desktop_path: install_path,
                        };

                        apps.push(app);
                    }
                }
            }
        }
        _ => {
            // PowerShell failed or UWP apps not available, return empty vec
        }
    }

    Ok(apps)
}

pub fn get_all_apps(search_paths: &[PathBuf]) -> Result<Vec<App>> {
    let mut all_apps = Vec::new();
    let mut seen_paths = HashSet::new();

    // Create a HashSet of search paths starting with the default Windows paths
    let mut path_set: HashSet<PathBuf> = HashSet::new();

    // Add default Windows paths
    for path in get_default_search_paths() {
        path_set.insert(path);
    }

    // Add extra search paths
    for path in search_paths.iter() {
        path_set.insert(path.clone());
    }

    // 1. Discover from Start Menu shortcuts (.lnk files)
    for search_path in &path_set {
        if !search_path.exists() {
            continue;
        }

        for entry in WalkDir::new(search_path)
            .max_depth(3)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                if let Some(extension) = path.extension() {
                    if extension == "lnk" {
                        if let Ok(app) = App::from_path(&path) {
                            if let Some(app_path) = &app.app_path_exe {
                                if seen_paths.insert(app_path.clone()) {
                                    all_apps.push(app);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Discover from registry App Paths
    if let Ok(registry_apps) = get_apps_from_registry() {
        for app in registry_apps {
            if let Some(app_path) = &app.app_path_exe {
                if seen_paths.insert(app_path.clone()) {
                    all_apps.push(app);
                }
            }
        }
    }

    // 3. Discover from PATH environment variable
    if let Ok(path_apps) = get_apps_from_path_env() {
        for app in path_apps {
            if let Some(app_path) = &app.app_path_exe {
                if seen_paths.insert(app_path.clone()) {
                    all_apps.push(app);
                }
            }
        }
    }

    // 4. Discover UWP/Windows Store apps using PowerShell
    if let Ok(uwp_apps) = get_uwp_apps_powershell() {
        for app in uwp_apps {
            if let Some(app_path) = &app.app_path_exe {
                if seen_paths.insert(app_path.clone()) {
                    all_apps.push(app);
                }
            }
        }
    }

    // Sort apps by name
    all_apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(all_apps)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_all_apps() {
        let search_paths = vec![PathBuf::from(
            "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs",
        )];
        let apps = get_all_apps(&search_paths).unwrap();
        println!(
            "DBG: {:?}",
            apps.into_iter()
                .filter(|app| app.name.contains("Chrome"))
                .collect::<Vec<_>>()
        );
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
}
