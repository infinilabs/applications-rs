use crate::common::App;
use crate::utils::image::{RustImage, RustImageData};
use crate::AppTrait;
use anyhow::Result;
use freedesktop_file_parser::{parse, EntryType};
use serde_derive::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{prelude::*, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, PartialEq, Clone, Default, Eq, Hash, Serialize, Deserialize)]
pub struct AppIcon {
    name: String,
    path: PathBuf,
    dimensions: Option<u16>,
}

pub fn brute_force_find_entry(
    desktop_file_path: &Path,
    entry_names: Vec<&str>,
) -> Result<Option<String>> {
    let file = std::fs::File::open(desktop_file_path)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        match line {
            Ok(line) => {
                for entry_name in entry_names.iter() {
                    if line.starts_with(entry_name) {
                        // let entry = line.split("=").last().unwrap();
                        let entry = line[entry_name.len() + 1..line.len()].trim();
                        return Ok(Some(entry.to_string()));
                    }
                }
            }
            Err(_e) => {}
        }
    }
    Ok(None)
}

/// in case the icon in .desktop file cannot be parsed, use this function to manually find the icon
/// example /usr/share/applications/microsoft-edge.desktop icon cannot be parsed with ini crate
pub fn brute_force_find_icon(desktop_file_path: &Path) -> Result<Option<String>> {
    // read the desktop file into lines and find the icon line
    brute_force_find_entry(desktop_file_path, vec!["Icon", "icon"])
}

pub fn brute_force_find_exec(desktop_file_path: &Path) -> Result<Option<String>> {
    brute_force_find_entry(desktop_file_path, vec!["Exec", "exec"])
}

/// clean exec path by removing placeholder "%"" args
/// like %u, %U, %F
fn clean_exec_path(exec: &str) -> String {
    let cleaned: Vec<&str> = exec
        .split_whitespace()
        .take_while(|s| !s.starts_with('%')) // Take everything up to first % parameter
        .collect();

    cleaned.join(" ")
}

pub fn parse_desktop_file_content(content: &str) -> Option<(String, Option<PathBuf>)> {
    // When parsing fails, we return None rather than erroring out
    // Because not everybody obeys the rules.
    let desktop_file = parse(content).ok()?;
    let desktop_file_entry = desktop_file.entry;

    let EntryType::Application(app_fields) = desktop_file_entry.entry_type else {
        return None;
    };

    let no_display = desktop_file_entry.no_display.unwrap_or(false);

    if no_display {
        return None;
    }

    let Some(_exec) = app_fields.exec else {
        return None;
    };

    let Some(icon) = desktop_file_entry.icon else {
        return None;
    };

    let name = desktop_file_entry.name.default;

    Some((name, icon.get_icon_path()))
}

pub fn get_default_search_paths() -> Vec<PathBuf> {
    let home_dir =
        PathBuf::from(std::env::var_os("HOME").expect("environment variable $HOME not found"));

    vec![
        "/usr/share/applications".into(),
        home_dir.join(".local/share/applications"),
        // Snap
        "/var/lib/snapd/desktop/applications".into(),
        // Flatpak
        "/var/lib/flatpak/app".into(),
        home_dir.join(".local/share/flatpak/app"),
    ]
}

pub fn get_all_apps(search_paths: &[PathBuf]) -> Result<Vec<App>> {
    let search_dirs: HashSet<&PathBuf> = search_paths.iter().filter(|dir| dir.exists()).collect();

    let icons_db = find_all_app_icons()?;

    // for each dir, search for .desktop files
    let mut apps: HashSet<App> = HashSet::new();
    for dir in search_dirs {
        if !dir.exists() {
            continue;
        }
        for entry in WalkDir::new(dir.clone()) {
            if entry.is_err() {
                continue;
            }
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().is_none() {
                continue;
            }

            if path.extension().unwrap() == "desktop" && path.is_file() {
                let desktop_file_content = std::fs::read_to_string(path)?;
                let Some((app_name, opt_icon_path)) =
                    parse_desktop_file_content(&desktop_file_content)
                else {
                    continue;
                };

                let app = App {
                    name: app_name,
                    icon_path: opt_icon_path,
                    app_path_exe: None,
                    app_desktop_path: path.to_path_buf(),
                };
                apps.insert(app);
            }
        }
    }
    Ok(apps.iter().cloned().collect())
}

/// Impl based on https://wiki.archlinux.org/title/Desktop_entries
pub fn find_all_app_icons() -> Result<HashMap<String, Vec<AppIcon>>> {
    let mut search_dirs = vec!["/usr/share/icons".into(), "/usr/share/pixmaps".into()];
    // If $XDG_DATA_DIRS is either not set or empty, a value equal to `/usr/local/share/:/usr/share/` should be used.
    let xdg_data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or("/usr/local/share/:/usr/share/".into());
    for xdg_data_dir in xdg_data_dirs.split(':') {
        let dir = Path::new(xdg_data_dir).join("icons");
        search_dirs.push(dir);
    }
    // filter out search_dirs that do not exist
    let search_dirs: Vec<PathBuf> = search_dirs.into_iter().filter(|dir| dir.exists()).collect();

    let mut set = HashSet::new();

    for dir in search_dirs {
        for entry in WalkDir::new(dir.clone()) {
            if entry.is_err() {
                continue;
            }
            let entry = entry.unwrap();
            let path = entry.path();
            match path.extension() {
                Some(ext) => {
                    if ext == "png" {
                        let path_str = path.to_string_lossy().to_string();
                        let split: Vec<&str> = path_str.split("/").collect();
                        let dim_str = if split.len() < 6 {
                            None
                        } else {
                            split[5].split("x").last()
                        };
                        let dim = match dim_str {
                            Some(dim) => match dim.parse::<u16>() {
                                Ok(dim) => Some(dim),
                                Err(_) => None,
                            },
                            None => None,
                        };
                        set.insert(AppIcon {
                            name: path.file_name().unwrap().to_str().unwrap().to_string(),
                            path: path.to_path_buf(),
                            dimensions: dim, // dimensions,
                        });
                    }
                }
                None => {
                    continue;
                }
            }
        }
    }
    let mut map: HashMap<String, Vec<AppIcon>> = HashMap::new();
    for icon in set {
        let name = icon.name.clone();
        let name = &name[0..name.len() - 4]; // remove .png
        if map.contains_key(name) {
            map.get_mut(name).unwrap().push(icon);
        } else {
            map.insert(name.to_string(), vec![icon]);
        }
    }
    // sort icons by dimensions
    for (_, icons) in map.iter_mut() {
        icons.sort_by(|a, b| {
            if a.dimensions.is_none() && b.dimensions.is_none() {
                return std::cmp::Ordering::Equal;
            }
            if a.dimensions.is_none() {
                return std::cmp::Ordering::Greater;
            }
            if b.dimensions.is_none() {
                return std::cmp::Ordering::Less;
            }
            b.dimensions.unwrap().cmp(&a.dimensions.unwrap())
        });
    }
    Ok(map)
}

pub fn open_file_with(file_path: PathBuf, app: App) {
    let exe_path = app.app_path_exe.unwrap();
    let exec_path_str = exe_path.to_str().unwrap();
    let file_path_str = file_path.to_str().unwrap();
    let output = std::process::Command::new(exec_path_str)
        .arg(file_path_str)
        .output()
        .expect("failed to execute process");
    if !output.status.success() {
        panic!("failed to execute process");
    }
}

pub fn get_running_apps() -> Vec<App> {
    todo!()
}

/// TODO: this is not working yet, xprop gives the current app name, but we need to locate its .desktop file if possible
/// If I need to compare app name with app apps, then this function should be moved to AppInfoContext where there is a `cached_apps`
pub fn get_frontmost_application() -> Result<App> {
    let output = std::process::Command::new("xprop")
        .arg("-root")
        .arg("_NET_ACTIVE_WINDOW")
        .output()
        .expect("failed to execute process");

    let output = std::str::from_utf8(&output.stdout).unwrap();
    let id = output.split_whitespace().last().unwrap();

    let output = std::process::Command::new("xprop")
        .arg("-id")
        .arg(id)
        .arg("WM_CLASS")
        .output()
        .expect("failed to execute process");

    let output = std::str::from_utf8(&output.stdout).unwrap();
    let app_name = output.split('"').nth(1).unwrap();

    let apps = get_all_apps(&vec![])?;
    for app in apps {
        if app.name == app_name {
            return Ok(app);
        }
    }

    Err(anyhow::Error::msg("No matching app found".to_string()))
}

impl AppTrait for App {
    fn load_icon(&self) -> Result<crate::utils::image::RustImageData> {
        match &self.icon_path {
            Some(icon_path) => {
                let icon_path_str = icon_path
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Failed to convert icon path to string"))?;
                let image = crate::utils::image::RustImageData::from_path(icon_path_str)
                    .map_err(|e| anyhow::anyhow!("Failed to get icon: {}", e))?;
                Ok(image)
            }
            None => Err(anyhow::Error::msg("Icon path is None".to_string())),
        }
    }

    fn from_path(path: &Path) -> Result<Self> {
        todo!()
    }
}

/// path should be a .png file, Linux icon can also be a .svg file, don't use this function in that case
pub fn load_icon(path: &Path) -> Result<RustImageData> {
    // if path is a .svg file
    if path.extension().unwrap() == "svg" {
        return Err(anyhow::anyhow!("SVG files are not supported on Linux yet"));
    }
    let image = RustImageData::from_path(path.to_str().unwrap())
        .map_err(|e| anyhow::anyhow!("Failed to get icon: {}", e))?;
    Ok(image)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_clean_exec_path() {
        assert_eq!(clean_exec_path("code %f").to_string(), "code");
        assert_eq!(clean_exec_path("code %f %F").to_string(), "code");
        assert_eq!(clean_exec_path("\"/home/hacker/.local/share/JetBrains/Toolbox/apps/intellij-idea-ultimate/bin/idea\" %u").to_string(), "\"/home/hacker/.local/share/JetBrains/Toolbox/apps/intellij-idea-ultimate/bin/idea\"");
    }

    #[test]
    fn test_get_apps() {
        let apps = get_all_apps(&[PathBuf::from("/home/steve/.local/share/flatpak/app")]).unwrap();
        assert!(!apps.is_empty());
    }

    #[test]
    fn test_find_all_app_icons() {
        let start = std::time::Instant::now();
        let icons_icons = find_all_app_icons().unwrap();
        let elapsed = start.elapsed();
        assert!(!icons_icons.is_empty());
        println!("Elapsed: {:?}", elapsed);
    }
}