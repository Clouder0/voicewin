use crate::request::{Body, HttpRequest};
use crate::sse::SseTextEvent;
use anyhow::{Context, anyhow};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseTextResponse {
    pub text: String,
    pub first_text_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub total_ms: u64,
}

#[derive(Clone, Debug)]
pub struct HttpExecutor {
    client: reqwest::Client,
}

impl HttpExecutor {
    pub fn new() -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .context("build http client")?;
        Ok(Self { client })
    }

    pub async fn execute(&self, req: &HttpRequest) -> anyhow::Result<HttpResponse> {
        let resp = self.send(req).await?;
        let status = resp.status().as_u16();
        let body = resp
            .bytes()
            .await
            .context("failed reading response body")?
            .to_vec();

        Ok(HttpResponse { status, body })
    }

    pub async fn execute_sse_collect_text(&self, req: &HttpRequest) -> anyhow::Result<String> {
        Ok(self.execute_sse_collect_text_metrics(req).await?.text)
    }

    pub async fn execute_sse_collect_text_metrics(
        &self,
        req: &HttpRequest,
    ) -> anyhow::Result<SseTextResponse> {
        self.execute_sse_collect_text_with_metrics(
            req,
            crate::openai_responses::extract_responses_sse_text_event,
        )
        .await
    }

    pub async fn execute_sse_collect_text_with<F>(
        &self,
        req: &HttpRequest,
        extract_text_event: F,
    ) -> anyhow::Result<String>
    where
        F: Fn(&serde_json::Value) -> SseTextEvent,
    {
        Ok(self
            .execute_sse_collect_text_with_metrics(req, extract_text_event)
            .await?
            .text)
    }

    pub async fn execute_sse_collect_text_with_metrics<F>(
        &self,
        req: &HttpRequest,
        extract_text_event: F,
    ) -> anyhow::Result<SseTextResponse>
    where
        F: Fn(&serde_json::Value) -> SseTextEvent,
    {
        let started = Instant::now();
        let resp = self.send(req).await?;
        let status = resp.status().as_u16();
        if !(200..=299).contains(&status) {
            let body = resp
                .bytes()
                .await
                .context("failed reading error response body")?;
            return Err(anyhow!(
                "http sse request failed: status={} body={}",
                status,
                String::from_utf8_lossy(&body)
            ));
        }

        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();
        let mut delta_text = String::new();
        let mut best_full_text = None::<String>;
        let mut first_text_ms = None::<u64>;
        let mut input_tokens = None::<u64>;
        let mut cached_input_tokens = None::<u64>;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed reading sse chunk")?;
            let chunk_text = std::str::from_utf8(&chunk).context("sse chunk is not valid utf-8")?;
            buffer.push_str(&chunk_text.replace("\r\n", "\n"));

            while let Some(pos) = buffer.find("\n\n") {
                let event_block = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                match parse_sse_data_payload(&event_block)? {
                    SseDataPayload::None => {}
                    SseDataPayload::Done => {
                        return finalize_sse_response(
                            started,
                            first_text_ms,
                            input_tokens,
                            cached_input_tokens,
                            delta_text,
                            best_full_text,
                        );
                    }
                    SseDataPayload::Json(payload) => {
                        if let Some(err) = payload.get("error") {
                            return Err(anyhow!("sse error payload: {}", err));
                        }

                        let text_event = extract_text_event(&payload);
                        if first_text_ms.is_none() && has_text_content(&text_event) {
                            first_text_ms =
                                Some(started.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
                        }
                        if let Some(value) = text_event.cached_input_tokens {
                            cached_input_tokens = Some(value);
                        }
                        if let Some(value) = text_event.input_tokens {
                            input_tokens = Some(value);
                        }
                        if let Some(delta) = text_event.delta {
                            delta_text.push_str(&delta);
                        }
                        if let Some(full_text) = text_event.full_text {
                            best_full_text = Some(full_text);
                        }
                        if text_event.done {
                            return finalize_sse_response(
                                started,
                                first_text_ms,
                                input_tokens,
                                cached_input_tokens,
                                delta_text,
                                best_full_text,
                            );
                        }
                    }
                }
            }
        }

        finalize_sse_response(
            started,
            first_text_ms,
            input_tokens,
            cached_input_tokens,
            delta_text,
            best_full_text,
        )
    }

    async fn send(&self, req: &HttpRequest) -> anyhow::Result<reqwest::Response> {
        let mut headers = HeaderMap::new();
        for (k, v) in &req.headers {
            let name = HeaderName::from_bytes(k.as_bytes())
                .with_context(|| format!("invalid header name: {k}"))?;
            let value = HeaderValue::from_str(v)
                .with_context(|| format!("invalid header value for {k}"))?;
            headers.insert(name, value);
        }

        let builder = match req.method.as_str() {
            "GET" => self.client.get(&req.url),
            "POST" => self.client.post(&req.url),
            "PUT" => self.client.put(&req.url),
            "DELETE" => self.client.delete(&req.url),
            other => return Err(anyhow!("unsupported method: {other}")),
        }
        .headers(headers);

        let builder = match &req.body {
            Body::Empty => builder,
            Body::Json(s) => builder.body(s.clone()),
            Body::MultipartFormData { bytes, .. } => builder.body(bytes.clone()),
        };

        builder.send().await.context("http request failed")
    }
}

pub async fn execute(req: &HttpRequest) -> anyhow::Result<HttpResponse> {
    HttpExecutor::new()?.execute(req).await
}

enum SseDataPayload {
    None,
    Done,
    Json(serde_json::Value),
}

fn parse_sse_data_payload(event_block: &str) -> anyhow::Result<SseDataPayload> {
    let mut data_lines = Vec::new();
    for raw_line in event_block.lines() {
        let line = raw_line.trim_end();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start());
        }
    }

    if data_lines.is_empty() {
        return Ok(SseDataPayload::None);
    }

    let data = data_lines.join("\n");
    if data == "[DONE]" {
        return Ok(SseDataPayload::Done);
    }

    let payload =
        serde_json::from_str(&data).with_context(|| format!("decode sse json payload: {data}"))?;
    Ok(SseDataPayload::Json(payload))
}

fn has_text_content(event: &SseTextEvent) -> bool {
    event
        .delta
        .as_ref()
        .is_some_and(|text| !text.trim().is_empty())
        || event
            .full_text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty())
}

fn finalize_sse_response(
    started: Instant,
    first_text_ms: Option<u64>,
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    delta_text: String,
    best_full_text: Option<String>,
) -> anyhow::Result<SseTextResponse> {
    let text = finalize_sse_text(delta_text, best_full_text)?;
    Ok(SseTextResponse {
        text,
        first_text_ms,
        input_tokens,
        cached_input_tokens,
        total_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

fn finalize_sse_text(delta_text: String, best_full_text: Option<String>) -> anyhow::Result<String> {
    if !delta_text.trim().is_empty() {
        return Ok(delta_text);
    }
    if let Some(full_text) = best_full_text.filter(|text| !text.trim().is_empty()) {
        return Ok(full_text);
    }

    Err(anyhow!("sse stream completed without output text"))
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
                ResponseTemplate::new(201).set_body_raw("{\"ok\":true}", "application/json"),
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
        let mut expected_body =
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"\r\n\r\n")
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

        let err = execute(&req)
            .await
            .expect_err("invalid header name should fail");
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

    #[tokio::test]
    async fn execute_sse_collect_text_aggregates_deltas_until_done() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let text = HttpExecutor::new()
            .unwrap()
            .execute_sse_collect_text(&HttpRequest {
                method: "POST".into(),
                url: format!("{}/responses", server.uri()),
                headers: vec![("Accept".into(), "text/event-stream".into())],
                body: Body::Json("{}".into()),
            })
            .await
            .expect("sse request should succeed");

        assert_eq!(text, "Hello");
    }

    #[tokio::test]
    async fn execute_sse_collect_text_uses_full_text_fallback_when_no_deltas_arrive() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"Recovered\"}]}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"Recovered\"}]}]}}\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let text = HttpExecutor::new()
            .unwrap()
            .execute_sse_collect_text(&HttpRequest {
                method: "POST".into(),
                url: format!("{}/responses", server.uri()),
                headers: vec![("Accept".into(), "text/event-stream".into())],
                body: Body::Json("{}".into()),
            })
            .await
            .expect("sse request should succeed");

        assert_eq!(text, "Recovered");
    }

    #[tokio::test]
    async fn execute_sse_collect_text_metrics_reports_first_text_and_total_latency() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_millis(5))
                    .set_body_raw(sse, "text/event-stream"),
            )
            .mount(&server)
            .await;

        let response = HttpExecutor::new()
            .unwrap()
            .execute_sse_collect_text_metrics(&HttpRequest {
                method: "POST".into(),
                url: format!("{}/responses", server.uri()),
                headers: vec![("Accept".into(), "text/event-stream".into())],
                body: Body::Json("{}".into()),
            })
            .await
            .expect("sse metrics request should succeed");

        assert_eq!(response.text, "Hello");
        assert!(response.first_text_ms.is_some());
        assert!(response.total_ms >= response.first_text_ms.unwrap_or_default());
        assert_eq!(response.input_tokens, None);
        assert_eq!(response.cached_input_tokens, None);
    }

    #[tokio::test]
    async fn execute_sse_collect_text_metrics_captures_input_and_cached_tokens() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":2048,\"input_tokens_details\":{\"cached_tokens\":512}},\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello\"}]}]}}\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let response = HttpExecutor::new()
            .unwrap()
            .execute_sse_collect_text_metrics(&HttpRequest {
                method: "POST".into(),
                url: format!("{}/responses", server.uri()),
                headers: vec![("Accept".into(), "text/event-stream".into())],
                body: Body::Json("{}".into()),
            })
            .await
            .expect("sse metrics request should succeed");

        assert_eq!(response.text, "Hello");
        assert_eq!(response.input_tokens, Some(2048));
        assert_eq!(response.cached_input_tokens, Some(512));
    }

    #[test]
    fn parse_sse_data_payload_handles_done_sentinel() {
        let payload = parse_sse_data_payload("data: [DONE]").expect("should parse");
        assert!(matches!(payload, SseDataPayload::Done));
    }

    #[test]
    fn parse_sse_data_payload_joins_multiline_data() {
        let payload =
            parse_sse_data_payload("event: message\ndata: [1,\ndata: 2]").expect("should parse");
        match payload {
            SseDataPayload::Json(value) => {
                assert_eq!(value, serde_json::json!([1, 2]));
            }
            _ => panic!("expected json payload"),
        }
    }
}
