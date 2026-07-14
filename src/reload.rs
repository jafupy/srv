use std::path::Path;

use anyhow::{Context, Result};
use axum::http::{Method, Request, header};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tower_livereload::{Reloader, predicate::Predicate};

#[derive(Clone, Copy)]
pub struct FullHtmlRequest;

impl<T> Predicate<Request<T>> for FullHtmlRequest {
    fn check(&mut self, request: &Request<T>) -> bool {
        request.method() == Method::GET && !request.headers().contains_key(header::RANGE)
    }
}

pub fn watch(root: &Path, reloader: Reloader) -> Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        if event.is_ok_and(|event| changes_content(&event)) {
            reloader.reload();
        }
    })
    .context("could not start file watcher")?;

    watcher
        .watch(root, RecursiveMode::Recursive)
        .with_context(|| format!("could not watch {}", root.display()))?;
    Ok(watcher)
}

fn changes_content(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind};

    #[test]
    fn ignores_access_events_but_reloads_for_changes() {
        assert!(!changes_content(&Event::new(EventKind::Access(
            AccessKind::Open(AccessMode::Any)
        ))));
        assert!(changes_content(&Event::new(EventKind::Create(
            CreateKind::File
        ))));
        assert!(changes_content(&Event::new(EventKind::Modify(
            ModifyKind::Any
        ))));
    }

    #[test]
    fn injection_requires_a_full_get() {
        let mut predicate = FullHtmlRequest;
        assert!(predicate.check(&Request::get("/").body(()).unwrap()));
        assert!(
            !predicate.check(
                &Request::get("/")
                    .header(header::RANGE, "bytes=0-3")
                    .body(())
                    .unwrap()
            )
        );
        assert!(!predicate.check(&Request::head("/").body(()).unwrap()));
    }
}
