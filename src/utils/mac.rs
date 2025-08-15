use crate::common::App;
use anyhow::anyhow;
use anyhow::Result;
use glob::glob;
use plist::Value as PlistValue;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacSystemProfilerAppList {
    #[serde(rename = "SPApplicationsDataType")]
    pub spapplications_data_type: Vec<MacSystemProfilterAppInfo>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacSystemProfilterAppInfo {
    #[serde(rename = "_name")]
    pub name: String,
    #[serde(rename = "arch_kind")]
    pub arch_kind: String,
    pub last_modified: String,
    #[serde(rename = "obtained_from")]
    pub obtained_from: String,
    pub path: String,
    #[serde(rename = "signed_by")]
    pub signed_by: Option<Vec<String>>,
    pub version: Option<String>,
    pub info: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CFBundlePrimaryIcon {
    #[serde(rename = "CFBundleIconName")]
    cf_bundle_icon_name: Option<String>,
    #[serde(rename = "CFBundleIconFiles")]
    cf_bundle_icon_files: Option<Vec<String>>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CFBundleIcons {
    #[serde(rename = "CFBundlePrimaryIcon")]
    cf_bundle_primary_icon: Option<CFBundlePrimaryIcon>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct InfoPlist {
    #[serde(rename = "CFBundleIconFile")]
    cf_bundle_icon_file: Option<String>,
    #[serde(rename = "CFBundleIcons")]
    cf_bundle_icons: Option<CFBundleIcons>,
    #[serde(rename = "CFBundleIcons~ipad")]
    cf_bundle_icons_ipad: Option<CFBundleIcons>,
    #[serde(rename = "CFBundleExecutable")]
    cf_bundle_executable: Option<String>,
    #[serde(rename = "CFBundleIconName")]
    cf_bundle_icon_name: Option<String>,
    #[serde(rename = "CFBundleIdentifier")]
    cf_bundle_identifier: Option<String>,
    #[serde(rename = "CFBundleInfoDictionaryVersion")]
    cf_bundle_info_dictionary_version: Option<String>,
    #[serde(rename = "CFBundleName")]
    cf_bundle_name: Option<String>,
    #[serde(rename = "CFBundlePackageType")]
    cf_bundle_package_type: Option<String>,
    #[serde(rename = "CFBundleShortVersionString")]
    cf_bundle_short_version_string: Option<String>,
    #[serde(rename = "CFBundleVersion")]
    cf_bundle_version: Option<String>,
    #[serde(rename = "CFBundleDisplayName")]
    cf_bundle_display_name: Option<String>,
}

impl InfoPlist {
    pub fn from_value(value: &plist::Value) -> Result<InfoPlist> {
        let info_plist = plist::from_value(value).unwrap();
        Ok(info_plist)
    }

    pub fn from_file(path: &PathBuf) -> Result<InfoPlist> {
        match plist::from_file(path) {
            Ok(info_plist) => Ok(info_plist),
            Err(_) => match plist::Value::from_file(path) {
                // using plist::Value is a workaround for the error "duplicate key: CFBundleShortVersionString"
                Ok(value) => Ok(InfoPlist::from_value(&value).unwrap()),
                Err(err) => Err(anyhow::Error::msg(format!("Fail to parse plist: {}", err))),
            },
        }
    }
}

fn run_mdfind_only_in(dir: &Path) -> Result<Vec<String>> {
    let output = std::process::Command::new("mdfind")
        .arg("-onlyin")
        .arg(format!("{}", dir.display()))
        .arg("kMDItemKind == 'Application'")
        .output()?;

    if !output.status.success() {
        return Err(anyhow!(
            "failed to spawn mdfind, stderr [{}]",
            String::from_utf8_lossy(&output.stdout)
        ));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let lines1: Vec<String> = stdout.split("\n").map(|line| line.to_string()).collect();

    let output = std::process::Command::new("mdfind")
        .arg("kMDItemContentType = 'com.apple.application-bundle'")
        .arg("-onlyin")
        .arg(format!("{}", dir.display()))
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "failed to spawn mdfind, stderr [{}]",
            String::from_utf8_lossy(&output.stdout)
        ));
    }
    let stdout = String::from_utf8(output.stdout)?;
    let lines2: Vec<String> = stdout.split("\n").map(|line| line.to_string()).collect();

    Ok(lines1
        .into_iter()
        .chain(lines2)
        .collect::<std::collections::HashSet<String>>()
        .into_iter()
        .collect())
}

pub fn run_mdfind_to_get_app_list(search_paths: &[PathBuf]) -> Result<Vec<String>> {
    let mut set = HashSet::new();

    for search_path in search_paths {
        let apps = run_mdfind_only_in(search_path)?;
        set.extend(apps);
    }

    Ok(set.into_iter().collect())
}

/// Mac App folder is very complicated, I made this struct with some helper functions to make it easier to work with
pub struct MacAppPath(PathBuf);

impl MacAppPath {
    pub fn new(path: PathBuf) -> Self {
        MacAppPath(path)
    }

    pub fn is_app(&self) -> bool {
        if !self.0.exists() {
            return false;
        }

        if self.has_wrapper() {
            // iOS app
            self.has_info_plist()
        } else {
            // macOS app
            let has_info_plist = self.has_info_plist();
            let has_resource_folder = {
                let resources = self.0.join("Contents/Resources");
                resources.exists()
            };

            has_info_plist && has_resource_folder
        }
    }

    /// Check if the path has a Wrapper folder
    /// iOS apps can run on Apple Silicon Macs, but these apps have different structures
    /// iOS apps are wrapped in a Wrapper folder
    /// For normal Mac apps, this function will always return false
    /// because Mac apps don't have a Wrapper folder
    /// For iOS apps, this function will return true if the Wrapper folder exists
    pub fn has_wrapper(&self) -> bool {
        match self.get_wrapper_path() {
            Some(path) => path.exists(),
            None => false,
        }
    }

    /// Get the path to the Wrapper folder
    /// iPad apps are wrapped in a Wrapper folder
    pub fn get_wrapper_path(&self) -> Option<PathBuf> {
        match self.0.join("Wrapper") {
            path if path.exists() => Some(path),
            _ => None,
        }
    }

    /// Get the path to the first inner .app folder in the Wrapper, if it exists
    /// iPad apps are wrapped in a Wrapper folder
    /// Here we assume there is only one inner .app folder, otherwise the logic will get too complicated
    pub fn get_app_path_in_wrapper(&self) -> Option<PathBuf> {
        let wrapper_path = self.get_wrapper_path()?;
        let wrapper_path_str = wrapper_path.to_str()?;
        // search for .app in the wrapper
        let glob_path = format!("{}/*.app", wrapper_path_str);
        if let Some(e) = glob(&glob_path)
            .expect("Failed to read glob pattern")
            .next()
        {
            return Some(e.unwrap());
        }
        None
    }

    pub fn has_info_plist(&self) -> bool {
        self.get_info_plist_path().is_some()
    }

    pub fn get_info_plist_path(&self) -> Option<PathBuf> {
        if self.has_wrapper() {
            let app_path_in_wrapper = self.get_app_path_in_wrapper()?;
            let path = app_path_in_wrapper.join("Info.plist"); // iOS apps doesn't have Contents folder
            match path.exists() {
                true => Some(path),
                false => None,
            }
        } else {
            let path = self.0.join("Contents").join("Info.plist");
            match path.exists() {
                true => Some(path),
                false => None,
            }
        }
    }

    /// Convert the MacAppPath to an App struct
    ///
    /// This function will return None if the path is not an app
    pub fn to_app(&self) -> Option<App> {
        // Validate it
        if !self.is_app() {
            return None;
        }
        let info_plist_path = self
            .get_info_plist_path()
            .expect("is_app() ensures that there is an Info.plist file");
        // If the Info.plist file is invalid, this is not an app, return None.
        let info_plist = InfoPlist::from_file(&info_plist_path).ok()?;

        /* App Name */
        let name = {
            if let Some(ref display_name) = info_plist.cf_bundle_display_name {
                display_name.to_string()
            } else if let Some(ref name) = info_plist.cf_bundle_name {
                name.to_string()
            } else {
                self.0.file_stem()?.to_str()?.to_string()
            }
        };
        let localized_app_names = self.get_localized_app_names();

        /* Executable file */
        let is_ios_app = self.has_wrapper();
        // Handle iOS apps differently - they have different paths
        let (resources_path, app_path_exe) = if is_ios_app {
            // For iOS apps, use the inner app path
            let inner_app_path = self.get_app_path_in_wrapper()?;
            let resources_path = inner_app_path.clone();
            let executable = info_plist.cf_bundle_executable.clone()?;
            let app_path_exe = inner_app_path.join(executable);
            (resources_path, Some(app_path_exe))
        } else {
            // For regular Mac apps
            let contents_path = self.0.join("Contents");
            let resources_path = contents_path.join("Resources");
            let macos_path = contents_path.join("MacOS");
            let app_path_exe = match info_plist.cf_bundle_executable.clone() {
                Some(executable) => {
                    let app_path_exe = macos_path.join(executable);
                    if app_path_exe.exists() {
                        Some(app_path_exe)
                    } else {
                        None
                    }
                }
                None => None,
            };
            (resources_path, app_path_exe)
        };

        /* Icon file */
        let icon_path = self.find_icon_path(&info_plist, &resources_path, is_ios_app);

        Some(App {
            name,
            localized_app_names,
            icon_path,
            app_path_exe,
            app_desktop_path: self.0.clone(),
        })
    }

    fn find_icon_path(
        &self,
        info_plist: &InfoPlist,
        resources_path: &PathBuf,
        is_ios_app: bool,
    ) -> Option<PathBuf> {
        if is_ios_app {
            // For iOS apps, icons are in the app root, not Resources folder
            let app_root = resources_path.clone(); // resources_path is actually the app root for iOS

            // Strategy 1: Check CFBundleIcons for iOS apps
            if let Some(icons) = &info_plist.cf_bundle_icons {
                if let Some(primary_icon) = &icons.cf_bundle_primary_icon {
                    if let Some(icon_files) = &primary_icon.cf_bundle_icon_files {
                        for icon_file in icon_files {
                            // Try different PNG file patterns for iOS
                            let patterns = [
                                format!("{}.png", icon_file),
                                format!("{}@2x.png", icon_file),
                                format!("{}@3x.png", icon_file),
                            ];

                            for pattern in &patterns {
                                let icon_path = app_root.join(pattern);
                                if icon_path.exists() {
                                    return Some(icon_path);
                                }
                            }
                        }
                    }
                }
            }

            // Strategy 2: Check CFBundleIcons~ipad for iPad-specific icons
            if let Some(icons) = &info_plist.cf_bundle_icons_ipad {
                if let Some(primary_icon) = &icons.cf_bundle_primary_icon {
                    if let Some(icon_files) = &primary_icon.cf_bundle_icon_files {
                        for icon_file in icon_files {
                            let patterns = [
                                format!("{}.png", icon_file),
                                format!("{}@2x.png", icon_file),
                            ];

                            for pattern in &patterns {
                                let icon_path = app_root.join(pattern);
                                if icon_path.exists() {
                                    return Some(icon_path);
                                }
                            }
                        }
                    }
                }
            }

            // Strategy 3: Check for common iOS icon patterns
            let common_ios_icons = [
                "AppIcon60x60@2x.png",
                "AppIcon60x60@3x.png",
                "AppIcon76x76@2x~ipad.png",
                "AppIcon83.5x83.5@2x~ipad.png",
                "AppIcon29x29@2x.png",
                "AppIcon40x40@2x.png",
                "AppIcon57x57@2x.png",
                "AppIcon72x72@2x~ipad.png",
            ];

            for icon_name in &common_ios_icons {
                let icon_path = app_root.join(icon_name);
                if icon_path.exists() {
                    return Some(icon_path);
                }
            }

            // Strategy 4: Check for Assets.car in iOS app root
            let assets_car_path = app_root.join("Assets.car");
            if assets_car_path.exists() {
                return Some(assets_car_path);
            }

            // Strategy 5: Check for any PNG files starting with AppIcon or Icon
            let png_pattern = app_root.join("AppIcon*.png");
            if let Ok(png_files) = glob::glob(&png_pattern.to_string_lossy()) {
                if let Some(Ok(png_file)) = png_files.into_iter().next() {
                    return Some(png_file);
                }
            }

            let icon_pattern = app_root.join("Icon*.png");
            if let Ok(png_files) = glob::glob(&icon_pattern.to_string_lossy()) {
                if let Some(Ok(png_file)) = png_files.into_iter().next() {
                    return Some(png_file);
                }
            }
        } else {
            // For regular macOS apps

            // Strategy 1: Try direct icon file from CFBundleIconFile
            if let Some(icon_file_name) = &info_plist.cf_bundle_icon_file {
                let icon_file_name = if icon_file_name.ends_with(".icns") {
                    icon_file_name.clone()
                } else {
                    format!("{}.icns", icon_file_name)
                };
                let icon_path = resources_path.join(icon_file_name);
                if icon_path.exists() {
                    return Some(icon_path);
                }
            }

            // Strategy 2: Try icon name from CFBundleIconName
            if let Some(icon_name) = &info_plist.cf_bundle_icon_name {
                let icon_file_name = format!("{}.icns", icon_name);
                let icon_path = resources_path.join(icon_file_name);
                if icon_path.exists() {
                    return Some(icon_path);
                }
            }

            // Strategy 3: Check for common icon file patterns
            let common_icon_names = ["AppIcon.icns", "app.icns", "icon.icns", "application.icns"];

            for icon_name in &common_icon_names {
                let icon_path = resources_path.join(icon_name);
                if icon_path.exists() {
                    return Some(icon_path);
                }
            }

            // Strategy 4: Check for any .icns files in Resources
            let icns_pattern = resources_path.join("*.icns");
            if let Ok(icns_files) = glob::glob(&icns_pattern.to_string_lossy()) {
                if let Some(Ok(icns_file)) = icns_files.into_iter().next() {
                    return Some(icns_file);
                }
            }

            // Strategy 5: Check for Assets.car (modern asset catalog)
            let assets_car_path = resources_path.join("Assets.car");
            if assets_car_path.exists() {
                return Some(assets_car_path);
            }
        }

        None
    }

    fn get_localized_app_names(&self) -> HashMap<String, String> {
        // support for iOS apps has not be implemented
        if self.has_wrapper() {
            return HashMap::new();
        }

        let mut names = HashMap::new();
        let resources_path = self.0.join("Contents/Resources");

        // InfoPlist.loctable is a modern replace for those "*.lproj" folders.
        let infoplist_path = resources_path.join("InfoPlist.loctable");
        if infoplist_path.exists() {
            if let Ok(loctable) = read_loctable(&infoplist_path) {
                extract_names_from_loctable(&loctable, &mut names);
            }
        }

        // Try to read from all lproj directories
        extract_from_all_lproj_dirs(&resources_path, &mut names).unwrap();

        names
    }
}

fn read_loctable(path: &Path) -> Result<PlistValue, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open plist: {}", e))?;
    let reader = BufReader::new(file);
    PlistValue::from_reader(reader).map_err(|e| format!("Failed to parse plist: {}", e))
}

fn extract_names_from_loctable(loctable: &PlistValue, names: &mut HashMap<String, String>) {
    // Look for CFBundleDisplayName or CFBundleName in different locales
    if let Some(dict) = loctable.as_dictionary() {
        for (locale, value) in dict {
            if let Some(locale_dict) = value.as_dictionary() {
                if let Some(display_name) = locale_dict.get("CFBundleDisplayName") {
                    if let Some(name) = display_name.as_string() {
                        names.insert(locale.to_string(), name.to_string());
                    }
                } else if let Some(bundle_name) = locale_dict.get("CFBundleName") {
                    if let Some(name) = bundle_name.as_string() {
                        names.insert(locale.to_string(), name.to_string());
                    }
                }
            }
        }
    }
}

/// InfoPlist.strings can be in:
///
/// * Apple binary property list
/// * Plain text key-value pairs, which can be in UTF-8 and UTF-16 encoded
fn infoplist_strings_parser(path: &Path) -> HashMap<String, String> {
    let mut result = HashMap::new();

    // Try to parse as binary plist first
    if let Ok(plist) = plist::from_file::<_, PlistValue>(path) {
        if let Some(dict) = plist.as_dictionary() {
            for (key, value) in dict {
                if let Some(val_str) = value.as_string() {
                    result.insert(key.to_string(), val_str.to_string());
                }
            }
            return result;
        }
    }

    // Fall back to text parsing for UTF-16 and UTF-8 formats
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return result,
    };

    let content = if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        // UTF-16 little-endian BOM detected
        let utf16: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16_lossy(&utf16)
    } else {
        // Try UTF-8
        String::from_utf8_lossy(&bytes).into_owned()
    };

    // Parse the property list format line by line
    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty()
            || line.starts_with("/*")
            || line.starts_with("//")
            || line.ends_with("*/")
        {
            continue;
        }

        if let Some(eq_pos) = line.find('=') {
            let mut key = line[..eq_pos].trim();
            let mut value = line[eq_pos + 1..].trim();

            // key has optional double quotes
            key = key.trim_matches('"');
            // Value has optional tailing semicolon
            value = value.trim_end_matches(';');
            // Value has surrounding double-quotes
            value = value.trim_matches('"');

            result.insert(key.to_string(), value.to_string());
        }
    }

    result
}

fn extract_from_all_lproj_dirs(
    resources_path: &Path,
    names: &mut HashMap<String, String>,
) -> Result<(), String> {
    const LPROJ: &str = ".lproj";

    // Find all .lproj directories
    if let Ok(entries) = std::fs::read_dir(resources_path) {
        for res_entry in entries {
            let entry = res_entry.unwrap();
            let file_path = entry.path();
            let Some(file_name_os_str) = file_path.file_name() else {
                continue;
            };
            let Some(file_name) = file_name_os_str.to_str() else {
                // The directories we want should have UTF-8 encoded name
                continue;
            };

            if file_path.is_dir() && file_name.ends_with(LPROJ) {
                let localized_info_plist_path = file_path.join("InfoPlist.strings");
                if !localized_info_plist_path.try_exists().expect("TODO") {
                    continue;
                }
                let info_plist_kvs: HashMap<String, String> =
                    infoplist_strings_parser(&localized_info_plist_path);

                let locale = file_name.trim_end_matches(LPROJ);
                // Some apps use "zh-CN.lproj" rather than "zh_CN.lproj"
                let locale = locale.replace('-', "_");

                if let Some(display_name) = info_plist_kvs.get("CFBundleDisplayName") {
                    names.insert(locale.to_string(), display_name.clone());
                    continue;
                }

                if let Some(display_name) = info_plist_kvs.get("CFBundleName") {
                    names.insert(locale.to_string(), display_name.clone());
                    continue;
                }
            }
        }
    }

    Ok(())
}
