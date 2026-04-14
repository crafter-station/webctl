use serde::{Deserialize, Serialize};

use crate::OperationKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpSurface {
    pub endpoints: Vec<HttpEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpEndpoint {
    pub namespace: Vec<String>,
    pub method: HttpMethod,
    pub path: String,
    pub description: String,
    pub operation_kind: OperationKind,
    #[serde(default)]
    pub sample_request_content_type: Option<String>,
    #[serde(default)]
    pub sample_response_content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

pub fn derive_operation_kind(method: &HttpMethod) -> OperationKind {
    match method {
        HttpMethod::Get | HttpMethod::Head | HttpMethod::Options => OperationKind::Read,
        HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch | HttpMethod::Delete => {
            OperationKind::Write
        }
    }
}

pub fn normalize_command_path(namespace: &[String]) -> Vec<String> {
    namespace.iter().map(|s| camel_to_kebab(s)).collect()
}

fn camel_to_kebab(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('-');
        }
        result.push(c.to_lowercase().next().unwrap_or(c));
    }
    result
}
