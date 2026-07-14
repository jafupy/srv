use std::path::{Component, PathBuf};

use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

use crate::backend::CapabilityBackend;

pub async fn fallback(backend: CapabilityBackend, uri: Uri, single: bool) -> Response {
    if let Some(directory) = relative_path(uri.path())
        && let Ok(response) = listing(&backend, directory, uri.path()).await
    {
        return response;
    }

    if single && let Ok(index) = backend.read(PathBuf::from("index.html")).await {
        return ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], index).into_response();
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
    items.sort_unstable_by_key(|item| item.0.to_lowercase());

    let base = if request_path.ends_with('/') {
        request_path.to_owned()
    } else {
        format!("{request_path}/")
    };
    let base = escape(&base);
    let mut html = format!(
        "<!doctype html><meta charset=utf-8><title>Index of {}</title>\
         <h1>Index of {}</h1><ul>",
        escape(request_path),
        escape(request_path)
    );
    if request_path != "/" {
        html.push_str("<li><a href=\"../\">../</a></li>");
    }
    for (name, is_dir) in items {
        let suffix = if is_dir { "/" } else { "" };
        let href = utf8_percent_encode(&name, NON_ALPHANUMERIC);
        html.push_str(&format!(
            "<li><a href=\"{base}{href}{suffix}\">{}{suffix}</a></li>",
            escape(&name)
        ));
    }
    html.push_str("</ul>");

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(html))
        .expect("static response headers are valid"))
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
}
