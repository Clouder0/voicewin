use crate::request::{Body, HttpRequest};
use anyhow::{Context, anyhow};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

pub async fn execute(req: &HttpRequest) -> anyhow::Result<HttpResponse> {
    // Important: without an explicit timeout, a broken endpoint can hang the
    // session indefinitely (especially during enhancement).
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .context("build http client")?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn execute_posts_json_and_returns_status_and_body() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/json"))
            .and(header("content-type", "application/json"))
            .and(body_string_contains("hello"))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_raw("{\"ok\":true}", "application/json"),
            )
            .mount(&server)
            .await;

        let response = execute(&HttpRequest {
            method: "POST".into(),
            url: format!("{}/json", server.uri()),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: Body::Json("{\"message\":\"hello\"}".into()),
        })
        .await
        .expect("json request should succeed");

        assert_eq!(response.status, 201);
        assert_eq!(String::from_utf8_lossy(&response.body), "{\"ok\":true}");
    }

    #[tokio::test]
    async fn execute_posts_multipart_and_preserves_content_type_and_bytes() {
        let server = MockServer::start().await;
        let boundary = "Boundary-123";
        let mut expected_body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"\r\n\r\n"
        )
        .into_bytes();
        expected_body.extend_from_slice(&[0, 255, 10, b'X']);
        expected_body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        Mock::given(method("POST"))
            .and(path("/multipart"))
            .and(header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}").as_str(),
            ))
            .respond_with(ResponseTemplate::new(202).set_body_string("accepted"))
            .mount(&server)
            .await;

        let response = execute(&HttpRequest {
            method: "POST".into(),
            url: format!("{}/multipart", server.uri()),
            headers: vec![(
                "Content-Type".into(),
                format!("multipart/form-data; boundary={boundary}"),
            )],
            body: Body::MultipartFormData {
                boundary: boundary.into(),
                bytes: expected_body.clone(),
            },
        })
        .await
        .expect("multipart request should succeed");

        let requests = server
            .received_requests()
            .await
            .expect("request recording should be enabled");

        assert_eq!(response.status, 202);
        assert_eq!(String::from_utf8_lossy(&response.body), "accepted");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].body, expected_body);
    }

    #[tokio::test]
    async fn execute_rejects_invalid_header_name_or_value_with_context() {
        let req = HttpRequest {
            method: "POST".into(),
            url: "http://127.0.0.1:9/invalid".into(),
            headers: vec![("bad header".into(), "value".into())],
            body: Body::Empty,
        };

        let err = execute(&req).await.expect_err("invalid header name should fail");
        assert!(err.to_string().contains("invalid header name"));

        let req = HttpRequest {
            method: "POST".into(),
            url: "http://127.0.0.1:9/invalid".into(),
            headers: vec![("x-test".into(), "line\nbreak".into())],
            body: Body::Empty,
        };

        let err = execute(&req)
            .await
            .expect_err("invalid header value should fail");
        assert!(err.to_string().contains("invalid header value for x-test"));
    }

    #[tokio::test]
    async fn execute_rejects_unsupported_method() {
        let err = execute(&HttpRequest {
            method: "PATCH".into(),
            url: "http://127.0.0.1:9/patch".into(),
            headers: Vec::new(),
            body: Body::Empty,
        })
        .await
        .expect_err("unsupported method should fail");

        assert!(err.to_string().contains("unsupported method: PATCH"));
    }
}
