use crate::request::{Body, HttpRequest};
use anyhow::{Context, anyhow};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

pub async fn execute(req: &HttpRequest) -> anyhow::Result<HttpResponse> {
    let client = reqwest::Client::new();

    let mut headers = HeaderMap::new();
    for (k, v) in &req.headers {
        let name = HeaderName::from_bytes(k.as_bytes())
            .with_context(|| format!("invalid header name: {k}"))?;
        let value =
            HeaderValue::from_str(v).with_context(|| format!("invalid header value for {k}"))?;
        headers.insert(name, value);
    }

    let builder = match req.method.as_str() {
        "GET" => client.get(&req.url),
        "POST" => client.post(&req.url),
        "PUT" => client.put(&req.url),
        "DELETE" => client.delete(&req.url),
        other => return Err(anyhow!("unsupported method: {other}")),
    }
    .headers(headers);

    let builder = match &req.body {
        Body::Empty => builder,
        Body::Json(s) => builder.body(s.clone()),
        Body::MultipartFormData { bytes, .. } => builder.body(bytes.clone()),
    };

    let resp = builder.send().await.context("http request failed")?;
    let status = resp.status().as_u16();
    let body = resp
        .bytes()
        .await
        .context("failed reading response body")?
        .to_vec();

    Ok(HttpResponse { status, body })
}
