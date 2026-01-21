use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Body,
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
}
