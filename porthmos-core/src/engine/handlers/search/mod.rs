use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use porthmos_vfs::{SearchEvent, SearchQuery};

use super::super::{Engine, Event, Location};

pub(crate) const SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);

fn event_for(search_event: SearchEvent) -> Event {
    match search_event {
        SearchEvent::Found(entry) => Event::SearchFound(entry),
        SearchEvent::Done { truncated } => Event::SearchDone { truncated },
        SearchEvent::Failed(message) => Event::SearchFailed(message),
    }
}

impl Engine {
    pub(crate) fn search(&mut self, location: Location, root: PathBuf, pattern: String) {
        self.search.cancel.store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        self.search.cancel = cancel.clone();
        let generation = self.search.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let current_generation = self.search.generation.clone();

        let Some(fs) = self.fs_for(location) else {
            return;
        };
        let events = self.events.clone();
        tokio::spawn(async move {
            tokio::time::sleep(SEARCH_DEBOUNCE).await;
            if cancel.load(Ordering::Relaxed) || current_generation.load(Ordering::Relaxed) != generation {
                return;
            }
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let forward = async {
                while let Some(search_event) = rx.recv().await {
                    let _ = events.send(event_for(search_event));
                }
            };
            tokio::join!(fs.search(SearchQuery::new(root, pattern), tx, cancel), forward);
        });
    }

    pub(crate) fn cancel_search(&mut self) {
        self.search.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
