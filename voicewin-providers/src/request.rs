use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted_headers: Vec<(String, String)> = self
            .headers
            .iter()
            .map(|(k, v)| {
                let sensitive = k.eq_ignore_ascii_case("authorization")
                    || k.eq_ignore_ascii_case("xi-api-key")
                    || k.to_ascii_lowercase().contains("api-key");
                let v = if sensitive { "[REDACTED]".into() } else { v.clone() };
                (k.clone(), v)
            })
            .collect();

        let body_summary = match &self.body {
            Body::Empty => "Empty".to_string(),
            Body::Json(s) => format!("Json(len={})", s.len()),
            Body::MultipartFormData { boundary, bytes } => {
                format!("MultipartFormData(boundary={}, bytes_len={})", boundary, bytes.len())
            }
        };

        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &redacted_headers)
            .field("body", &body_summary)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Body {
    Empty,
    Json(String),
    MultipartFormData { boundary: String, bytes: Vec<u8> },
}

impl HttpRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_lookup_is_case_insensitive() {
        let req = HttpRequest {
            method: "GET".into(),
            url: "https://example.com".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: Body::Empty,
        };
        assert_eq!(req.header("content-type"), Some("application/json"));
    }

    #[test]
    fn debug_redacts_sensitive_headers() {
        let req = HttpRequest {
            method: "GET".into(),
            url: "https://example.com".into(),
            headers: vec![
                ("Authorization".into(), "Bearer sk-test-123".into()),
                ("xi-api-key".into(), "xi-456".into()),
                ("X-Api-Key".into(), "x-789".into()),
                ("Content-Type".into(), "application/json".into()),
            ],
            body: Body::Empty,
        };

        let s = format!("{req:?}");
        assert!(!s.contains("sk-test-123"));
        assert!(!s.contains("xi-456"));
        assert!(!s.contains("x-789"));
        assert!(!s.contains("Bearer"));
        assert!(s.contains("[REDACTED]"));
    }
}
