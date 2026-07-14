use std::{str::FromStr, sync::Arc, time::Instant};

use anyhow::{Context, Result, bail};
use axum::{
    body::Body,
    extract::State,
    http::{
        HeaderName, HeaderValue, Method, Request, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
            ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL,
        },
    },
    middleware::Next,
    response::{IntoResponse, Response},
};

#[derive(Clone)]
pub struct Policy {
    headers: Arc<Vec<(HeaderName, HeaderValue)>>,
    cors: bool,
    cache: HeaderValue,
    verbose: bool,
}

impl Policy {
    pub fn new(
        raw_headers: &[String],
        cors: bool,
        cache_seconds: u64,
        immutable: bool,
        verbose: bool,
    ) -> Result<Self> {
        if immutable && cache_seconds == 0 {
            bail!("--immutable requires --cache to be greater than zero");
        }

        let headers = raw_headers
            .iter()
            .map(String::as_str)
            .map(parse_header)
            .collect::<Result<Vec<_>>>()?;
        let cache = cache_header(cache_seconds, immutable);

        Ok(Self {
            headers: Arc::new(headers),
            cors,
            cache,
            verbose,
        })
    }
}

pub async fn apply(
    State(policy): State<Policy>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let started = Instant::now();
    let method = request.method().clone();
    let uri = request.uri().clone();

    let mut response = if policy.cors && method == Method::OPTIONS {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(request).await
    };

    if (method == Method::GET || method == Method::HEAD)
        && response.status().is_success()
        && !response.headers().contains_key(CACHE_CONTROL)
    {
        response
            .headers_mut()
            .insert(CACHE_CONTROL, policy.cache.clone());
    }

    if policy.cors {
        response
            .headers_mut()
            .insert(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, HEAD, OPTIONS"),
        );
        response
            .headers_mut()
            .insert(ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("*"));
    }

    for (name, value) in policy.headers.iter() {
        response.headers_mut().insert(name.clone(), value.clone());
    }

    if policy.verbose {
        eprintln!(
            "{method} {uri} -> {} ({} ms)",
            response.status().as_u16(),
            started.elapsed().as_millis()
        );
    }

    response
}

fn parse_header(raw: &str) -> Result<(HeaderName, HeaderValue)> {
    let (name, value) = raw
        .split_once(':')
        .with_context(|| format!("invalid header {raw:?}; expected NAME:VALUE"))?;
    let name = HeaderName::from_str(name.trim())
        .with_context(|| format!("invalid header name in {raw:?}"))?;
    let value = HeaderValue::from_str(value.trim())
        .with_context(|| format!("invalid header value in {raw:?}"))?;
    Ok((name, value))
}

fn cache_header(seconds: u64, immutable: bool) -> HeaderValue {
    if seconds == 0 {
        return HeaderValue::from_static("no-cache");
    }
    let suffix = if immutable { ", immutable" } else { "" };
    HeaderValue::from_str(&format!("public, max-age={seconds}{suffix}"))
        .expect("cache-control value is always valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_custom_headers() {
        let (name, value) = parse_header("X-Test: yes").unwrap();
        assert_eq!(name.as_str(), "x-test");
        assert_eq!(value.to_str().unwrap(), "yes");
        assert!(parse_header("broken").is_err());
    }

    #[test]
    fn builds_cache_headers() {
        assert_eq!(cache_header(0, false).to_str().unwrap(), "no-cache");
        assert_eq!(
            cache_header(60, false).to_str().unwrap(),
            "public, max-age=60"
        );
        assert_eq!(
            cache_header(60, true).to_str().unwrap(),
            "public, max-age=60, immutable"
        );
    }
}
