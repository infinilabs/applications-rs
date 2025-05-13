use crate::common::{App, AppTrait};
use crate::utils::image::{RustImage, RustImageData};
use crate::utils::mac::{run_mdfind_to_get_app_list, MacAppPath, MacSystemProfilterAppInfo};
use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use tauri_icns::{IconFamily, IconType};

pub fn get_all_apps_mdfind(search_paths: &[PathBuf]) -> Result<Vec<App>> {
    let apps_list = run_mdfind_to_get_app_list(search_paths)?;
    Ok(apps_list
        .iter()
        .map(|app_path| MacAppPath::new(PathBuf::from(app_path)).to_app())
        .filter_map(|x| x)
        .collect())
}

pub fn get_default_search_paths() -> Vec<PathBuf> {
    Vec::new()
}

pub fn get_all_apps(search_paths: &[PathBuf]) -> Result<Vec<App>> {
    get_all_apps_mdfind(search_paths)
}

impl From<MacSystemProfilterAppInfo> for Option<App> {
    fn from(app_info: MacSystemProfilterAppInfo) -> Self {
        let app_path = MacAppPath::new(PathBuf::from(app_info.path));
        app_path.to_app()
    }
}

/// path can be the path to .app folder or .icns file
pub fn load_icon(path: &Path) -> Result<RustImageData> {
    // check file type and file extension
    let file = File::open(path)
        .map_err(|e| anyhow::Error::msg(format!("Failed to open icon file: {}", e)))?;
    let file_type = file
        .metadata()
        .map_err(|e| anyhow::Error::msg(format!("Failed to get file metadata: {}", e)))?
        .file_type();
    let file_extension = path.extension().unwrap_or_default();
    if file_type.is_dir() {
        // it's a .app folder
        let app = App::from_path(path)
            .map_err(|e| anyhow::Error::msg(format!("Failed to create App from path: {}", e)))?;
        app.load_icon()
    } else if file_extension == "icns" {
        let file = BufReader::new(file);
        let icon_family = IconFamily::read(file)
            .map_err(|e| anyhow::Error::msg(format!("Failed to read icon family: {}", e)))?;

        let mut largest_icon_type = IconType::Mask8_16x16;
        let mut largest_width = 0;
        for icon_type in icon_family.available_icons() {
            let icon_type_width = icon_type.pixel_width();
            if icon_type_width > largest_width {
                largest_width = icon_type_width;
                largest_icon_type = icon_type;
                if largest_width >= 64 {
                    // width 256 is large enough
                    break;
                }
            }
        }

        let largest_icon = icon_family.get_icon_with_type(largest_icon_type)?;
        let mut buffer: Vec<u8> = Vec::new();
        let cursor = Cursor::new(&mut buffer);
        largest_icon
            .write_png(cursor)
            .map_err(|e| anyhow::Error::msg(format!("Failed to write PNG: {}", e)))?;

        let bytes: &[u8] = &buffer;
        RustImageData::from_bytes(bytes)
            .map_err(|e| anyhow::Error::msg(format!("Failed to create image from bytes: {}", e)))
        // Ok(RustImageData::from_dynamic_image(image::DynamicImage::ImageRgba8(icon)))
    } else {
        Err(anyhow::Error::msg("Failed to load icon"))
    }
}

impl AppTrait for App {
    fn load_icon(&self) -> Result<RustImageData> {
        if let Some(icon_path) = &self.icon_path {
            load_icon(icon_path)
        } else {
            Err(anyhow::Error::msg("No icon path available"))
        }
    }

    fn from_path(path: &Path) -> Result<Self> {
        MacAppPath::new(path.to_path_buf())
            .to_app()
            .ok_or(anyhow::Error::msg("Failed to create App from path"))
    }
}

// generate test
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::mac::MacAppPath;

    #[test]
    fn find_info_plist() {
        let apps = super::get_all_apps(&[]).unwrap();
        for app in apps {
            let path = app.app_desktop_path;
            let mac_app_path = MacAppPath::new(path.clone());
            let info_plist_path = mac_app_path.get_info_plist_path();
            if info_plist_path.is_none() {
                println!("Info.plist not found: {:?}", path);
            }
        }
    }

    #[test]
    fn test_get_all_apps() {
        let apps = get_all_apps(&[PathBuf::from("/"), PathBuf::from("/Users/home/steve")]).unwrap();
        assert!(apps.iter().any(|app| app.name == "Finder"));
        assert!(apps.iter().any(|app| app.name == "Spotlight"));
        assert!(apps.iter().any(|app| app.name == "App Store"));
        assert!(apps.iter().any(|app| app.name == "Maps"));
        assert!(apps.iter().any(|app| app.name == "Mail"));
        assert!(apps.iter().any(|app| app.name == "FaceTime"));
        assert!(apps.iter().any(|app| app.name == "Weather"));
        assert!(apps.iter().any(|app| app.name == "Stocks"));
        assert!(apps.iter().any(|app| app.name == "Books"));
        assert!(apps.iter().any(|app| app.name == "Preview"));

        // No idea why `apps` does not contain Safari.app
        // assert!(apps.iter().any(|app| app.name == "Safari"));
        //
        // Searching in `/` returns nothing, but doing it in `/Applications`
        // returns the result. Quite weird considering `/Application` is a descendant of `/`.
        //
        // $ mdfind -onlyin / "kMDItemKind == 'Application'" | rg -i safari
        //
        // $ mdfind -onlyin /Applications "kMDItemKind == 'Application'" | rg -i safari
        // /Applications/Safari.app
    }
}
