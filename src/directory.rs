use std::path::{Component, PathBuf};

use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

use crate::backend::CapabilityBackend;

#[derive(Debug, Clone)]
pub struct Options {
    pub single: bool,
    pub listing: bool,
    pub clean_urls: bool,
    pub not_found: Option<PathBuf>,
}

pub async fn fallback(backend: CapabilityBackend, uri: Uri, options: Options) -> Response {
    let Some(path) = relative_path(uri.path()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    if options.listing
        && let Ok(response) = listing(&backend, path.clone(), uri.path()).await
    {
        return response;
    }

    if options.clean_urls
        && path.extension().is_none()
        && let Ok(html) = backend.read(path.with_extension("html")).await
    {
        return html_response(StatusCode::OK, html);
    }

    if options.single
        && let Ok(index) = backend.read(PathBuf::from("index.html")).await
    {
        return html_response(StatusCode::OK, index);
    }

    if let Some(not_found) = options.not_found
        && let Ok(body) = backend.read(not_found).await
    {
        return html_response(StatusCode::NOT_FOUND, body);
    }

    StatusCode::NOT_FOUND.into_response()
}

fn relative_path(request_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode_str(request_path).decode_utf8().ok()?;
    let relative = PathBuf::from(decoded.trim_start_matches('/'));
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    Some(relative)
}

async fn listing(
    backend: &CapabilityBackend,
    directory: PathBuf,
    request_path: &str,
) -> std::io::Result<Response> {
    let mut items = backend.entries(directory).await?;
    items.sort_unstable_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    let base = if request_path.ends_with('/') {
        request_path.to_owned()
    } else {
        format!("{request_path}/")
    };
    let base = escape(&base);
    let title = format!("Index of {}", escape(request_path));

    let mut html = String::from("<!doctype html><html><head><meta charset=utf-8>");
    html.push_str("<meta name=viewport content=\"width=device-width,initial-scale=1\">");
    html.push_str(&format!("<title>{title}</title>"));
    html.push_str(
        "<style>\
         :root{color-scheme:light dark;font-family:ui-sans-serif,system-ui,sans-serif}\
         body{max-width:860px;margin:64px auto;padding:0 24px}\
         h1{font-size:1.35rem;margin:0 0 20px}\
         ul{list-style:none;padding:0;margin:0;border:1px solid color-mix(in srgb,currentColor 18%,transparent);border-radius:12px;overflow:hidden}\
         li+li{border-top:1px solid color-mix(in srgb,currentColor 12%,transparent)}\
         a{display:flex;gap:16px;align-items:center;padding:11px 14px;color:inherit;text-decoration:none}\
         a:hover{background:color-mix(in srgb,currentColor 7%,transparent)}\
         .name{flex:1;overflow-wrap:anywhere}.size{opacity:.58;font-variant-numeric:tabular-nums}\
         .dir .name{font-weight:650}</style></head><body>",
    );
    html.push_str(&format!("<h1>{title}</h1><ul>"));

    if request_path != "/" {
        html.push_str("<li class=dir><a href=\"../\"><span class=name>../</span></a></li>");
    }

    for item in items {
        let suffix = if item.is_dir { "/" } else { "" };
        let class = if item.is_dir { " class=dir" } else { "" };
        let href = utf8_percent_encode(&item.name, NON_ALPHANUMERIC);
        let size = if item.is_dir {
            String::new()
        } else {
            format!("<span class=size>{}</span>", format_size(item.len))
        };
        html.push_str(&format!(
            "<li{class}><a href=\"{base}{href}{suffix}\"><span class=name>{}{suffix}</span>{size}</a></li>",
            escape(&item.name)
        ));
    }
    html.push_str("</ul></body></html>");

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(html))
        .expect("static response headers are valid"))
}

fn html_response(status: StatusCode, body: Vec<u8>) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_directory_names() {
        assert_eq!(escape("<x & \"y\">"), "&lt;x &amp; &quot;y&quot;&gt;");
        assert_eq!(escape("/a&copy;/"), "/a&amp;copy;/");
    }

    #[test]
    fn rejects_parent_traversal() {
        assert!(relative_path("/../secret").is_none());
        assert!(relative_path("/%2e%2e/secret").is_none());
    }

    #[test]
    fn formats_file_sizes() {
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1_500), "1.5 KB");
        assert_eq!(format_size(15_000), "15 KB");
    }
}
