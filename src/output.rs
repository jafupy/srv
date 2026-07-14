use std::{
    io,
    net::{IpAddr, SocketAddr},
    path::Path,
    process::Command,
};

use local_ip_address::local_ip;
use qrcode::{QrCode, render::unicode};

pub struct Urls {
    pub local: String,
}

pub fn startup(
    root: &Path,
    address: SocketAddr,
    show_qr: bool,
    live_reload: bool,
    quiet: bool,
) -> Urls {
    let local = url(loopback_for(address.ip()), address.port());
    let network = network_ip(address.ip(), local_ip().ok()).map(|ip| url(ip, address.port()));

    if !quiet {
        println!("Serving {}", root.display());
        println!("  Local:   {local}");
        if let Some(url) = &network {
            println!("  Network: {url}");
        }
        if live_reload {
            println!("  Reload:  watching for changes");
        }
        println!("\nPress Ctrl+C to stop.");

        if show_qr
            && let Some(url) = &network
            && let Ok(code) = QrCode::new(url.as_bytes())
        {
            // Two vertical QR modules per terminal cell using Unicode half blocks.
            let qr = code
                .render::<unicode::Dense1x2>()
                .quiet_zone(true)
                .module_dimensions(1, 1)
                .build();
            println!("\n{qr}");
        }
    }

    Urls { local }
}

pub fn open_browser(url: &str) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(url).spawn()?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "opening a browser is not supported on this platform",
    ))
}

fn loopback_for(bound: IpAddr) -> IpAddr {
    match bound {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        ip => ip,
    }
}

fn network_ip(bound: IpAddr, discovered: Option<IpAddr>) -> Option<IpAddr> {
    if bound.is_loopback() {
        return None;
    }
    if !bound.is_unspecified() {
        return Some(bound);
    }
    discovered.filter(|ip| !ip.is_loopback() && ip.is_ipv4() == bound.is_ipv4())
}

fn url(ip: IpAddr, port: u16) -> String {
    match ip {
        IpAddr::V4(ip) => format!("http://{ip}:{port}"),
        IpAddr::V6(ip) => format!("http://[{ip}]:{port}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_ipv4_and_ipv6_urls() {
        assert_eq!(
            url("192.168.1.2".parse().unwrap(), 3000),
            "http://192.168.1.2:3000"
        );
        assert_eq!(url("::1".parse().unwrap(), 80), "http://[::1]:80");
    }

    #[test]
    fn advertises_only_addresses_reachable_through_the_bind() {
        let lan = "192.168.1.2".parse().unwrap();
        assert_eq!(network_ip("0.0.0.0".parse().unwrap(), Some(lan)), Some(lan));
        assert_eq!(network_ip("127.0.0.1".parse().unwrap(), Some(lan)), None);
        assert_eq!(
            network_ip("192.168.1.9".parse().unwrap(), Some(lan)),
            Some("192.168.1.9".parse().unwrap())
        );
        assert_eq!(network_ip("::".parse().unwrap(), Some(lan)), None);
    }
}
