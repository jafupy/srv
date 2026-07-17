mod backend;
mod cli;
mod directory;
mod output;
mod reload;
mod toolbar;

use std::{
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
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
    let root = canonical_root(&args.directory)?;
    let address = SocketAddr::new(args.host, args.port);
    let listener = bind_available(address)
        .await
        .with_context(|| format!("could not listen on {address}"))?;
    let address = listener.local_addr()?;

    let backend = CapabilityBackend::new(&root)?;
    let files = ServeDir::with_backend("", backend.clone());
    let single = args.single;
    let fallback_backend = backend.clone();
    let fallback = get(move |uri| directory::fallback(fallback_backend.clone(), uri, single));
    let app = Router::new().fallback_service(files.fallback(fallback));
    let app = if args.no_ls {
        app
    } else {
        app.layer(middleware::from_fn_with_state(
            (backend, single),
            toolbar::inject,
        ))
    };

    // Keep the native watcher alive for as long as the server is running.
    let (app, _watcher) = if args.no_reload {
        (app, None)
    } else {
        let layer = LiveReloadLayer::new()
            .request_predicate::<axum::body::Body, _>(reload::FullHtmlRequest);
        let watcher = reload::watch(&root, layer.reloader())?;
        (app.layer(layer), Some(watcher))
    };

    output::startup(&root, address, !args.no_qr, !args.no_reload);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server failed")
}

async fn bind_available(mut address: SocketAddr) -> io::Result<TcpListener> {
    loop {
        match TcpListener::bind(address).await {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
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

        let listener = bind_available(address).await.unwrap();
        assert!(listener.local_addr().unwrap().port() > address.port());
    }
}
