use std::{net::IpAddr, path::PathBuf};

use clap::{ArgAction, Parser};

/// Fast native static file server with instant LAN sharing.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Args {
    /// Directory to serve.
    #[arg(default_value = ".")]
    pub directory: PathBuf,

    /// Interface to bind to. The default exposes the server on the LAN.
    #[arg(short = 'l', long, default_value = "0.0.0.0")]
    pub host: IpAddr,

    /// Port to listen on; use 0 to select a free port.
    #[arg(short, long, default_value_t = 3000, env = "PORT")]
    pub port: u16,

    /// Fall back to index.html for single-page applications.
    #[arg(short, long)]
    pub single: bool,

    /// Disable extensionless .html resolution.
    #[arg(long)]
    pub no_clean_urls: bool,

    /// Disable directory listings when no index.html exists.
    #[arg(long)]
    pub no_listing: bool,

    /// Serve dotfiles and other hidden path components.
    #[arg(long)]
    pub hidden: bool,

    /// Relative file to return for 404 responses.
    #[arg(long, value_name = "FILE")]
    pub not_found: Option<PathBuf>,

    /// Open the local URL in the default browser.
    #[arg(long)]
    pub open: bool,

    /// Add permissive CORS headers and answer preflight requests.
    #[arg(long)]
    pub cors: bool,

    /// Add a response header. Repeat for multiple headers.
    #[arg(long = "header", value_name = "NAME:VALUE", action = ArgAction::Append)]
    pub headers: Vec<String>,

    /// Browser cache lifetime in seconds. Zero sends Cache-Control: no-cache.
    #[arg(long, value_name = "SECONDS", default_value_t = 0)]
    pub cache: u64,

    /// Mark cached responses immutable. Requires a non-zero cache lifetime.
    #[arg(long)]
    pub immutable: bool,

    /// Fail instead of advancing to the next port when the requested port is busy.
    #[arg(long)]
    pub no_port_switching: bool,

    /// Print one line per request.
    #[arg(long, conflicts_with = "quiet")]
    pub verbose: bool,

    /// Suppress startup output and the QR code.
    #[arg(short, long)]
    pub quiet: bool,

    /// Do not print the QR code.
    #[arg(long)]
    pub no_qr: bool,

    /// Disable file watching and automatic browser refresh.
    #[arg(long)]
    pub no_reload: bool,
}
