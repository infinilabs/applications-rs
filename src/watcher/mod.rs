use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

#[derive(Debug)]
pub enum Change {
    AppInstalled {
        app_path: PathBuf,
    },
    /// NOTE: Since `app_path` has been deleted, so there is no way we can check it, which
    /// means there are cases where `app_path` is not a an application.
    AppDeleted {
        app_path: PathBuf,
    },
}
