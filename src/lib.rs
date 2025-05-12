pub mod api;
pub mod common;
// difference platforms may have different implementation and signatures for each function, so platforms will not be public
mod platforms;
pub mod utils;
pub mod watcher;

pub use common::{App, AppInfo, AppInfoContext, AppTrait};
pub use platforms::{get_all_apps, get_default_search_paths};
