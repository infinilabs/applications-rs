mod common;
// difference platforms may have different implementation and signatures for each function, so platforms will not be public
mod platforms;
mod utils;
pub mod watcher;

pub use common::{App, AppTrait};
pub use platforms::{get_all_apps, get_default_search_paths};
