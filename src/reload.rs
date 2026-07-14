use std::{
    path::{Component, Path},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

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
    let root = root.to_path_buf();
    let last_reload = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
    let callback_root = root.clone();
    let callback_last_reload = Arc::clone(&last_reload);

    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let Ok(event) = event else {
            return;
        };
        if !should_reload(&event, &callback_root) {
            return;
        }

        let now = Instant::now();
        let Ok(mut last_reload) = callback_last_reload.lock() else {
            return;
        };
        if now.duration_since(*last_reload) < Duration::from_millis(75) {
            return;
        }
        *last_reload = now;
        reloader.reload();
    })
    .context("could not start file watcher")?;

    watcher
        .watch(&root, RecursiveMode::Recursive)
        .with_context(|| format!("could not watch {}", root.display()))?;
    Ok(watcher)
}

fn should_reload(event: &Event, root: &Path) -> bool {
    changes_content(event)
        && (event.paths.is_empty() || event.paths.iter().any(|path| !ignored(path, root)))
}

fn changes_content(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    )
}

fn ignored(path: &Path, root: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        matches!(name.to_str(), Some(".git" | "node_modules" | "target"))
    })
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
    fn ignores_generated_dependency_trees() {
        let root = Path::new("site");
        assert!(ignored(Path::new("site/node_modules/pkg/index.js"), root));
        assert!(ignored(Path::new("site/.git/index"), root));
        assert!(!ignored(Path::new("site/src/index.js"), root));
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
