use super::Change;
use crate::platforms::parse_desktop_file_content;
use anyhow::Result;
use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify, WatchDescriptor};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The flag we use when adding new entries.
fn watch_flag() -> AddWatchFlags {
    AddWatchFlags::IN_CREATE
        | AddWatchFlags::IN_DELETE
        | AddWatchFlags::IN_MOVE
        | AddWatchFlags::IN_DELETE_SELF
        | AddWatchFlags::IN_MOVE_SELF
        | AddWatchFlags::IN_ONLYDIR
}

pub struct Watcher {
    inotify: Inotify,
    search_paths: HashMap<WatchDescriptor, PathBuf>,
}

impl Watcher {
    pub fn new<P: AsRef<Path>>(search_paths: &[P]) -> Result<Self> {
        let inotify = Inotify::init(InitFlags::IN_CLOEXEC)?;

        let mut search_paths_with_descriptor = HashMap::new();

        for search_path in search_paths {
            let search_path = search_path.as_ref();
            let watch_descriptor = inotify.add_watch(search_path, watch_flag())?;

            search_paths_with_descriptor.insert(watch_descriptor, search_path.to_path_buf());
        }

        Ok(Self {
            inotify,
            search_paths: search_paths_with_descriptor,
        })
    }

    pub fn recv(&mut self) -> Result<Vec<Change>> {
        let events = self.inotify.read_events()?;
        let mut changes = Vec::with_capacity(events.len());
        for event in events {
            let watch_desciptor = event.wd;
            let search_path = self
                .search_paths
                .get(&watch_desciptor)
                .expect("an event occurred on a search path that we do not watch")
                .to_path_buf();
            let mask = event.mask;
            let opt_file_name = event.name;

            if mask.contains(AddWatchFlags::IN_CREATE) || mask.contains(AddWatchFlags::IN_MOVED_TO)
            {
                let file_name = opt_file_name.as_ref().unwrap();
                let file_path = search_path.join(file_name);
                if file_path.extension() == Some(OsStr::new("desktop"))
                    && file_path.metadata()?.is_file()
                {
                    let desktop_file_content = std::fs::read_to_string(&file_path)?;
                    let Some((_app_name, _, opt_icon_path)) =
                        parse_desktop_file_content(&desktop_file_content)
                    else {
                        continue;
                    };

                    if opt_icon_path.is_none() {
                        continue;
                    }

                    changes.push(Change::AppInstalled {
                        app_path: file_path,
                    });
                }
            }

            if mask.contains(AddWatchFlags::IN_DELETE)
                || mask.contains(AddWatchFlags::IN_MOVED_FROM)
            {
                let file_name = opt_file_name.unwrap();
                let file_path = search_path.join(file_name);
                if file_path.extension() == Some(OsStr::new("desktop")) {
                    changes.push(Change::AppDeleted {
                        app_path: file_path,
                    });
                }
            }
        }

        Ok(changes)
    }

    pub fn watch<P: AsRef<Path>>(&mut self, search_path: P) -> Result<()> {
        let watch_descriptor = self.inotify.add_watch(search_path.as_ref(), watch_flag())?;
        self.search_paths
            .insert(watch_descriptor, search_path.as_ref().to_path_buf());

        Ok(())
    }

    pub fn unwatch<P: AsRef<Path>>(&mut self, search_path: P) -> Result<()> {
        let search_path = search_path.as_ref();

        let watch_descriptor = *self
            .search_paths
            .iter()
            .find(|(_wd, path)| *path == search_path)
            .unwrap_or_else(|| {
                panic!(
                    "search path [{}] has not been watched",
                    search_path.display()
                )
            })
            .0;

        self.inotify.rm_watch(watch_descriptor)?;
        self.search_paths
            .remove(&watch_descriptor)
            .expect("just checked it is Some");

        Ok(())
    }
}
