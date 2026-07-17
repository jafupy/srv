use std::{net::IpAddr, path::PathBuf};

use clap::Parser;

/// Fast static file server with a LAN QR code.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Args {
    /// Directory to serve.
    #[arg(default_value = ".")]
    pub directory: PathBuf,

    /// Interface to bind to.
    #[arg(short = 'l', long, default_value = "0.0.0.0")]
    pub host: IpAddr,

    /// Port to listen on; use 0 to select a free port.
    #[arg(short, long, default_value_t = 3000)]
    pub port: u16,

    /// Fall back to index.html for single-page applications.
    #[arg(short, long)]
    pub single: bool,

    /// Do not print the QR code.
    #[arg(long)]
    pub no_qr: bool,

    /// Disable the injected file explorer.
    #[arg(long)]
    pub no_ls: bool,

    /// Disable file watching and automatic browser refresh.
    #[arg(long)]
    pub no_reload: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_no_ls() {
        let args = Args::try_parse_from(["srv", "--no-ls"]).unwrap();
        assert!(args.no_ls);
    }
}
