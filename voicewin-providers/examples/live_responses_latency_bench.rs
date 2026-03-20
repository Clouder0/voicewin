#[path = "support/mod.rs"]
mod support;

use std::time::Instant;

use anyhow::Context;
use serde::Serialize;
use voicewin_providers::openai_compatible::{ChatMessage, build_list_models_request};
use voicewin_providers::openai_responses::{OpenAiResponsesConfig, build_responses_sse_request};
use voicewin_providers::runtime::HttpExecutor;

#[derive(Debug, Serialize)]
struct ResponseTiming {
    text: String,
    total_ms: u128,
    first_token_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct WarmedTiming {
    text: String,
    warmup_ms: u128,
    total_ms: u128,
    warmup_status: u16,
    first_token_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct Summary {
    name: String,
    runs: usize,
    total_median_ms: u128,
    total_min_ms: u128,
    total_max_ms: u128,
    warmup_median_ms: Option<u128>,
    first_token_median_ms: Option<u128>,
    sample_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct BenchmarkResult {
    base_url: String,
    model: String,
    reasoning_effort: Option<String>,
    image_enabled: bool,
    image_source: Option<String>,
    image_mime_type: Option<String>,
    image_bytes: Option<usize>,
    rounds: usize,
    warmup_delay_ms: u64,
    cold: Vec<ResponseTiming>,
    warmed: Vec<WarmedTiming>,
    persistent: Vec<ResponseTiming>,
    summaries: Vec<Summary>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url = std::env::var("VOICEWIN_LIVE_BASE_URL")
        .or_else(|_| std::env::var("LLM_BASE_URL"))
        .unwrap_or_else(|_| "https://cc2.caaa.tech/v1".into());
    let api_key = std::env::var("VOICEWIN_LIVE_API_KEY")
        .or_else(|_| std::env::var("LLM_API_KEY"))
        .context("missing VOICEWIN_LIVE_API_KEY or LLM_API_KEY")?;
    let model = std::env::var("VOICEWIN_LIVE_MODEL").unwrap_or_else(|_| "gpt-5.4".into());
    let reasoning_effort = std::env::var("VOICEWIN_LIVE_REASONING_EFFORT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let rounds = std::env::var("VOICEWIN_LIVE_ROUNDS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(5);
    let warmup_delay_ms = std::env::var("VOICEWIN_LIVE_WARMUP_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let system_text = std::env::var("VOICEWIN_LIVE_SYSTEM").unwrap_or_else(|_| {
        "You are VoiceWin's post-processor. Return only the cleaned-up dictation.".into()
    });
    let user_text = std::env::var("VOICEWIN_LIVE_TEXT")
        .unwrap_or_else(|_| "turn this into a polished sentence: hello voicewin world".into());
    let attached_image = support::load_optional_image_from_env(
        &["VOICEWIN_LIVE_IMAGE_DATA_URL"],
        &["VOICEWIN_LIVE_IMAGE_PATH"],
    )?;

    let cfg = OpenAiResponsesConfig {
        base_url: base_url.clone(),
        api_key: api_key.clone(),
        model: model.clone(),
        reasoning_effort: reasoning_effort.clone(),
    };
    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: system_text,
        },
        ChatMessage {
            role: "user".into(),
            content: user_text,
        },
    ];

    let mut cold = Vec::with_capacity(rounds);
    let mut warmed = Vec::with_capacity(rounds);
    let mut persistent = Vec::with_capacity(rounds);
    let persistent_http = HttpExecutor::new()?;

    for _ in 0..rounds {
        let cold_http = HttpExecutor::new()?;
        cold.push(
            run_responses_once(
                &cold_http,
                &cfg,
                &messages,
                attached_image.as_ref().map(|image| &image.artifact),
            )
            .await?,
        );

        let warmed_http = HttpExecutor::new()?;
        warmed.push(
            run_warmed_once(
                &warmed_http,
                &cfg,
                &messages,
                attached_image.as_ref().map(|image| &image.artifact),
                warmup_delay_ms,
            )
            .await?,
        );

        persistent.push(
            run_responses_once(
                &persistent_http,
                &cfg,
                &messages,
                attached_image.as_ref().map(|image| &image.artifact),
            )
            .await?,
        );
    }

    let result = BenchmarkResult {
        base_url,
        model,
        reasoning_effort,
        image_enabled: attached_image.is_some(),
        image_source: attached_image.as_ref().map(|image| image.source.clone()),
        image_mime_type: attached_image.as_ref().map(|image| image.mime_type.clone()),
        image_bytes: attached_image.as_ref().map(|image| image.bytes),
        rounds,
        warmup_delay_ms,
        summaries: vec![
            summarize_response("cold", &cold),
            summarize_warmed("warmed", &warmed),
            summarize_response("persistent", &persistent),
        ],
        cold,
        warmed,
        persistent,
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&result).context("encode benchmark output")?
    );
    Ok(())
}

async fn run_responses_once(
    http: &HttpExecutor,
    cfg: &OpenAiResponsesConfig,
    messages: &[ChatMessage],
    attached_image: Option<&voicewin_core::context::ImageArtifact>,
) -> anyhow::Result<ResponseTiming> {
    let req = build_responses_sse_request(cfg, messages, attached_image);
    let response = http.execute_sse_collect_text_metrics(&req).await?;
    Ok(ResponseTiming {
        text: response.text,
        total_ms: u128::from(response.total_ms),
        first_token_ms: response.first_text_ms,
    })
}

async fn run_warmed_once(
    http: &HttpExecutor,
    cfg: &OpenAiResponsesConfig,
    messages: &[ChatMessage],
    attached_image: Option<&voicewin_core::context::ImageArtifact>,
    warmup_delay_ms: u64,
) -> anyhow::Result<WarmedTiming> {
    let models_req = build_list_models_request(&cfg.base_url, &cfg.api_key);
    let warm_started = Instant::now();
    let warm_resp = http.execute(&models_req).await?;
    let warmup_ms = warm_started.elapsed().as_millis();

    if warmup_delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(warmup_delay_ms)).await;
    }

    let req = build_responses_sse_request(cfg, messages, attached_image);
    let response = http.execute_sse_collect_text_metrics(&req).await?;

    Ok(WarmedTiming {
        text: response.text,
        warmup_ms,
        total_ms: u128::from(response.total_ms),
        warmup_status: warm_resp.status,
        first_token_ms: response.first_text_ms,
    })
}

fn summarize_response(name: &str, rows: &[ResponseTiming]) -> Summary {
    Summary {
        name: name.into(),
        runs: rows.len(),
        total_median_ms: median(rows.iter().map(|row| row.total_ms).collect()),
        total_min_ms: rows
            .iter()
            .map(|row| row.total_ms)
            .min()
            .unwrap_or_default(),
        total_max_ms: rows
            .iter()
            .map(|row| row.total_ms)
            .max()
            .unwrap_or_default(),
        warmup_median_ms: None,
        first_token_median_ms: median_option(
            rows.iter()
                .filter_map(|row| row.first_token_ms.map(u128::from))
                .collect(),
        ),
        sample_text: rows.first().map(|row| row.text.clone()),
    }
}

fn summarize_warmed(name: &str, rows: &[WarmedTiming]) -> Summary {
    Summary {
        name: name.into(),
        runs: rows.len(),
        total_median_ms: median(rows.iter().map(|row| row.total_ms).collect()),
        total_min_ms: rows
            .iter()
            .map(|row| row.total_ms)
            .min()
            .unwrap_or_default(),
        total_max_ms: rows
            .iter()
            .map(|row| row.total_ms)
            .max()
            .unwrap_or_default(),
        warmup_median_ms: Some(median(rows.iter().map(|row| row.warmup_ms).collect())),
        first_token_median_ms: median_option(
            rows.iter()
                .filter_map(|row| row.first_token_ms.map(u128::from))
                .collect(),
        ),
        sample_text: rows.first().map(|row| row.text.clone()),
    }
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_option(values: Vec<u128>) -> Option<u128> {
    if values.is_empty() {
        None
    } else {
        Some(median(values))
    }
}
