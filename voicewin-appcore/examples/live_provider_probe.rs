use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use voicewin_appcore::service::AppService;
use voicewin_core::types::AppIdentity;
use voicewin_engine::traits::ContextSnapshot;
use voicewin_platform::test::{StdoutInserter, TestContextProvider};
use voicewin_runtime::ipc::ProviderProbeKind;

fn temp_config_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::env::temp_dir()
        .join(format!("voicewin-live-provider-probe-{nonce}"))
        .join("config.json")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let provider_kind =
        std::env::var("VOICEWIN_LIVE_PROVIDER_KIND").unwrap_or_else(|_| "openai_compatible".into());
    let api_key = std::env::var("VOICEWIN_LIVE_API_KEY")
        .or_else(|_| std::env::var("LLM_API_KEY"))
        .context("missing VOICEWIN_LIVE_API_KEY or LLM_API_KEY")?;
    let (base_url, model, api_kind) = match provider_kind.as_str() {
        "gemini" => (
            std::env::var("VOICEWIN_LIVE_BASE_URL")
                .unwrap_or_else(|_| "https://cc2.caaa.tech/v1beta".into()),
            std::env::var("VOICEWIN_LIVE_MODEL")
                .unwrap_or_else(|_| "gemini-3-flash-preview".into()),
            std::env::var("VOICEWIN_LIVE_API_KIND")
                .unwrap_or_else(|_| "stream_generate_content_sse".into()),
        ),
        _ => (
            std::env::var("VOICEWIN_LIVE_BASE_URL")
                .unwrap_or_else(|_| "https://cc2.caaa.tech/v1".into()),
            std::env::var("VOICEWIN_LIVE_MODEL").unwrap_or_else(|_| "gpt-5.4".into()),
            std::env::var("VOICEWIN_LIVE_API_KIND").unwrap_or_else(|_| "responses_sse".into()),
        ),
    };
    let reasoning_effort = std::env::var("VOICEWIN_LIVE_REASONING_EFFORT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let rounds = std::env::var("VOICEWIN_LIVE_ROUNDS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);
    let sleep_ms = std::env::var("VOICEWIN_LIVE_SLEEP_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let probe_kind = match std::env::var("VOICEWIN_LIVE_PROBE_KIND")
        .unwrap_or_else(|_| "smoke".into())
        .trim()
    {
        "screenshot_product_name" | "screenshot" => ProviderProbeKind::ScreenshotProductName,
        _ => ProviderProbeKind::Smoke,
    };

    let svc = AppService::new(
        temp_config_path(),
        TestContextProvider::new(AppIdentity::new(), ContextSnapshot::default()).boxed(),
        Arc::new(StdoutInserter),
    );

    match provider_kind.as_str() {
        "gemini" => svc.set_gemini_api_key(&api_key)?,
        _ => svc.set_openai_api_key(&api_key)?,
    }

    let mut elapsed_ms = Vec::with_capacity(rounds);
    let mut first_token_ms = Vec::with_capacity(rounds);
    let mut input_tokens = Vec::with_capacity(rounds);
    let mut cached_input_tokens = Vec::with_capacity(rounds);
    let mut final_response = None;

    for round in 0..rounds {
        let response = svc
            .probe_llm_provider(
                &provider_kind,
                &api_kind,
                &base_url,
                &model,
                reasoning_effort.as_deref(),
                probe_kind.clone(),
            )
            .await
            .with_context(|| format!("run live provider probe round {}", round + 1))?;

        println!(
            "round={} provider_elapsed_ms={}",
            round + 1,
            response.elapsed_ms
        );
        println!(
            "round={} provider_first_token_ms={}",
            round + 1,
            response.first_token_ms.unwrap_or_default()
        );
        println!(
            "round={} provider_input_tokens={}",
            round + 1,
            response.input_tokens.unwrap_or_default()
        );
        println!(
            "round={} provider_cached_input_tokens={}",
            round + 1,
            response.cached_input_tokens.unwrap_or_default()
        );

        elapsed_ms.push(u128::from(response.elapsed_ms));
        if let Some(value) = response.first_token_ms {
            first_token_ms.push(u128::from(value));
        }
        if let Some(value) = response.input_tokens {
            input_tokens.push(u128::from(value));
        }
        if let Some(value) = response.cached_input_tokens {
            cached_input_tokens.push(u128::from(value));
        }
        final_response = Some(response);

        if sleep_ms > 0 && round + 1 < rounds {
            tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
        }
    }

    let response = final_response.expect("at least one round should run");
    let min_elapsed = elapsed_ms.iter().copied().min().unwrap_or(0);
    let max_elapsed = elapsed_ms.iter().copied().max().unwrap_or(0);
    let avg_elapsed = if elapsed_ms.is_empty() {
        0
    } else {
        elapsed_ms.iter().sum::<u128>() / elapsed_ms.len() as u128
    };
    let min_first_token = first_token_ms.iter().copied().min().unwrap_or(0);
    let max_first_token = first_token_ms.iter().copied().max().unwrap_or(0);
    let avg_first_token = if first_token_ms.is_empty() {
        0
    } else {
        first_token_ms.iter().sum::<u128>() / first_token_ms.len() as u128
    };
    let min_cached_input_tokens = cached_input_tokens.iter().copied().min().unwrap_or(0);
    let max_cached_input_tokens = cached_input_tokens.iter().copied().max().unwrap_or(0);
    let avg_cached_input_tokens = if cached_input_tokens.is_empty() {
        0
    } else {
        cached_input_tokens.iter().sum::<u128>() / cached_input_tokens.len() as u128
    };
    let min_input_tokens = input_tokens.iter().copied().min().unwrap_or(0);
    let max_input_tokens = input_tokens.iter().copied().max().unwrap_or(0);
    let avg_input_tokens = if input_tokens.is_empty() {
        0
    } else {
        input_tokens.iter().sum::<u128>() / input_tokens.len() as u128
    };

    println!("rounds={rounds}");
    println!("elapsed_min_ms={min_elapsed}");
    println!("elapsed_avg_ms={avg_elapsed}");
    println!("elapsed_max_ms={max_elapsed}");
    println!("first_token_min_ms={min_first_token}");
    println!("first_token_avg_ms={avg_first_token}");
    println!("first_token_max_ms={max_first_token}");
    println!("input_tokens_min={min_input_tokens}");
    println!("input_tokens_avg={avg_input_tokens}");
    println!("input_tokens_max={max_input_tokens}");
    println!("cached_input_tokens_min={min_cached_input_tokens}");
    println!("cached_input_tokens_avg={avg_cached_input_tokens}");
    println!("cached_input_tokens_max={max_cached_input_tokens}");
    println!("provider_kind={}", response.provider_kind);
    println!("api_kind={}", response.api_kind);
    println!("model={}", response.model);
    println!("probe_kind={:?}", response.probe_kind);
    println!("elapsed_ms={}", response.elapsed_ms);
    println!(
        "first_token_ms={}",
        response.first_token_ms.unwrap_or_default()
    );
    println!("input_tokens={}", response.input_tokens.unwrap_or_default());
    println!(
        "cached_input_tokens={}",
        response.cached_input_tokens.unwrap_or_default()
    );
    println!("expected_output={}", response.expected_output);
    println!("final_output={}", response.final_output);
    println!("warning={}", response.warning.unwrap_or_default());

    Ok(())
}
