use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use voicewin_appcore::service::AppService;
use voicewin_core::config::AppConfig;
use voicewin_core::context::{ImageArtifact, VisualCaptureScope, VisualContextMode};
use voicewin_core::enhancement::{PromptMode, PromptTemplate};
use voicewin_core::types::{AppIdentity, PromptId};
use voicewin_engine::traits::{AudioInput, ContextSnapshot};
use voicewin_platform::test::{StdoutInserter, TestContextProvider};
use voicewin_runtime::defaults::default_global_defaults;
use voicewin_runtime::history::HistoryStore;

fn temp_config_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::env::temp_dir()
        .join(format!("voicewin-live-session-stop-{nonce}"))
        .join("config.json")
}

fn parse_bool_env(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

fn parse_u64_env(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
}

fn parse_legacy_bool_env(name: &str) -> Option<bool> {
    std::env::var(name)
        .ok()
        .and_then(|value| match value.trim() {
            "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON" => Some(true),
            "0" | "false" | "FALSE" | "no" | "NO" | "off" | "OFF" => Some(false),
            _ => None,
        })
}

fn parse_visual_context_mode(
    screenshot_data_url: Option<&String>,
) -> anyhow::Result<VisualContextMode> {
    if let Ok(value) = std::env::var("VOICEWIN_LIVE_VISUAL_MODE") {
        return match value.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(VisualContextMode::Off),
            "auto" => Ok(VisualContextMode::Auto),
            "screenshot" => Ok(VisualContextMode::Screenshot),
            "ocr" => Ok(VisualContextMode::Ocr),
            other => anyhow::bail!(
                "unsupported VOICEWIN_LIVE_VISUAL_MODE={other}; expected off|auto|screenshot|ocr"
            ),
        };
    }

    if let Some(enabled) = parse_legacy_bool_env("VOICEWIN_LIVE_USE_OCR") {
        return Ok(if enabled {
            VisualContextMode::Screenshot
        } else {
            VisualContextMode::Off
        });
    }

    Ok(if screenshot_data_url.is_some() {
        VisualContextMode::Screenshot
    } else {
        VisualContextMode::Off
    })
}

fn parse_visual_capture_scope() -> anyhow::Result<VisualCaptureScope> {
    match std::env::var("VOICEWIN_LIVE_VISUAL_CAPTURE_SCOPE")
        .unwrap_or_else(|_| "display".into())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "display" => Ok(VisualCaptureScope::Display),
        "foreground_window" | "foreground-window" => Ok(VisualCaptureScope::ForegroundWindow),
        other => anyhow::bail!(
            "unsupported VOICEWIN_LIVE_VISUAL_CAPTURE_SCOPE={other}; expected display|foreground_window"
        ),
    }
}

fn avg(values: &[u128]) -> u128 {
    if values.is_empty() {
        0
    } else {
        values.iter().sum::<u128>() / values.len() as u128
    }
}

fn screen_ocr_source_label(
    runtime: Option<&voicewin_core::llm::VisualContextRuntime>,
) -> Option<&'static str> {
    match runtime?.screen_ocr_source? {
        voicewin_core::llm::ScreenOcrSource::Inline => Some("inline"),
        voicewin_core::llm::ScreenOcrSource::Prepared => Some("prepared"),
    }
}

fn capture_scope_label(scope: VisualCaptureScope) -> &'static str {
    match scope {
        VisualCaptureScope::Display => "display",
        VisualCaptureScope::ForegroundWindow => "foreground_window",
    }
}

fn visual_runtime_label(
    runtime: Option<&voicewin_core::llm::VisualContextRuntime>,
) -> Option<String> {
    let runtime = runtime?;
    if matches!(runtime.mode, VisualContextMode::Off) {
        return None;
    }

    let mode = format!("{:?}", runtime.mode).to_ascii_lowercase();
    let dispatch = format!("{:?}", runtime.dispatch).to_ascii_lowercase();
    let visual = if dispatch == mode {
        mode
    } else {
        format!("{mode}->{dispatch}")
    };
    let scope = capture_scope_label(runtime.capture_scope);
    let source_suffix = screen_ocr_source_label(Some(runtime))
        .map(|source| format!("/ocr_{source}"))
        .unwrap_or_default();
    let actual_scope_suffix = runtime
        .capture_actual_scope
        .filter(|actual_scope| *actual_scope != runtime.capture_scope)
        .map(|actual_scope| format!("/actual_{}", capture_scope_label(actual_scope)))
        .unwrap_or_default();
    Some(format!(
        "{visual}/{scope}{source_suffix}{actual_scope_suffix}"
    ))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let provider_kind =
        std::env::var("VOICEWIN_LIVE_PROVIDER_KIND").unwrap_or_else(|_| "openai_compatible".into());
    let api_key = std::env::var("VOICEWIN_LIVE_API_KEY")
        .or_else(|_| std::env::var("LLM_API_KEY"))
        .context("missing VOICEWIN_LIVE_API_KEY or LLM_API_KEY")?;
    let prepared = parse_bool_env("VOICEWIN_LIVE_PREPARED");
    let prepared_concurrent = parse_bool_env("VOICEWIN_LIVE_PREPARED_CONCURRENT");
    let rounds = std::env::var("VOICEWIN_LIVE_ROUNDS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);
    let recording_sleep_ms = std::env::var("VOICEWIN_LIVE_RECORDING_SLEEP_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let snapshot_delay_ms = parse_u64_env("VOICEWIN_LIVE_SNAPSHOT_DELAY_MS").unwrap_or(0);
    let capture_delay_ms = parse_u64_env("VOICEWIN_LIVE_CAPTURE_DELAY_MS").unwrap_or(0);
    let prompt_text = std::env::var("VOICEWIN_LIVE_PROMPT_TEXT").unwrap_or_else(|_| {
        "Fix grammar, punctuation, capitalization, and light dictation disfluencies while preserving meaning. Output only the cleaned text.".into()
    });
    let transcript = std::env::var("VOICEWIN_LIVE_TRANSCRIPT").unwrap_or_else(|_| {
        "please ship the voice win update using eleven labs scribe v2 later this week".into()
    });
    let screenshot_data_url = std::env::var("VOICEWIN_LIVE_SCREENSHOT_DATA_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let visual_context_mode = parse_visual_context_mode(screenshot_data_url.as_ref())?;
    let visual_capture_scope = parse_visual_capture_scope()?;

    let mut defaults = default_global_defaults();
    match provider_kind.as_str() {
        "gemini" => {
            defaults.llm_provider_kind = "gemini".into();
            defaults.llm_base_url = std::env::var("VOICEWIN_LIVE_BASE_URL")
                .unwrap_or_else(|_| "https://cc2.caaa.tech/v1beta".into());
            defaults.llm_model = std::env::var("VOICEWIN_LIVE_MODEL")
                .unwrap_or_else(|_| "gemini-3-flash-preview".into());
            defaults.llm_api_kind = std::env::var("VOICEWIN_LIVE_API_KIND")
                .unwrap_or_else(|_| "stream_generate_content_sse".into());
        }
        _ => {
            defaults.llm_provider_kind = "openai_compatible".into();
            defaults.llm_base_url = std::env::var("VOICEWIN_LIVE_BASE_URL")
                .unwrap_or_else(|_| "https://cc2.caaa.tech/v1".into());
            defaults.llm_model =
                std::env::var("VOICEWIN_LIVE_MODEL").unwrap_or_else(|_| "gpt-5.4".into());
            defaults.llm_api_kind =
                std::env::var("VOICEWIN_LIVE_API_KIND").unwrap_or_else(|_| "responses_sse".into());
        }
    }
    defaults.llm_preflight_mode =
        std::env::var("VOICEWIN_LIVE_PREFLIGHT_MODE").unwrap_or_else(|_| "off".into());
    defaults.llm_preflight_delay_ms = parse_u64_env("VOICEWIN_LIVE_PREFLIGHT_DELAY_MS")
        .unwrap_or(defaults.llm_preflight_delay_ms);
    defaults.llm_reasoning_effort = std::env::var("VOICEWIN_LIVE_REASONING_EFFORT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    defaults.enable_enhancement = true;
    defaults.context.use_clipboard = false;
    defaults.context.use_selected_text = false;
    defaults.context.use_window_context = false;
    defaults.context.use_custom_vocabulary = false;
    defaults.context.visual_context_mode = visual_context_mode;
    defaults.context.visual_capture_scope = visual_capture_scope;

    let prompt = PromptTemplate {
        id: PromptId::new(),
        title: std::env::var("VOICEWIN_LIVE_PROMPT_TITLE")
            .unwrap_or_else(|_| "Live Stop Latency".into()),
        mode: PromptMode::Enhancer,
        prompt_text,
        trigger_words: vec![],
    };
    defaults.prompt_id = Some(prompt.id.clone());

    let cfg = AppConfig {
        defaults,
        profiles: vec![],
        prompts: vec![prompt],
        llm_api_key_present: true,
    };

    let config_path = temp_config_path();
    let svc = AppService::new(
        config_path.clone(),
        TestContextProvider::new(
            AppIdentity::new()
                .with_process_name("mail.exe")
                .with_window_title("Inbox"),
            ContextSnapshot::default(),
        )
        .with_snapshot_delay(Duration::from_millis(snapshot_delay_ms))
        .with_capture_delay(Duration::from_millis(capture_delay_ms))
        .with_captured_screenshot(screenshot_data_url.as_ref().map(|data_url| ImageArtifact {
            data_url: data_url.clone(),
        }))
        .boxed(),
        Arc::new(StdoutInserter),
    );
    svc.save_config(&cfg)
        .context("save stop-latency benchmark config")?;

    match provider_kind.as_str() {
        "gemini" => svc.set_gemini_api_key(&api_key)?,
        _ => svc.set_openai_api_key(&api_key)?,
    }

    let history_path = config_path
        .parent()
        .map(|dir| dir.join("history.json"))
        .unwrap_or_else(|| PathBuf::from("history.json"));
    let history = HistoryStore::at_path(history_path);

    let mut prepare_wall_ms = Vec::with_capacity(rounds);
    let mut stop_wall_ms = Vec::with_capacity(rounds);
    let mut enhancement_ms = Vec::with_capacity(rounds);
    let mut first_token_ms = Vec::with_capacity(rounds);
    let mut cached_input_tokens = Vec::with_capacity(rounds);
    let mut ocr_elapsed_ms = Vec::with_capacity(rounds);
    let mut ocr_first_token_ms = Vec::with_capacity(rounds);
    let mut ocr_text_chars = Vec::with_capacity(rounds);
    let mut visual_variants = HashSet::new();
    let mut screen_ocr_source_variants = HashSet::new();
    let mut final_visual_dispatch = None;
    let mut final_screen_ocr_source = None;
    let mut final_text = None;
    let mut final_warning = None;

    for round in 0..rounds {
        let prepared_ctx = if prepared {
            if prepared_concurrent {
                let svc_for_prepare = svc.clone();
                let prepare_task = tokio::spawn(async move {
                    let started = Instant::now();
                    let prepared = svc_for_prepare.prepare_session_context().await;
                    let elapsed_ms = started.elapsed().as_millis();
                    (prepared, elapsed_ms)
                });

                if recording_sleep_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(recording_sleep_ms)).await;
                }

                let stop_started = Instant::now();
                let (prepared_result, elapsed_ms) =
                    prepare_task.await.context("join concurrent prepare task")?;
                prepare_wall_ms.push(elapsed_ms);
                let prepared_ctx = prepared_result
                    .with_context(|| format!("prepare session context round {}", round + 1))?;

                let response = svc
                    .run_session_with_prepared_with_hook(
                        prepared_ctx,
                        voicewin_runtime::ipc::RunSessionRequest {
                            transcript: transcript.clone(),
                            warning: None,
                        },
                        AudioInput {
                            sample_rate_hz: 16_000,
                            samples: vec![],
                        },
                        |_stage| async {},
                    )
                    .await
                    .with_context(|| format!("run stop path round {}", round + 1))?;
                let stop_elapsed = stop_started.elapsed().as_millis();
                stop_wall_ms.push(stop_elapsed);
                let round_warning = response.warning.clone();
                final_text = response.final_text;
                final_warning = round_warning.clone();

                let entry = history
                    .load()
                    .context("load history after stop-latency run")?
                    .into_iter()
                    .last()
                    .context("missing history entry after stop-latency run")?;

                if let Some(value) = entry.enhancement_ms {
                    enhancement_ms.push(u128::from(value));
                }
                if let Some(value) = entry.enhancement_first_token_ms {
                    first_token_ms.push(u128::from(value));
                }
                if let Some(value) = entry.enhancement_cached_input_tokens {
                    cached_input_tokens.push(u128::from(value));
                }
                if let Some(runtime) = entry.visual_context_runtime.as_ref() {
                    if let Some(label) = visual_runtime_label(Some(runtime)) {
                        visual_variants.insert(label.clone());
                        final_visual_dispatch = Some(label);
                    }
                    if let Some(actual_scope) = runtime.capture_actual_scope {
                        println!(
                            "round={} capture_actual_scope={}",
                            round + 1,
                            capture_scope_label(actual_scope)
                        );
                    }
                    if let Some(value) = runtime.screenshot_capture_elapsed_ms {
                        println!("round={} screenshot_capture_elapsed_ms={value}", round + 1);
                    }
                    if let Some(reason) = runtime.capture_fallback_reason.as_deref() {
                        println!("round={} capture_fallback_reason={reason}", round + 1);
                    }
                    if let Some(source) = screen_ocr_source_label(Some(runtime)) {
                        screen_ocr_source_variants.insert(source);
                        final_screen_ocr_source = Some(source);
                    }
                    if let Some(value) = runtime.screen_ocr_elapsed_ms {
                        ocr_elapsed_ms.push(u128::from(value));
                    }
                    if let Some(value) = runtime.screen_ocr_first_token_ms {
                        ocr_first_token_ms.push(u128::from(value));
                    }
                    if let Some(value) = runtime.screen_ocr_text_chars {
                        ocr_text_chars.push(u128::from(value));
                    }
                }

                println!(
                    "round={} prepared={} prepared_concurrent={} preflight_mode={} preflight_delay_ms={} prepare_wall_ms={} stop_wall_ms={} enhancement_ms={} first_token_ms={} cached_input_tokens={} visual_dispatch={} screen_ocr_source={} screen_ocr_elapsed_ms={} screen_ocr_first_token_ms={} screen_ocr_text_chars={} stage={} warning={}",
                    round + 1,
                    prepared,
                    prepared_concurrent,
                    cfg.defaults.llm_preflight_mode,
                    cfg.defaults.llm_preflight_delay_ms,
                    elapsed_ms,
                    stop_elapsed,
                    entry.enhancement_ms.unwrap_or_default(),
                    entry.enhancement_first_token_ms.unwrap_or_default(),
                    entry.enhancement_cached_input_tokens.unwrap_or_default(),
                    entry
                        .visual_context_runtime
                        .as_ref()
                        .and_then(|runtime| visual_runtime_label(Some(runtime)))
                        .unwrap_or_default(),
                    entry
                        .visual_context_runtime
                        .as_ref()
                        .and_then(|runtime| screen_ocr_source_label(Some(runtime)))
                        .unwrap_or_default(),
                    entry
                        .visual_context_runtime
                        .as_ref()
                        .and_then(|runtime| runtime.screen_ocr_elapsed_ms)
                        .unwrap_or_default(),
                    entry
                        .visual_context_runtime
                        .as_ref()
                        .and_then(|runtime| runtime.screen_ocr_first_token_ms)
                        .unwrap_or_default(),
                    entry
                        .visual_context_runtime
                        .as_ref()
                        .and_then(|runtime| runtime.screen_ocr_text_chars)
                        .unwrap_or_default(),
                    response.stage,
                    round_warning.as_deref().unwrap_or(""),
                );
                continue;
            }

            let started = Instant::now();
            let ctx = svc
                .prepare_session_context()
                .await
                .with_context(|| format!("prepare session context round {}", round + 1))?;
            let elapsed = started.elapsed().as_millis();
            prepare_wall_ms.push(elapsed);
            Some(ctx)
        } else {
            None
        };

        if recording_sleep_ms > 0 {
            tokio::time::sleep(Duration::from_millis(recording_sleep_ms)).await;
        }

        let stop_started = Instant::now();
        let response = if let Some(prepared_ctx) = prepared_ctx {
            svc.run_session_with_prepared_with_hook(
                prepared_ctx,
                voicewin_runtime::ipc::RunSessionRequest {
                    transcript: transcript.clone(),
                    warning: None,
                },
                AudioInput {
                    sample_rate_hz: 16_000,
                    samples: vec![],
                },
                |_stage| async {},
            )
            .await
        } else {
            svc.run_session(
                voicewin_runtime::ipc::RunSessionRequest {
                    transcript: transcript.clone(),
                    warning: None,
                },
                AudioInput {
                    sample_rate_hz: 16_000,
                    samples: vec![],
                },
            )
            .await
        }
        .with_context(|| format!("run stop path round {}", round + 1))?;
        let stop_elapsed = stop_started.elapsed().as_millis();
        stop_wall_ms.push(stop_elapsed);
        let round_warning = response.warning.clone();
        final_text = response.final_text;
        final_warning = round_warning.clone();

        let entry = history
            .load()
            .context("load history after stop-latency run")?
            .into_iter()
            .last()
            .context("missing history entry after stop-latency run")?;

        if let Some(value) = entry.enhancement_ms {
            enhancement_ms.push(u128::from(value));
        }
        if let Some(value) = entry.enhancement_first_token_ms {
            first_token_ms.push(u128::from(value));
        }
        if let Some(value) = entry.enhancement_cached_input_tokens {
            cached_input_tokens.push(u128::from(value));
        }
        if let Some(runtime) = entry.visual_context_runtime.as_ref() {
            if let Some(label) = visual_runtime_label(Some(runtime)) {
                visual_variants.insert(label.clone());
                final_visual_dispatch = Some(label);
            }
            if let Some(actual_scope) = runtime.capture_actual_scope {
                println!(
                    "round={} capture_actual_scope={}",
                    round + 1,
                    capture_scope_label(actual_scope)
                );
            }
            if let Some(value) = runtime.screenshot_capture_elapsed_ms {
                println!("round={} screenshot_capture_elapsed_ms={value}", round + 1);
            }
            if let Some(reason) = runtime.capture_fallback_reason.as_deref() {
                println!("round={} capture_fallback_reason={reason}", round + 1);
            }
            if let Some(source) = screen_ocr_source_label(Some(runtime)) {
                screen_ocr_source_variants.insert(source);
                final_screen_ocr_source = Some(source);
            }
            if let Some(value) = runtime.screen_ocr_elapsed_ms {
                ocr_elapsed_ms.push(u128::from(value));
            }
            if let Some(value) = runtime.screen_ocr_first_token_ms {
                ocr_first_token_ms.push(u128::from(value));
            }
            if let Some(value) = runtime.screen_ocr_text_chars {
                ocr_text_chars.push(u128::from(value));
            }
        }

        println!(
            "round={} prepared={} prepared_concurrent={} preflight_mode={} preflight_delay_ms={} prepare_wall_ms={} stop_wall_ms={} enhancement_ms={} first_token_ms={} cached_input_tokens={} visual_dispatch={} screen_ocr_source={} screen_ocr_elapsed_ms={} screen_ocr_first_token_ms={} screen_ocr_text_chars={} stage={} warning={}",
            round + 1,
            prepared,
            prepared_concurrent,
            cfg.defaults.llm_preflight_mode,
            cfg.defaults.llm_preflight_delay_ms,
            prepare_wall_ms.last().copied().unwrap_or(0),
            stop_elapsed,
            entry.enhancement_ms.unwrap_or_default(),
            entry.enhancement_first_token_ms.unwrap_or_default(),
            entry.enhancement_cached_input_tokens.unwrap_or_default(),
            entry
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| visual_runtime_label(Some(runtime)))
                .unwrap_or_default(),
            entry
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| screen_ocr_source_label(Some(runtime)))
                .unwrap_or_default(),
            entry
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| runtime.screen_ocr_elapsed_ms)
                .unwrap_or_default(),
            entry
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| runtime.screen_ocr_first_token_ms)
                .unwrap_or_default(),
            entry
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| runtime.screen_ocr_text_chars)
                .unwrap_or_default(),
            response.stage,
            round_warning.as_deref().unwrap_or(""),
        );
    }

    println!("rounds={rounds}");
    println!("prepared={prepared}");
    println!("prepared_concurrent={prepared_concurrent}");
    println!("screenshot_enabled={}", screenshot_data_url.is_some());
    println!("visual_context_mode={visual_context_mode:?}");
    println!("visual_capture_scope={visual_capture_scope:?}");
    println!("recording_sleep_ms={recording_sleep_ms}");
    println!("snapshot_delay_ms={snapshot_delay_ms}");
    println!("capture_delay_ms={capture_delay_ms}");
    println!("preflight_mode={}", cfg.defaults.llm_preflight_mode);
    println!("preflight_delay_ms={}", cfg.defaults.llm_preflight_delay_ms);
    println!(
        "prepare_wall_min_ms={}",
        prepare_wall_ms.iter().copied().min().unwrap_or(0)
    );
    println!("prepare_wall_avg_ms={}", avg(&prepare_wall_ms));
    println!(
        "prepare_wall_max_ms={}",
        prepare_wall_ms.iter().copied().max().unwrap_or(0)
    );
    println!(
        "stop_wall_min_ms={}",
        stop_wall_ms.iter().copied().min().unwrap_or(0)
    );
    println!("stop_wall_avg_ms={}", avg(&stop_wall_ms));
    println!(
        "stop_wall_max_ms={}",
        stop_wall_ms.iter().copied().max().unwrap_or(0)
    );
    println!(
        "enhancement_min_ms={}",
        enhancement_ms.iter().copied().min().unwrap_or(0)
    );
    println!("enhancement_avg_ms={}", avg(&enhancement_ms));
    println!(
        "enhancement_max_ms={}",
        enhancement_ms.iter().copied().max().unwrap_or(0)
    );
    println!(
        "first_token_min_ms={}",
        first_token_ms.iter().copied().min().unwrap_or(0)
    );
    println!("first_token_avg_ms={}", avg(&first_token_ms));
    println!(
        "first_token_max_ms={}",
        first_token_ms.iter().copied().max().unwrap_or(0)
    );
    println!(
        "cached_input_tokens_min={}",
        cached_input_tokens.iter().copied().min().unwrap_or(0)
    );
    println!("cached_input_tokens_avg={}", avg(&cached_input_tokens));
    println!(
        "cached_input_tokens_max={}",
        cached_input_tokens.iter().copied().max().unwrap_or(0)
    );
    println!("visual_variant_count={}", visual_variants.len());
    println!(
        "visual_dispatch={}",
        final_visual_dispatch.as_deref().unwrap_or_default()
    );
    println!(
        "capture_actual_scope={}",
        history
            .load()
            .ok()
            .and_then(|entries| entries.into_iter().last())
            .as_ref()
            .and_then(|entry| entry.visual_context_runtime.as_ref())
            .and_then(|runtime| runtime.capture_actual_scope)
            .map(capture_scope_label)
            .unwrap_or("")
    );
    println!(
        "screenshot_capture_elapsed_ms={}",
        history
            .load()
            .ok()
            .and_then(|entries| entries.into_iter().last())
            .as_ref()
            .and_then(|entry| entry.visual_context_runtime.as_ref())
            .and_then(|runtime| runtime.screenshot_capture_elapsed_ms)
            .unwrap_or_default()
    );
    println!(
        "capture_fallback_reason={}",
        history
            .load()
            .ok()
            .and_then(|entries| entries.into_iter().last())
            .as_ref()
            .and_then(|entry| entry.visual_context_runtime.as_ref())
            .and_then(|runtime| runtime.capture_fallback_reason.as_deref())
            .unwrap_or("")
    );
    println!(
        "screen_ocr_source_variant_count={}",
        screen_ocr_source_variants.len()
    );
    println!(
        "screen_ocr_source={}",
        final_screen_ocr_source.unwrap_or_default()
    );
    println!(
        "screen_ocr_elapsed_min_ms={}",
        ocr_elapsed_ms.iter().copied().min().unwrap_or(0)
    );
    println!("screen_ocr_elapsed_avg_ms={}", avg(&ocr_elapsed_ms));
    println!(
        "screen_ocr_elapsed_max_ms={}",
        ocr_elapsed_ms.iter().copied().max().unwrap_or(0)
    );
    println!(
        "screen_ocr_first_token_min_ms={}",
        ocr_first_token_ms.iter().copied().min().unwrap_or(0)
    );
    println!("screen_ocr_first_token_avg_ms={}", avg(&ocr_first_token_ms));
    println!(
        "screen_ocr_first_token_max_ms={}",
        ocr_first_token_ms.iter().copied().max().unwrap_or(0)
    );
    println!(
        "screen_ocr_text_chars_min={}",
        ocr_text_chars.iter().copied().min().unwrap_or(0)
    );
    println!("screen_ocr_text_chars_avg={}", avg(&ocr_text_chars));
    println!(
        "screen_ocr_text_chars_max={}",
        ocr_text_chars.iter().copied().max().unwrap_or(0)
    );
    println!("final_text={}", final_text.as_deref().unwrap_or_default());
    println!("warning={}", final_warning.as_deref().unwrap_or_default());

    Ok(())
}
