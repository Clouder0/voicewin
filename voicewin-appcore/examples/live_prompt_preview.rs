use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use voicewin_appcore::service::AppService;
use voicewin_core::config::AppConfig;
use voicewin_core::context::{ImageArtifact, VisualCaptureScope, VisualContextMode};
use voicewin_core::enhancement::{PromptMode, PromptTemplate};
use voicewin_core::power_mode::{AppMatcher, PowerModeOverrides, PowerModeProfile};
use voicewin_core::types::{AppIdentity, ProfileId, PromptId};
use voicewin_engine::traits::ContextSnapshot;
use voicewin_platform::test::{StdoutInserter, TestContextProvider};
use voicewin_runtime::defaults::default_global_defaults;

fn temp_config_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::env::temp_dir()
        .join(format!("voicewin-live-preview-{nonce}"))
        .join("config.json")
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
    let prompt_text = std::env::var("VOICEWIN_LIVE_PROMPT_TEXT").unwrap_or_else(|_| {
        "Fix grammar, punctuation, and capitalization. Return only the cleaned dictation.".into()
    });
    let transcript = std::env::var("VOICEWIN_LIVE_TRANSCRIPT")
        .unwrap_or_else(|_| "turn this into a polished sentence: hello voicewin world".into());
    let rounds = std::env::var("VOICEWIN_LIVE_ROUNDS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);
    let sleep_ms = std::env::var("VOICEWIN_LIVE_SLEEP_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let preview_scope =
        std::env::var("VOICEWIN_LIVE_SCOPE").unwrap_or_else(|_| "current_app".into());
    let profile_name =
        std::env::var("VOICEWIN_LIVE_PROFILE_NAME").unwrap_or_else(|_| "Live Profile".into());
    let clipboard_text = std::env::var("VOICEWIN_LIVE_CLIPBOARD")
        .unwrap_or_else(|_| "clipboard from profile".into());
    let selected_text = std::env::var("VOICEWIN_LIVE_SELECTED_TEXT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let custom_vocabulary = std::env::var("VOICEWIN_LIVE_CUSTOM_VOCABULARY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
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
    defaults.llm_reasoning_effort = std::env::var("VOICEWIN_LIVE_REASONING_EFFORT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    defaults.enable_enhancement = true;
    defaults.context.use_clipboard = false;
    defaults.context.use_selected_text = selected_text.is_some();
    defaults.context.visual_context_mode = visual_context_mode;
    defaults.context.visual_capture_scope = visual_capture_scope;

    let prompt = PromptTemplate {
        id: PromptId::new(),
        title: std::env::var("VOICEWIN_LIVE_PROMPT_TITLE")
            .unwrap_or_else(|_| "Live Cleanup".into()),
        mode: PromptMode::Enhancer,
        prompt_text,
        trigger_words: vec![],
    };
    defaults.prompt_id = Some(prompt.id.clone());

    let mut forced_profile_id = None;
    let mut force_defaults = false;
    let profiles = match preview_scope.as_str() {
        "defaults" => {
            force_defaults = true;
            vec![PowerModeProfile {
                id: ProfileId::new(),
                name: profile_name.clone(),
                enabled: true,
                matchers: vec![AppMatcher::ProcessNameEquals("mail".into())],
                overrides: PowerModeOverrides {
                    context: Some(voicewin_core::context::ContextToggles {
                        use_clipboard: true,
                        use_selected_text: false,
                        use_window_context: true,
                        use_custom_vocabulary: false,
                        visual_context_mode,
                        visual_capture_scope,
                    }),
                    ..Default::default()
                },
            }]
        }
        "profile" => {
            let profile_id = ProfileId::new();
            forced_profile_id = Some(profile_id.clone());
            vec![PowerModeProfile {
                id: profile_id,
                name: profile_name.clone(),
                enabled: true,
                matchers: vec![AppMatcher::ProcessNameEquals("mail".into())],
                overrides: PowerModeOverrides {
                    context: Some(voicewin_core::context::ContextToggles {
                        use_clipboard: true,
                        use_selected_text: false,
                        use_window_context: true,
                        use_custom_vocabulary: false,
                        visual_context_mode,
                        visual_capture_scope,
                    }),
                    ..Default::default()
                },
            }]
        }
        _ => vec![],
    };

    let cfg = AppConfig {
        defaults,
        profiles,
        prompts: vec![prompt.clone()],
        llm_api_key_present: true,
    };

    let config_path = temp_config_path();
    let ctx = TestContextProvider::new(
        AppIdentity::new()
            .with_process_name("mail")
            .with_window_title("Inbox"),
        ContextSnapshot {
            clipboard: Some(clipboard_text.clone()),
            selected_text: selected_text.clone(),
            window_context: Some("Inbox".into()),
            ..Default::default()
        },
    )
    .with_captured_screenshot(screenshot_data_url.as_ref().map(|data_url| ImageArtifact {
        data_url: data_url.clone(),
    }))
    .boxed();
    let svc = AppService::new(config_path.clone(), ctx, Arc::new(StdoutInserter));
    svc.save_config(&cfg).context("save live preview config")?;
    if let Some(custom_vocabulary) = custom_vocabulary.as_ref() {
        let custom_vocabulary_path = config_path
            .parent()
            .map(|dir| dir.join("custom_vocabulary.txt"))
            .unwrap_or_else(|| PathBuf::from("custom_vocabulary.txt"));
        if let Some(parent) = custom_vocabulary_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "create custom vocabulary parent directory: {}",
                    parent.display()
                )
            })?;
        }
        std::fs::write(&custom_vocabulary_path, custom_vocabulary).with_context(|| {
            format!(
                "write live custom vocabulary file: {}",
                custom_vocabulary_path.display()
            )
        })?;
    }

    match provider_kind.as_str() {
        "gemini" => svc.set_gemini_api_key(&api_key)?,
        _ => svc.set_openai_api_key(&api_key)?,
    }

    let mut elapsed_ms = Vec::with_capacity(rounds);
    let mut input_tokens = Vec::with_capacity(rounds);
    let mut ocr_elapsed_ms = Vec::with_capacity(rounds);
    let mut ocr_first_token_ms = Vec::with_capacity(rounds);
    let mut ocr_text_chars = Vec::with_capacity(rounds);
    let mut visual_variants = HashSet::new();
    let mut screen_ocr_source_variants = HashSet::new();
    let mut final_response = None;

    for round in 0..rounds {
        let started = Instant::now();
        let response = svc
            .preview_prompt(
                prompt.clone(),
                transcript.clone(),
                forced_profile_id.clone(),
                force_defaults,
            )
            .await
            .with_context(|| format!("run live prompt preview round {}", round + 1))?;
        let elapsed = started.elapsed().as_millis();
        println!("round={} elapsed_ms={elapsed}", round + 1);
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
        if let Some(runtime) = response.visual_context_runtime.as_ref() {
            if let Some(label) = visual_runtime_label(Some(runtime)) {
                visual_variants.insert(label.clone());
                println!("round={} visual_dispatch={label}", round + 1);
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
                println!("round={} screen_ocr_source={source}", round + 1);
            }
            if let Some(value) = runtime.screen_ocr_elapsed_ms {
                ocr_elapsed_ms.push(u128::from(value));
                println!("round={} screen_ocr_elapsed_ms={value}", round + 1);
            }
            if let Some(value) = runtime.screen_ocr_first_token_ms {
                ocr_first_token_ms.push(u128::from(value));
                println!("round={} screen_ocr_first_token_ms={value}", round + 1);
            }
            if let Some(value) = runtime.screen_ocr_text_chars {
                ocr_text_chars.push(u128::from(value));
                println!("round={} screen_ocr_text_chars={value}", round + 1);
            }
        }
        elapsed_ms.push(elapsed);
        if let Some(value) = response.input_tokens {
            input_tokens.push(u128::from(value));
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
        avg(&elapsed_ms)
    };
    let min_input_tokens = input_tokens.iter().copied().min().unwrap_or(0);
    let max_input_tokens = input_tokens.iter().copied().max().unwrap_or(0);
    let avg_input_tokens = if input_tokens.is_empty() {
        0
    } else {
        avg(&input_tokens)
    };

    println!("rounds={rounds}");
    println!("screenshot_enabled={}", screenshot_data_url.is_some());
    println!("visual_context_mode={visual_context_mode:?}");
    println!("visual_capture_scope={visual_capture_scope:?}");
    println!("elapsed_min_ms={min_elapsed}");
    println!("elapsed_avg_ms={avg_elapsed}");
    println!("elapsed_max_ms={max_elapsed}");
    println!("input_tokens_min={min_input_tokens}");
    println!("input_tokens_avg={avg_input_tokens}");
    println!("input_tokens_max={max_input_tokens}");
    println!("visual_variant_count={}", visual_variants.len());
    if let Some(label) = visual_runtime_label(response.visual_context_runtime.as_ref()) {
        println!("visual_dispatch={label}");
    }
    println!(
        "capture_actual_scope={}",
        response
            .visual_context_runtime
            .as_ref()
            .and_then(|runtime| runtime.capture_actual_scope)
            .map(capture_scope_label)
            .unwrap_or("")
    );
    println!(
        "screenshot_capture_elapsed_ms={}",
        response
            .visual_context_runtime
            .as_ref()
            .and_then(|runtime| runtime.screenshot_capture_elapsed_ms)
            .unwrap_or_default()
    );
    println!(
        "capture_fallback_reason={}",
        response
            .visual_context_runtime
            .as_ref()
            .and_then(|runtime| runtime.capture_fallback_reason.as_deref())
            .unwrap_or("")
    );
    println!(
        "screen_ocr_source_variant_count={}",
        screen_ocr_source_variants.len()
    );
    println!(
        "screen_ocr_source={}",
        screen_ocr_source_label(response.visual_context_runtime.as_ref()).unwrap_or_default()
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
    println!("provider_elapsed_ms={}", response.elapsed_ms);
    println!(
        "provider_first_token_ms={}",
        response.first_token_ms.unwrap_or_default()
    );
    println!(
        "provider_input_tokens={}",
        response.input_tokens.unwrap_or_default()
    );
    println!(
        "provider_cached_input_tokens={}",
        response.cached_input_tokens.unwrap_or_default()
    );
    println!("preview_scope={preview_scope}");
    println!("force_defaults={force_defaults}");
    println!(
        "forced_profile_id={}",
        forced_profile_id
            .as_ref()
            .map(|value| value.0.to_string())
            .unwrap_or_default()
    );
    println!(
        "app_process_name={}",
        response.app_process_name.as_deref().unwrap_or("")
    );
    println!(
        "matched_profile_name={}",
        response.matched_profile_name.as_deref().unwrap_or("")
    );
    println!("provider_kind={}", response.provider_kind);
    println!("api_kind={}", response.api_kind);
    println!("model={}", response.model);
    println!("warning={}", response.warning.as_deref().unwrap_or(""));
    println!("config_path={}", config_path.display());
    println!("custom_vocabulary_enabled={}", custom_vocabulary.is_some());
    println!("selected_text_enabled={}", selected_text.is_some());
    println!("--- system_message ---\n{}", response.system_message);
    println!("--- user_message ---\n{}", response.user_message);
    println!("--- raw_output ---\n{}", response.raw_output);
    println!("--- final_output ---\n{}", response.final_output);

    Ok(())
}
