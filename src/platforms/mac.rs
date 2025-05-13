use crate::common::{App, AppTrait};
use crate::utils::mac::{run_mdfind_to_get_app_list, MacAppPath, MacSystemProfilterAppInfo};
use anyhow::Result;
use std::path::{Path, PathBuf};

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

impl AppTrait for App {
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
