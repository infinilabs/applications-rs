use super::Change;
use crate::platforms::parse_lnk2;
use anyhow::Result;
use notify::event::CreateKind;
use notify::event::RemoveKind;
use notify::windows::ReadDirectoryChangesWatcher;
use notify::Result as NotifyResult;
use notify::{recommended_watcher, Event, EventKind, RecursiveMode, Watcher as WatcherTrait};
use std::ffi::OsStr;
use std::path::Path;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;

pub struct Watcher {
    notify_watcher: ReadDirectoryChangesWatcher,
    rx: Receiver<NotifyResult<Event>>,
}

impl Watcher {
    pub fn new<P: AsRef<Path>>(search_paths: &[P]) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<NotifyResult<Event>>();
        let mut watcher = recommended_watcher(tx)?;
        for search_path in search_paths.iter() {
            let search_path = search_path.as_ref();
            watcher.watch(search_path, RecursiveMode::Recursive)?;
        }

        Ok(Self {
            notify_watcher: watcher,
            rx,
        })
    }

    pub fn recv(&mut self) -> Result<Vec<Change>> {
        let mut changes = Vec::new();

        let event = self.rx.recv()??;
        let event_kind = event.kind;

        if EventKind::Create(CreateKind::File) == event_kind {
            for path in event.paths.iter() {
                if path.extension() == Some(OsStr::new("lnk")) && path.metadata()?.is_file() {
                    if parse_lnk2(path.clone()).is_some() {
                        changes.push(Change::AppInstalled {
                            app_path: path.clone(),
                        });
                    }
                }
            }
        }

        if EventKind::Remove(RemoveKind::File) == event_kind {
            for path in event.paths {
                if path.extension() == Some(OsStr::new("lnk")) {
                    changes.push(Change::AppDeleted { app_path: path });
                }
            }
        }

        Ok(changes)
    }

    pub fn unwatch<P: AsRef<Path>>(&mut self, search_path: P) -> Result<()> {
        self.notify_watcher.unwatch(search_path.as_ref())?;
        Ok(())
    }

    pub fn watch<P: AsRef<Path>>(&mut self, search_path: P) -> Result<()> {
        self.notify_watcher
            .watch(search_path.as_ref(), RecursiveMode::Recursive)?;
        Ok(())
    }
}
