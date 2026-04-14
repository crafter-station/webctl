use std::collections::BTreeMap;

use webctl_ir::{HttpEndpoint, HttpMethod};
use webctl_probe::har::{HarEntry, HarLog};

pub fn infer_endpoints(har: &HarLog) -> Vec<HttpEndpoint> {
    let mut endpoints = BTreeMap::<(String, String), HttpEndpoint>::new();

    for entry in &har.log.entries {
        if is_static_asset(entry) {
            continue;
        }

        let Some(method) = parse_method(&entry.request.method) else {
            continue;
        };

        if method == HttpMethod::Options {
            continue;
        }

        let normalized_path = normalize_endpoint_path(&entry.request.url);
        let key = (entry.request.method.to_ascii_uppercase(), normalized_path.clone());

        endpoints.entry(key).or_insert_with(|| HttpEndpoint {
            namespace: namespace_from_path(&normalized_path),
            method: method.clone(),
            path: normalized_path.clone(),
            description: description_from_path(&normalized_path),
            operation_kind: webctl_ir::derive_operation_kind(&method),
            sample_request_content_type: request_content_type(entry),
            sample_response_content_type: response_content_type(entry),
        });
    }

    endpoints.into_values().collect()
}

fn parse_method(method: &str) -> Option<HttpMethod> {
    match method.to_ascii_uppercase().as_str() {
        "GET" => Some(HttpMethod::Get),
        "POST" => Some(HttpMethod::Post),
        "PUT" => Some(HttpMethod::Put),
        "PATCH" => Some(HttpMethod::Patch),
        "DELETE" => Some(HttpMethod::Delete),
        "HEAD" => Some(HttpMethod::Head),
        "OPTIONS" => Some(HttpMethod::Options),
        _ => None,
    }
}

fn normalize_endpoint_path(raw_url: &str) -> String {
    let Ok(url) = url::Url::parse(raw_url) else {
        return raw_url.split('#').next().unwrap_or(raw_url).to_string();
    };
    let mut path = if url.path().is_empty() {
        "/".to_string()
    } else {
        url.path().to_string()
    };
    let query = url
        .query_pairs()
        .map(|(key, _)| format!("{key}={{{key}}}"))
        .collect::<Vec<_>>();

    if !query.is_empty() {
        path.push('?');
        path.push_str(&query.join("&"));
    }

    path
}

fn namespace_from_path(path: &str) -> Vec<String> {
    let path_only = path.split('?').next().unwrap_or(path);
    let mut parts = path_only
        .split('/')
        .filter(|part| !part.is_empty())
        .map(segment_to_token)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        parts.push("root".to_string());
    }

    parts.truncate(2);
    parts
}

fn description_from_path(path: &str) -> String {
    let path_only = path.split('?').next().unwrap_or(path);
    let parts = path_only
        .split('/')
        .filter(|part| !part.is_empty())
        .map(segment_to_phrase)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        "Root endpoint".to_string()
    } else {
        parts.join(" ")
    }
}

fn segment_to_token(segment: &str) -> String {
    segment
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("-")
}

fn segment_to_phrase(segment: &str) -> String {
    segment
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

const ASSET_RESOURCE_TYPES: &[&str] = &[
    "Font", "Image", "Stylesheet", "Media", "Script", "Manifest", "Ping", "Other",
];

const ASSET_EXTENSIONS: &[&str] = &[
    ".js", ".css", ".png", ".jpg", ".jpeg", ".gif", ".svg", ".ico", ".woff", ".woff2",
    ".ttf", ".otf", ".eot", ".mp3", ".mp4", ".webm", ".webp", ".avif", ".map",
];

fn is_static_asset(entry: &HarEntry) -> bool {
    if let Some(ref rt) = entry.resource_type {
        if ASSET_RESOURCE_TYPES.iter().any(|t| t.eq_ignore_ascii_case(rt)) {
            return true;
        }
    }

    let path = entry.request.url.split('?').next().unwrap_or(&entry.request.url);
    if ASSET_EXTENSIONS.iter().any(|ext| path.ends_with(ext)) {
        return true;
    }

    if let Some(ref mime) = entry.response.content.mime_type {
        let m = mime.to_lowercase();
        if m.starts_with("image/")
            || m.starts_with("font/")
            || m.starts_with("audio/")
            || m.starts_with("video/")
            || m == "application/javascript"
            || m == "text/javascript"
            || m == "text/css"
            || m == "application/x-javascript"
        {
            return true;
        }
    }

    false
}

fn request_content_type(entry: &HarEntry) -> Option<String> {
    entry.request
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.clone())
        .or_else(|| entry.request.post_data.as_ref().map(|post| post.mime_type.clone()))
}

fn response_content_type(entry: &HarEntry) -> Option<String> {
    entry.response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.clone())
        .or_else(|| entry.response.content.mime_type.clone())
}
