#[path = "support/mod.rs"]
mod support;

use anyhow::{Context, anyhow};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::time::Duration;
use voicewin_providers::gemini::{
    GeminiGenerateContentConfig, build_stream_generate_content_request,
    extract_generate_content_sse_text_event,
};
use voicewin_providers::openai_compatible::ChatMessage;
use voicewin_providers::request::{Body, HttpRequest};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url = std::env::var("VOICEWIN_GEMINI_BASE_URL")
        .or_else(|_| std::env::var("GEMINI_BASE_URL"))
        .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta".into());
    let api_key = std::env::var("VOICEWIN_GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_API_KEY"))
        .context("missing VOICEWIN_GEMINI_API_KEY or GOOGLE_API_KEY")?;
    let model =
        std::env::var("VOICEWIN_GEMINI_MODEL").unwrap_or_else(|_| "gemini-3-flash-preview".into());
    let reasoning_effort = std::env::var("VOICEWIN_GEMINI_REASONING_EFFORT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let system_text =
        std::env::var("VOICEWIN_GEMINI_SYSTEM").unwrap_or_else(|_| "Return exactly BANANA.".into());
    let user_text =
        std::env::var("VOICEWIN_GEMINI_TEXT").unwrap_or_else(|_| "hello voicewin".into());
    let max_events = std::env::var("VOICEWIN_GEMINI_MAX_EVENTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(8);
    let attached_image = support::load_optional_image_from_env(
        &[
            "VOICEWIN_GEMINI_IMAGE_DATA_URL",
            "VOICEWIN_LIVE_IMAGE_DATA_URL",
        ],
        &["VOICEWIN_GEMINI_IMAGE_PATH", "VOICEWIN_LIVE_IMAGE_PATH"],
    )?;

    let req = build_stream_generate_content_request(
        &GeminiGenerateContentConfig {
            base_url,
            api_key,
            model,
            reasoning_effort,
        },
        &[
            ChatMessage {
                role: "system".into(),
                content: system_text,
            },
            ChatMessage {
                role: "user".into(),
                content: user_text,
            },
        ],
        attached_image.as_ref().map(|image| &image.artifact),
    );

    eprintln!("request={req:?}");
    if let Some(image) = attached_image.as_ref() {
        eprintln!(
            "image_enabled=true source={} mime_type={} bytes={}",
            image.source, image.mime_type, image.bytes
        );
    } else {
        eprintln!("image_enabled=false");
    }
    if let Body::Json(body) = &req.body {
        let parsed: serde_json::Value =
            serde_json::from_str(body).context("decode request JSON for logging")?;
        eprintln!(
            "request_body={}",
            serde_json::to_string_pretty(&parsed).context("pretty print request JSON")?
        );
    }

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .context("build http client")?;

    let response = send(&client, &req).await?;
    let status = response.status();
    let headers = response.headers().clone();
    eprintln!("status={status}");
    log_header(&headers, "content-type");

    if !status.is_success() {
        let body = response.text().await.context("read error response body")?;
        println!("{body}");
        return Err(anyhow!("gemini probe failed with status {status}"));
    }

    let final_text = consume_sse(response, max_events).await?;
    println!("{final_text}");
    Ok(())
}

async fn send(client: &reqwest::Client, req: &HttpRequest) -> anyhow::Result<reqwest::Response> {
    let mut headers = HeaderMap::new();
    for (name, value) in &req.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .with_context(|| format!("invalid header name: {name}"))?;
        let value = HeaderValue::from_str(value)
            .with_context(|| format!("invalid header value for {name}"))?;
        headers.insert(name, value);
    }

    let builder = match req.method.as_str() {
        "POST" => client.post(&req.url),
        "GET" => client.get(&req.url),
        other => return Err(anyhow!("unsupported probe method: {other}")),
    }
    .headers(headers);

    let builder = match &req.body {
        Body::Empty => builder,
        Body::Json(body) => builder.body(body.clone()),
        Body::MultipartFormData { bytes, .. } => builder.body(bytes.clone()),
    };

    builder
        .send()
        .await
        .context("send live Gemini probe request")
}

async fn consume_sse(response: reqwest::Response, max_events: usize) -> anyhow::Result<String> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut delta_text = String::new();
    let mut event_count = 0usize;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("read SSE chunk")?;
        let chunk_text = std::str::from_utf8(&chunk).context("SSE chunk is not valid utf-8")?;
        buffer.push_str(&chunk_text.replace("\r\n", "\n"));

        while let Some(pos) = buffer.find("\n\n") {
            let event_block = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            let Some(payload) = parse_sse_json_payload(&event_block)? else {
                continue;
            };

            event_count += 1;
            if event_count <= max_events {
                eprintln!(
                    "event[{event_count}]={}",
                    serde_json::to_string(&payload).context("encode SSE event for logging")?
                );
            }

            if let Some(error) = payload.get("error") {
                return Err(anyhow!("SSE error payload: {error}"));
            }

            let text_event = extract_generate_content_sse_text_event(&payload);
            if let Some(delta) = text_event.delta {
                delta_text.push_str(&delta);
            }
            if text_event.done && !delta_text.trim().is_empty() {
                return Ok(delta_text);
            }
        }
    }

    if delta_text.trim().is_empty() {
        return Err(anyhow!("Gemini SSE stream completed without output text"));
    }

    Ok(delta_text)
}

fn log_header(headers: &HeaderMap, name: &str) {
    if let Some(value) = headers.get(name) {
        match value.to_str() {
            Ok(value) => eprintln!("header.{name}={value}"),
            Err(_) => eprintln!("header.{name}=<binary>"),
        }
    }
}

fn parse_sse_json_payload(event_block: &str) -> anyhow::Result<Option<serde_json::Value>> {
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
        return Ok(None);
    }

    let data = data_lines.join("\n");
    let payload =
        serde_json::from_str(&data).with_context(|| format!("decode SSE JSON payload: {data}"))?;
    Ok(Some(payload))
}
