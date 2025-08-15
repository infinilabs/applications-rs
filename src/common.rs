//! Common Data Structures

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, Eq, Hash)]
pub struct App {
    /// Base name. Should only be used when the localized app name needed is
    /// not found.
    ///
    /// On macOS, this will be the "CFBundleDisplayName" or "CFBundleName"
    /// defined in "Info.plist", or the stem part of the app package's file
    /// name (Finder.app => Finder).
    pub name: String,
    /// Localized app names, for example:
    ///
    /// ```text
    /// en: Finder
    /// zh_CN: 访达
    /// zh_HK: Finder
    /// zh_TW: Finder
    /// ```
    pub localized_app_names: BTreeMap<String, String>,
    /// Path to the icon file.
    pub icon_path: Option<PathBuf>,
    /// Path to the executable file.
    pub app_path_exe: Option<PathBuf>,
    // Path to the .desktop file for Linux, .app for Mac
    pub app_desktop_path: PathBuf,
}

/// This trait specifies the methods that an app should implement, such as loading its logo
pub trait AppTrait
where
    Self: Sized,
{
    fn from_path(path: &Path) -> Result<Self>;
}
