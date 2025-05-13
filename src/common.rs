//! Common Data Structures

use crate::utils::image::RustImageData;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, Eq, Hash)]
pub struct App {
    pub name: String,
    pub icon_path: Option<PathBuf>,
    /// Path to the .app file for mac, or Exec for Linux, or .exe for Windows
    pub app_path_exe: Option<PathBuf>,
    // Path to the .desktop file for Linux, .app for Mac
    pub app_desktop_path: PathBuf,
}

/// This trait specifies the methods that an app should implement, such as loading its logo
pub trait AppTrait
where
    Self: Sized,
{
    fn load_icon(&self) -> Result<RustImageData>;
    fn from_path(path: &Path) -> Result<Self>;
}
