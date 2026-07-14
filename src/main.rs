mod backend;
mod cli;
mod directory;
mod output;
mod policy;
mod reload;

use std::{
    io,
    net::SocketAddr,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use axum::{Router, middleware, routing::get};
use backend::CapabilityBackend;
use clap::Parser;
use cli::Args;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use tower_livereload::LiveReloadLayer;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    validate_relative_option(args.not_found.as_deref(), "--not-found")?;

    let root = canonical_root(&args.directory)?;
    let address = SocketAddr::new(args.host, args.port);
    let listener = bind_available(address, !args.no_port_switching)
        .await
        .with_context(|| format!("could not listen on {address}"))?;
    let address = listener.local_addr()?;

    let backend = CapabilityBackend::new(&root, args.hidden)?;
    let files = ServeDir::with_backend("", backend.clone());
    let fallback_options = directory::Options {
        single: args.single,
        listing: !args.no_listing,
        clean_urls: !args.no_clean_urls,
        not_found: args.not_found.clone(),
    };
    let fallback =
        get(move |uri| directory::fallback(backend.clone(), uri, fallback_options.clone()));
    let app = Router::new().fallback_service(files.fallback(fallback));

    // Keep the native watcher alive for as long as the server is running.
    let (app, _watcher) = if args.no_reload {
        (app, None)
    } else {
        let layer = LiveReloadLayer::new()
            .request_predicate::<axum::body::Body, _>(reload::FullHtmlRequest);
        let watcher = reload::watch(&root, layer.reloader())?;
        (app.layer(layer), Some(watcher))
    };

    let response_policy = policy::Policy::new(
        &args.headers,
        args.cors,
        args.cache,
        args.immutable,
        args.verbose && !args.quiet,
    )?;
    let app = app.layer(middleware::from_fn_with_state(
        response_policy,
        policy::apply,
    ));

    let urls = output::startup(
        &root,
        address,
        !args.no_qr && !args.quiet,
        !args.no_reload,
        args.quiet,
    );
    if args.open
        && let Err(error) = output::open_browser(&urls.local)
        && !args.quiet
    {
        eprintln!("Could not open {}: {error}", urls.local);
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server failed")
}

async fn bind_available(mut address: SocketAddr, switch_ports: bool) -> io::Result<TcpListener> {
    loop {
        match TcpListener::bind(address).await {
            Ok(listener) => return Ok(listener),
            Err(error) if switch_ports && error.kind() == io::ErrorKind::AddrInUse => {
                let Some(port) = address.port().checked_add(1) else {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "no free port available",
                    ));
                };
                address.set_port(port);
            }
            Err(error) => return Err(error),
        }
    }
}

fn canonical_root(directory: &Path) -> Result<PathBuf> {
    let root = directory
        .canonicalize()
        .with_context(|| format!("cannot serve {}", directory.display()))?;
    if !root.is_dir() {
        bail!("{} is not a directory", root.display());
    }
    Ok(root)
}

fn validate_relative_option(path: Option<&Path>, option: &str) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        bail!("{option} must be a relative path inside the served directory");
    }
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn increments_when_the_requested_port_is_busy() {
        let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = occupied.local_addr().unwrap();
        if address.port() == u16::MAX {
            return;
        }

        let listener = bind_available(address, true).await.unwrap();
        assert!(listener.local_addr().unwrap().port() > address.port());
    }

    #[tokio::test]
    async fn can_disable_port_switching() {
        let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = occupied.local_addr().unwrap();
        let error = bind_available(address, false).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    }

    #[test]
    fn custom_error_page_must_stay_inside_root() {
        assert!(validate_relative_option(Some(Path::new("404.html")), "--not-found").is_ok());
        assert!(validate_relative_option(Some(Path::new("../404.html")), "--not-found").is_err());
    }
}
