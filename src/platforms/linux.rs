use crate::common::App;
use crate::utils::image::RustImage;
use crate::AppTrait;
use anyhow::Result;
use freedesktop_file_parser::{parse, EntryType};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

const FLATPAK_GLOBAL_APP_PATH: &str = "/var/lib/flatpak/app";
static FLATPAK_PERSONAL_APP_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let home_dir =
        PathBuf::from(std::env::var_os("HOME").expect("environment variable $HOME not found"));
    home_dir.join(".local/share/flatpak/app")
});

#[derive(Debug, PartialEq, Clone, Default, Eq, Hash, Serialize, Deserialize)]
pub struct AppIcon {
    name: String,
    path: PathBuf,
    dimensions: Option<u16>,
}

pub(crate) fn parse_desktop_file_content(content: &str) -> Option<(String, Option<PathBuf>)> {
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

    app_fields.exec?;

    let icon = desktop_file_entry.icon?;

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
        FLATPAK_GLOBAL_APP_PATH.into(),
        FLATPAK_PERSONAL_APP_PATH.to_path_buf(),
    ]
}

/// Specialized implementation for Flatpak
///
/// Flatpak Application desktop file path:
///
/// ```text
/// <flatpak_app_path>/<app_identifer>/current/active/files/share/applications/<app_identifier>.desktop
/// ```
fn get_flatpak_applications(flatpak_app_path: &Path) -> Result<Vec<App>> {
    let dir = std::fs::read_dir(flatpak_app_path)?;
    let mut apps = Vec::new();

    for res_entry in dir {
        let entry = res_entry?;
        let app_desktop_file_path = {
            let mut path = entry.path();
            path.push("current/active/files/share/applications");
            let app_identifier = entry
                .file_name()
                .into_string()
                .expect("flatpak app identifier should be UTF-8 encoded");
            path.push(format!("{}.desktop", app_identifier));

            path
        };

        if !app_desktop_file_path.try_exists()? {
            continue;
        }

        let desktop_file_content = std::fs::read_to_string(&app_desktop_file_path)?;
        let Some((app_name, opt_icon_path)) = parse_desktop_file_content(&desktop_file_content)
        else {
            continue;
        };

        let app = App {
            name: app_name,
            icon_path: opt_icon_path,
            app_path_exe: None,
            app_desktop_path: app_desktop_file_path,
        };
        apps.push(app);
    }

    Ok(apps)
}

pub fn get_all_apps(search_paths: &[PathBuf]) -> Result<Vec<App>> {
    let search_dirs: HashSet<&PathBuf> = search_paths.iter().filter(|dir| dir.exists()).collect();

    // for each dir, search for .desktop files
    let mut apps: HashSet<App> = HashSet::new();
    for dir in search_dirs {
        // Specialized impl for Flatpak
        if dir == Path::new(FLATPAK_GLOBAL_APP_PATH) || dir == &*FLATPAK_PERSONAL_APP_PATH {
            let flatpak_apps = get_flatpak_applications(dir.as_path())?;
            apps.extend(flatpak_apps);

            continue;
        }

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

pub fn get_frontmost_application() -> Result<App> {
    unimplemented!()
}

pub fn get_running_apps() -> Vec<App> {
    unimplemented!()
}
pub fn open_file_with(_file_path: PathBuf, _app: App) {
    unimplemented!()
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
        let desktop_file_content = std::fs::read_to_string(&path)?;
        let Some((app_name, opt_icon_path)) = parse_desktop_file_content(&desktop_file_content)
        else {
            return Err(anyhow::anyhow!("invalid desktop file"));
        };

        Ok(App {
            name: app_name,
            icon_path: opt_icon_path,
            app_path_exe: None,
            app_desktop_path: path.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_apps() {
        let default_search_path = get_default_search_paths();
        let apps = get_all_apps(&default_search_path).unwrap();
        assert!(!apps.is_empty());
    }

    #[test]
    fn test_parse_desktop_file_content_invalid_content() {
        let invalid_content = "";
        assert!(parse_desktop_file_content(invalid_content).is_none());

        let without_type = r#"[Desktop Entry]
Version=1.0
Name = "Zed"
"#;
        assert!(parse_desktop_file_content(without_type).is_none());
    }

    #[test]
    fn test_parse_desktop_file_content() {
        let zed = r#"[Desktop Entry]
Version=1.0
Type=Application
Name=Zed
GenericName=Text Editor
Comment=A high-performance, multiplayer code editor.
TryExec=/home/foo/.local/zed.app/libexec/zed-editor
StartupNotify=true
Exec=/home/foo/.local/zed.app/libexec/zed-editor %U
Icon=/home/foo/.local/zed.app/share/icons/hicolor/512x512/apps/zed.png
Categories=Utility;TextEditor;Development;IDE;
Keywords=zed;
MimeType=text/plain;application/x-zerosize;x-scheme-handler/zed;
Actions=NewWorkspace;

[Desktop Action NewWorkspace]
Exec=/home/foo/.local/zed.app/libexec/zed-editor --new %U
Name=Open a new workspace"#;

        let (name, _opt_icon_path) = parse_desktop_file_content(zed).unwrap();

        assert_eq!(name, "Zed");
    }

    #[test]
    fn test_parse_desktop_file_content_no_exec() {
        let zed = r#"[Desktop Entry]
Version=1.0
Type=Application
Name=Zed
GenericName=Text Editor
Comment=A high-performance, multiplayer code editor.
TryExec=/home/foo/.local/zed.app/libexec/zed-editor
StartupNotify=true
Icon=/home/foo/.local/zed.app/share/icons/hicolor/512x512/apps/zed.png
Categories=Utility;TextEditor;Development;IDE;
Keywords=zed;
MimeType=text/plain;application/x-zerosize;x-scheme-handler/zed;
Actions=NewWorkspace;

[Desktop Action NewWorkspace]
Name=Open a new workspace"#;

        assert!(parse_desktop_file_content(zed).is_none());
    }

    #[test]
    fn test_parse_desktop_file_content_no_icon() {
        let zed = r#"[Desktop Entry]
Version=1.0
Type=Application
Name=Zed
GenericName=Text Editor
Comment=A high-performance, multiplayer code editor.
TryExec=/home/foo/.local/zed.app/libexec/zed-editor
StartupNotify=true
Exec=/home/foo/.local/zed.app/libexec/zed-editor %U
Categories=Utility;TextEditor;Development;IDE;
Keywords=zed;
MimeType=text/plain;application/x-zerosize;x-scheme-handler/zed;
Actions=NewWorkspace;

[Desktop Action NewWorkspace]
Exec=/home/foo/.local/zed.app/libexec/zed-editor --new %U
Name=Open a new workspace"#;

        assert!(parse_desktop_file_content(zed).is_none());
    }

    #[test]
    fn test_parse_desktop_file_content_no_display_is_set() {
        let zed = r#"[Desktop Entry]
Version=1.0
Type=Application
Name=Zed
GenericName=Text Editor
Comment=A high-performance, multiplayer code editor.
TryExec=/home/foo/.local/zed.app/libexec/zed-editor
StartupNotify=true
Exec=/home/foo/.local/zed.app/libexec/zed-editor %U
Icon=/home/foo/.local/zed.app/share/icons/hicolor/512x512/apps/zed.png
Categories=Utility;TextEditor;Development;IDE;
NoDisplay=true
Keywords=zed;
MimeType=text/plain;application/x-zerosize;x-scheme-handler/zed;
Actions=NewWorkspace;

[Desktop Action NewWorkspace]
Exec=/home/foo/.local/zed.app/libexec/zed-editor --new %U
Name=Open a new workspace"#;

        assert!(parse_desktop_file_content(zed).is_none());
    }
}
