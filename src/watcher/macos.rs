use super::Change;
use anyhow::Result;
use nix::fcntl::open;
use nix::{
    fcntl::OFlag,
    sys::{
        event::{EvFlags, EventFilter, FilterFlag, KEvent, Kqueue},
        stat::Mode,
    },
};
use std::path::{Path, PathBuf};
use std::{
    collections::{HashMap, HashSet},
    fs,
    os::fd::IntoRawFd,
};

fn watch_flag() -> FilterFlag {
    FilterFlag::NOTE_WRITE | FilterFlag::NOTE_DELETE | FilterFlag::NOTE_RENAME
}

pub struct Watcher {
    search_paths: HashMap<i32, PathBuf>,
    kqueue: Kqueue,

    prev_app_list: HashMap<i32, HashSet<PathBuf>>,
}

impl Watcher {
    pub fn new<P: AsRef<Path>>(search_paths: &[P]) -> Result<Self> {
        let kqueue = Kqueue::new()?;

        let mut search_paths_with_fd_info = HashMap::new();
        let mut kevent_to_register = Vec::with_capacity(search_paths.len());
        let mut prev_app_list = HashMap::new();

        for search_path in search_paths {
            let search_path = search_path.as_ref();
            if !search_path.is_dir() {
                return Err(anyhow::anyhow!("search_path is not a directory"));
            }

            let owned_fd = open(search_path, OFlag::O_RDONLY, Mode::empty())?;
            let raw_fd = owned_fd.into_raw_fd();
            search_paths_with_fd_info.insert(raw_fd, search_path.to_path_buf());
            let kevent = KEvent::new(
                raw_fd as usize,
                EventFilter::EVFILT_VNODE,
                EvFlags::EV_ADD | EvFlags::EV_CLEAR,
                watch_flag(),
                0,
                0,
            );

            kevent_to_register.push(kevent);

            let apps = get_current_apps(&search_path)?;
            prev_app_list.insert(raw_fd, apps);
        }
        kqueue.kevent(&kevent_to_register, &mut [], None)?;

        Ok(Self {
            search_paths: search_paths_with_fd_info,
            kqueue,
            prev_app_list,
        })
    }

    pub fn recv(&mut self) -> Result<Vec<Change>> {
        if self.search_paths.is_empty() {
            return Ok(Vec::new());
        }

        let kevent = unsafe { std::mem::MaybeUninit::<KEvent>::zeroed().assume_init() };
        let mut buffer = vec![kevent; self.search_paths.len()];

        let n_events = self.kqueue.kevent(&[], buffer.as_mut(), None)?;

        let mut changes = Vec::with_capacity(n_events);

        for kevent in buffer.iter().take(n_events) {
            let raw_fd = kevent.ident() as i32;
            let fflag = kevent.fflags();
            let search_path_name = self
                .search_paths
                .get(&raw_fd)
                .expect("an event occurred on a search path that we do not watch")
                .clone();

            if fflag.contains(FilterFlag::NOTE_WRITE) {
                let prev_app_list = self
                    .prev_app_list
                    .get(&raw_fd)
                    .expect("an event occurred on a search path that we do not watch");
                let current_app_list = get_current_apps(&search_path_name)?;

                let apps_deleted = prev_app_list.difference(&current_app_list);
                let apps_added = current_app_list.difference(prev_app_list);

                for app_deleted in apps_deleted {
                    changes.push(Change::AppDeleted {
                        app_path: app_deleted.clone(),
                    });
                }

                for app_added in apps_added {
                    changes.push(Change::AppInstalled {
                        app_path: app_added.clone(),
                    });
                }

                *self
                    .prev_app_list
                    .get_mut(&raw_fd)
                    .expect("an event occurred on a search path that do not watch") =
                    current_app_list;
            }
        }

        Ok(changes)
    }

    pub fn unwatch<P: AsRef<Path>>(&mut self, search_path: P) -> Result<()> {
        let search_path = search_path.as_ref();

        let fd = *self
            .search_paths
            .iter()
            .find(|(_fd, path)| *path == search_path)
            .unwrap_or_else(|| {
                panic!(
                    "search path [{}] has not been watched",
                    search_path.display()
                )
            })
            .0;

        self.search_paths
            .remove(&fd)
            .expect("it has just been checked");
        self.prev_app_list.remove(&fd).unwrap_or_else(|| {
            panic!(
                "search path [{}] has not been watched",
                search_path.display()
            )
        });

        let kevent = KEvent::new(
            fd as usize,
            EventFilter::EVFILT_VNODE,
            EvFlags::EV_DELETE,
            FilterFlag::empty(),
            0,
            0,
        );
        self.kqueue.kevent(&[kevent], &mut [], None)?;

        Ok(())
    }

    pub fn watch<P: AsRef<Path>>(&mut self, search_path: P) -> Result<()> {
        let search_path = search_path.as_ref();

        if !search_path.is_dir() {
            return Err(anyhow::anyhow!("search_path is not a directory"));
        }
        let owned_fd = open(search_path, OFlag::O_RDONLY, Mode::empty())?;
        let raw_fd = owned_fd.into_raw_fd();
        self.search_paths.insert(raw_fd, search_path.to_path_buf());
        let kevent = KEvent::new(
            raw_fd as usize,
            EventFilter::EVFILT_VNODE,
            EvFlags::EV_ADD | EvFlags::EV_CLEAR,
            watch_flag(),
            0,
            0,
        );

        let apps = get_current_apps(&search_path)?;
        self.prev_app_list.insert(raw_fd, apps);

        self.kqueue.kevent(&[kevent], &mut [], None)?;

        Ok(())
    }

    pub fn watch_list_is_empty(&self) -> bool {
        self.search_paths.is_empty()
    }
}

fn get_current_apps<P: AsRef<Path> + ?Sized>(path: &P) -> Result<HashSet<PathBuf>> {
    let list = fs::read_dir(path)?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.is_dir() && path.extension()? == "app").then_some(path)
        })
        .collect();

    Ok(list)
}
