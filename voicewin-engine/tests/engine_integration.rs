use std::sync::Arc;
use voicewin_core::context::{ScreenshotCaptureMetadata, VisualCaptureScope, VisualContextMode};
use voicewin_core::enhancement::{PromptMode, PromptTemplate};
use voicewin_core::llm::{
    VisualContextDispatch, screenshot_context_warning, visual_context_capture_unavailable_warning,
};
use voicewin_core::power_mode::{GlobalDefaults, PowerModeOverrides, PowerModeProfile};
use voicewin_core::types::{AppIdentity, InsertMode, ProfileId, PromptId};
use voicewin_engine::engine::{EngineConfig, VoicewinEngine};
use voicewin_engine::traits::{
    AppContextProvider, AudioInput, CapturedScreenshot, ContextSnapshot, EnhancedText, Inserter,
    LlmProvider, PreparedScreenOcr, ScreenshotCaptureOptions, SttProvider, Transcript,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct TestContext;

#[async_trait::async_trait]
impl AppContextProvider for TestContext {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        Ok(AppIdentity::new().with_process_name("slack.exe"))
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        Ok(ContextSnapshot {
            clipboard: Some("VOICE-123".into()),
            selected_text: None,
            window_context: Some("Application: Slack".into()),
            custom_vocabulary: Some("VoiceInk".into()),
            screenshot: None,
            screenshot_metadata: None,
            precomputed_screen_ocr: None,
        })
    }
}

struct TestInserter {
    inserted: Arc<std::sync::Mutex<Vec<(String, InsertMode)>>>,
}

#[async_trait::async_trait]
impl Inserter for TestInserter {
    async fn insert(&self, text: &str, mode: InsertMode) -> anyhow::Result<()> {
        self.inserted.lock().unwrap().push((text.to_string(), mode));
        Ok(())
    }
}

struct TestStt;

#[async_trait::async_trait]
impl SttProvider for TestStt {
    async fn transcribe(
        &self,
        _audio: &AudioInput,
        provider: &str,
        model: &str,
        _language: &str,
    ) -> anyhow::Result<Transcript> {
        Ok(Transcript {
            text: "rewrite um hello world rewrite".into(),
            provider: provider.into(),
            model: model.into(),
        })
    }
}

struct PanicStt;

#[async_trait::async_trait]
impl SttProvider for PanicStt {
    async fn transcribe(
        &self,
        _audio: &AudioInput,
        _provider: &str,
        _model: &str,
        _language: &str,
    ) -> anyhow::Result<Transcript> {
        panic!("STT should not be called when transcript override is provided")
    }
}

struct OpenAiCompatibleLlm;

#[async_trait::async_trait]
impl LlmProvider for OpenAiCompatibleLlm {
    async fn enhance(
        &self,
        provider_kind: &str,
        api_kind: &str,
        base_url: &str,
        api_key: &str,
        model: &str,
        reasoning_effort: Option<&str>,
        system_message: &str,
        user_message: &str,
        _attached_image: Option<&voicewin_core::context::ImageArtifact>,
    ) -> anyhow::Result<EnhancedText> {
        assert_eq!(provider_kind, "openai_compatible");
        assert_eq!(api_kind, "chat_completions");
        assert_eq!(reasoning_effort, None);
        let cfg = voicewin_providers::openai_compatible::OpenAiCompatibleChatConfig {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            reasoning_effort: reasoning_effort.map(ToOwned::to_owned),
        };

        let messages = vec![
            voicewin_providers::openai_compatible::ChatMessage {
                role: "system".into(),
                content: system_message.to_string(),
            },
            voicewin_providers::openai_compatible::ChatMessage {
                role: "user".into(),
                content: user_message.to_string(),
            },
        ];

        let req =
            voicewin_providers::openai_compatible::build_chat_completions_request(&cfg, &messages);
        let resp = voicewin_providers::runtime::execute(&req).await?;
        if !(200..=299).contains(&resp.status) {
            return Err(anyhow::anyhow!("bad status {}", resp.status));
        }

        let text = voicewin_providers::parse::parse_openai_chat_completion(&resp.body)?;
        Ok(EnhancedText {
            text,
            provider: "openai-compatible".into(),
            model: model.into(),
            first_token_ms: None,
            input_tokens: None,
            cached_input_tokens: None,
        })
    }
}

struct PanicLlm;

#[async_trait::async_trait]
impl LlmProvider for PanicLlm {
    async fn enhance(
        &self,
        _provider_kind: &str,
        _api_kind: &str,
        _base_url: &str,
        _api_key: &str,
        _model: &str,
        _reasoning_effort: Option<&str>,
        _system_message: &str,
        _user_message: &str,
        _attached_image: Option<&voicewin_core::context::ImageArtifact>,
    ) -> anyhow::Result<EnhancedText> {
        panic!("LLM should not be called when no API key is set")
    }
}

struct CapturingContext {
    screenshot: Option<voicewin_core::context::ImageArtifact>,
    screenshot_metadata: Option<ScreenshotCaptureMetadata>,
    capture_count: Arc<std::sync::atomic::AtomicUsize>,
    capture_options: Arc<std::sync::Mutex<Vec<ScreenshotCaptureOptions>>>,
}

#[async_trait::async_trait]
impl AppContextProvider for CapturingContext {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        Ok(AppIdentity::new().with_process_name("slack.exe"))
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        Ok(ContextSnapshot {
            clipboard: None,
            selected_text: None,
            window_context: Some("Application: Slack".into()),
            custom_vocabulary: None,
            screenshot: None,
            screenshot_metadata: None,
            precomputed_screen_ocr: None,
        })
    }

    async fn capture_screenshot(
        &self,
        options: ScreenshotCaptureOptions,
    ) -> anyhow::Result<Option<CapturedScreenshot>> {
        self.capture_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.capture_options.lock().unwrap().push(options);
        Ok(self.screenshot.clone().map(|image| CapturedScreenshot {
            image,
            metadata: self
                .screenshot_metadata
                .clone()
                .unwrap_or(ScreenshotCaptureMetadata {
                    actual_scope: Some(options.scope),
                    capture_elapsed_ms: None,
                    fallback_reason: None,
                }),
        }))
    }
}

struct PrecomputedOcrContext;

#[async_trait::async_trait]
impl AppContextProvider for PrecomputedOcrContext {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        Ok(AppIdentity::new().with_process_name("slack.exe"))
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        Ok(ContextSnapshot {
            window_context: Some("Application: Slack".into()),
            precomputed_screen_ocr: Some(PreparedScreenOcr {
                text: "VOICEWIN".into(),
                elapsed_ms: 42,
                first_token_ms: Some(21),
            }),
            ..Default::default()
        })
    }
}

struct RecordingLlm {
    seen_images: Arc<std::sync::Mutex<Vec<Option<String>>>>,
}

#[async_trait::async_trait]
impl LlmProvider for RecordingLlm {
    async fn enhance(
        &self,
        _provider_kind: &str,
        _api_kind: &str,
        _base_url: &str,
        _api_key: &str,
        model: &str,
        _reasoning_effort: Option<&str>,
        _system_message: &str,
        _user_message: &str,
        attached_image: Option<&voicewin_core::context::ImageArtifact>,
    ) -> anyhow::Result<EnhancedText> {
        self.seen_images
            .lock()
            .unwrap()
            .push(attached_image.map(|image| image.data_url.clone()));
        Ok(EnhancedText {
            text: "VoiceWin".into(),
            provider: "recording".into(),
            model: model.into(),
            first_token_ms: None,
            input_tokens: None,
            cached_input_tokens: None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InspectingCall {
    api_kind: String,
    system_message: String,
    user_message: String,
    attached_image: bool,
}

struct InspectingLlm {
    calls: Arc<std::sync::Mutex<Vec<InspectingCall>>>,
}

#[async_trait::async_trait]
impl LlmProvider for InspectingLlm {
    async fn enhance(
        &self,
        _provider_kind: &str,
        api_kind: &str,
        _base_url: &str,
        _api_key: &str,
        model: &str,
        _reasoning_effort: Option<&str>,
        system_message: &str,
        user_message: &str,
        attached_image: Option<&voicewin_core::context::ImageArtifact>,
    ) -> anyhow::Result<EnhancedText> {
        self.calls.lock().unwrap().push(InspectingCall {
            api_kind: api_kind.to_string(),
            system_message: system_message.to_string(),
            user_message: user_message.to_string(),
            attached_image: attached_image.is_some(),
        });
        Ok(EnhancedText {
            text: if attached_image.is_some() {
                "VOICEWIN".into()
            } else {
                "VoiceWin".into()
            },
            provider: "inspect".into(),
            model: model.into(),
            first_token_ms: None,
            input_tokens: None,
            cached_input_tokens: None,
        })
    }
}

#[tokio::test]
async fn end_to_end_session_uses_power_mode_and_llm() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"choices":[{"message":{"content":"Hello, world."}}]}"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let defaults = GlobalDefaults {
        enable_enhancement: true,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "local".into(),
        stt_model: "mock".into(),
        language: "en".into(),
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url: server.uri(),
        llm_model: "gpt-4o-mini".into(),
        llm_api_kind: "chat_completions".into(),
        llm_preflight_mode: "http_connect".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 1_280,
        llm_reasoning_effort: None,
        microphone_device: None,
        microphone_device_id: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    };

    let profile = PowerModeProfile {
        id: ProfileId::new(),
        name: "Slack".into(),
        enabled: true,
        matchers: vec![voicewin_core::power_mode::AppMatcher::ProcessNameEquals(
            "slack.exe".into(),
        )],
        overrides: PowerModeOverrides {
            insert_mode: Some(InsertMode::PasteAndEnter),
            ..Default::default()
        },
    };

    let prompts = vec![PromptTemplate {
        id: PromptId::new(),
        title: "Rewrite".into(),
        mode: PromptMode::Enhancer,
        prompt_text: "Clean up.".into(),
        trigger_words: vec!["rewrite".into()],
    }];

    let inserted = Arc::new(std::sync::Mutex::new(vec![]));

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles: vec![profile],
            prompts,
            openai_api_key: "k".into(),
            gemini_api_key: String::new(),
        },
        Arc::new(TestContext),
        Arc::new(TestStt),
        Arc::new(OpenAiCompatibleLlm),
        Arc::new(TestInserter {
            inserted: inserted.clone(),
        }),
    );

    let audio = AudioInput {
        sample_rate_hz: 16_000,
        samples: vec![0.0; 8],
    };

    let res = engine.run_session(audio).await.unwrap();
    assert_eq!(res.final_text.as_deref(), Some("Hello, world."));

    let inserted = inserted.lock().unwrap();
    assert_eq!(inserted.len(), 1);
    assert_eq!(inserted[0].0, "Hello, world.");
    assert_eq!(inserted[0].1, InsertMode::PasteAndEnter);
}

#[tokio::test]
async fn trigger_words_do_not_strip_without_llm_key() {
    let defaults = GlobalDefaults {
        // User enabled enhancement, but has not configured an API key.
        enable_enhancement: true,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "local".into(),
        stt_model: "mock".into(),
        language: "en".into(),
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url: "https://api.example.com/v1".into(),
        llm_model: "gpt-4o-mini".into(),
        llm_api_kind: "chat_completions".into(),
        llm_preflight_mode: "http_connect".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 1_280,
        llm_reasoning_effort: None,
        microphone_device: None,
        microphone_device_id: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    };

    let prompts = vec![PromptTemplate {
        id: PromptId::new(),
        title: "Rewrite".into(),
        mode: PromptMode::Enhancer,
        prompt_text: "Clean up.".into(),
        trigger_words: vec!["rewrite".into()],
    }];

    let inserted = Arc::new(std::sync::Mutex::new(vec![]));

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles: vec![],
            prompts,
            openai_api_key: String::new(),
            gemini_api_key: String::new(),
        },
        Arc::new(TestContext),
        Arc::new(TestStt),
        Arc::new(PanicLlm),
        Arc::new(TestInserter {
            inserted: inserted.clone(),
        }),
    );

    let audio = AudioInput {
        sample_rate_hz: 16_000,
        samples: vec![0.0; 8],
    };

    let res = engine.run_session(audio).await.unwrap();
    let text = res.final_text.as_deref().unwrap_or_default();
    assert!(
        text.contains("rewrite"),
        "trigger word should not be stripped when enhancement is unavailable"
    );
}

#[tokio::test]
async fn transcript_override_skips_stt_and_inserts() {
    let defaults = GlobalDefaults {
        enable_enhancement: false,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "elevenlabs".into(),
        stt_model: "scribe_v2_realtime".into(),
        language: "en".into(),
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url: "https://api.example.com/v1".into(),
        llm_model: "gpt-4o-mini".into(),
        llm_api_kind: "chat_completions".into(),
        llm_preflight_mode: "http_connect".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 1_280,
        llm_reasoning_effort: None,
        microphone_device: None,
        microphone_device_id: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    };

    let inserted = Arc::new(std::sync::Mutex::new(vec![]));

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles: vec![],
            prompts: vec![],
            openai_api_key: String::new(),
            gemini_api_key: String::new(),
        },
        Arc::new(TestContext),
        Arc::new(PanicStt),
        Arc::new(PanicLlm),
        Arc::new(TestInserter {
            inserted: inserted.clone(),
        }),
    );

    let res = engine
        .run_session_with_transcript_with_hook("hello world".into(), |_stage| async {})
        .await
        .unwrap();
    assert_eq!(res.final_text.as_deref(), Some("hello world"));

    let inserted = inserted.lock().unwrap();
    assert_eq!(inserted.len(), 1);
    assert_eq!(inserted[0].0, "hello world");
}

#[tokio::test]
async fn transcript_override_captures_screenshot_when_ocr_is_enabled() {
    let capture_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let capture_options = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_images = Arc::new(std::sync::Mutex::new(Vec::new()));
    let screenshot = voicewin_core::context::ImageArtifact {
        data_url: "data:image/png;base64,SGVsbG8=".into(),
    };

    let mut defaults = GlobalDefaults {
        enable_enhancement: true,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "elevenlabs".into(),
        stt_model: "scribe_v2_realtime".into(),
        language: "en".into(),
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url: "https://api.example.com/v1".into(),
        llm_model: "gpt-5.4".into(),
        llm_api_kind: "responses_sse".into(),
        llm_preflight_mode: "off".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 640,
        llm_reasoning_effort: None,
        microphone_device: None,
        microphone_device_id: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    };
    defaults.context.use_clipboard = false;
    defaults.context.use_selected_text = false;
    defaults.context.use_custom_vocabulary = false;
    defaults.context.visual_context_mode = VisualContextMode::Screenshot;

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Rewrite".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Clean up.".into(),
                trigger_words: vec![],
            }],
            openai_api_key: "sk-live".into(),
            gemini_api_key: String::new(),
        },
        Arc::new(CapturingContext {
            screenshot: Some(screenshot.clone()),
            screenshot_metadata: None,
            capture_count: capture_count.clone(),
            capture_options: capture_options.clone(),
        }),
        Arc::new(PanicStt),
        Arc::new(RecordingLlm {
            seen_images: seen_images.clone(),
        }),
        Arc::new(TestInserter {
            inserted: Arc::new(std::sync::Mutex::new(vec![])),
        }),
    );

    let response = engine
        .run_session_with_transcript_with_hook("voice wen".into(), |_stage| async {})
        .await
        .unwrap();

    assert_eq!(response.final_text.as_deref(), Some("VoiceWin"));
    assert_eq!(response.visual_context.mode, VisualContextMode::Screenshot);
    assert_eq!(
        response.visual_context.capture_scope,
        VisualCaptureScope::Display
    );
    assert_eq!(
        response.visual_context.dispatch,
        VisualContextDispatch::Screenshot
    );
    assert_eq!(response.visual_context.screen_ocr_elapsed_ms, None);
    assert_eq!(response.visual_context.screen_ocr_first_token_ms, None);
    assert_eq!(response.visual_context.screen_ocr_text_chars, None);
    assert_eq!(capture_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        capture_options.lock().unwrap().as_slice(),
        &[ScreenshotCaptureOptions {
            max_edge_px: 640,
            scope: VisualCaptureScope::Display,
        }]
    );
    assert_eq!(
        seen_images.lock().unwrap().as_slice(),
        &[Some(screenshot.data_url)]
    );
}

#[tokio::test]
async fn transcript_override_skips_screenshot_capture_when_visual_context_is_disabled() {
    let capture_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let capture_options = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_images = Arc::new(std::sync::Mutex::new(Vec::new()));

    let mut defaults = GlobalDefaults {
        enable_enhancement: true,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "elevenlabs".into(),
        stt_model: "scribe_v2_realtime".into(),
        language: "en".into(),
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url: "https://api.example.com/v1".into(),
        llm_model: "gpt-5.4".into(),
        llm_api_kind: "responses_sse".into(),
        llm_preflight_mode: "off".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 1_280,
        llm_reasoning_effort: None,
        microphone_device: None,
        microphone_device_id: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    };
    defaults.context.use_clipboard = false;
    defaults.context.use_selected_text = false;
    defaults.context.use_custom_vocabulary = false;
    defaults.context.visual_context_mode = VisualContextMode::Off;

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Rewrite".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Clean up.".into(),
                trigger_words: vec![],
            }],
            openai_api_key: "sk-live".into(),
            gemini_api_key: String::new(),
        },
        Arc::new(CapturingContext {
            screenshot: Some(voicewin_core::context::ImageArtifact {
                data_url: "data:image/png;base64,SGVsbG8=".into(),
            }),
            screenshot_metadata: Some(ScreenshotCaptureMetadata {
                actual_scope: Some(VisualCaptureScope::Display),
                capture_elapsed_ms: Some(17),
                fallback_reason: Some("no_foreground_window".into()),
            }),
            capture_count: capture_count.clone(),
            capture_options: capture_options.clone(),
        }),
        Arc::new(PanicStt),
        Arc::new(RecordingLlm {
            seen_images: seen_images.clone(),
        }),
        Arc::new(TestInserter {
            inserted: Arc::new(std::sync::Mutex::new(vec![])),
        }),
    );

    let response = engine
        .run_session_with_transcript_with_hook("voice wen".into(), |_stage| async {})
        .await
        .unwrap();

    assert_eq!(response.final_text.as_deref(), Some("VoiceWin"));
    assert_eq!(capture_count.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(capture_options.lock().unwrap().is_empty());
    assert_eq!(seen_images.lock().unwrap().as_slice(), &[None]);
}

#[tokio::test]
async fn transcript_override_skips_screenshot_capture_when_api_cannot_attach_images() {
    let capture_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let capture_options = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_images = Arc::new(std::sync::Mutex::new(Vec::new()));

    let mut defaults = GlobalDefaults {
        enable_enhancement: true,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "elevenlabs".into(),
        stt_model: "scribe_v2_realtime".into(),
        language: "en".into(),
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url: "https://api.example.com/v1".into(),
        llm_model: "gpt-5.4".into(),
        llm_api_kind: "chat_completions".into(),
        llm_preflight_mode: "off".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 1_280,
        llm_reasoning_effort: None,
        microphone_device: None,
        microphone_device_id: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    };
    defaults.context.use_clipboard = false;
    defaults.context.use_selected_text = false;
    defaults.context.use_custom_vocabulary = false;
    defaults.context.visual_context_mode = VisualContextMode::Screenshot;

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Rewrite".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Clean up.".into(),
                trigger_words: vec![],
            }],
            openai_api_key: "sk-live".into(),
            gemini_api_key: String::new(),
        },
        Arc::new(CapturingContext {
            screenshot: Some(voicewin_core::context::ImageArtifact {
                data_url: "data:image/png;base64,SGVsbG8=".into(),
            }),
            screenshot_metadata: Some(ScreenshotCaptureMetadata {
                actual_scope: Some(VisualCaptureScope::Display),
                capture_elapsed_ms: Some(17),
                fallback_reason: Some("no_foreground_window".into()),
            }),
            capture_count: capture_count.clone(),
            capture_options: capture_options.clone(),
        }),
        Arc::new(PanicStt),
        Arc::new(RecordingLlm {
            seen_images: seen_images.clone(),
        }),
        Arc::new(TestInserter {
            inserted: Arc::new(std::sync::Mutex::new(vec![])),
        }),
    );

    let response = engine
        .run_session_with_transcript_with_hook("voice wen".into(), |_stage| async {})
        .await
        .unwrap();

    assert_eq!(response.final_text.as_deref(), Some("VoiceWin"));
    assert_eq!(capture_count.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(capture_options.lock().unwrap().is_empty());
    assert_eq!(seen_images.lock().unwrap().as_slice(), &[None]);
    assert_eq!(
        response.warning.as_deref(),
        screenshot_context_warning("openai_compatible", "chat_completions").as_deref()
    );
}

#[tokio::test]
async fn transcript_override_uses_ocr_sidecar_for_auto_mode_on_text_only_api() {
    let capture_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let capture_options = Arc::new(std::sync::Mutex::new(Vec::new()));
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));

    let mut defaults = GlobalDefaults {
        enable_enhancement: true,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "elevenlabs".into(),
        stt_model: "scribe_v2_realtime".into(),
        language: "en".into(),
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url: "https://api.example.com/v1".into(),
        llm_model: "gpt-5.4".into(),
        llm_api_kind: "chat_completions".into(),
        llm_preflight_mode: "off".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 640,
        llm_reasoning_effort: None,
        microphone_device: None,
        microphone_device_id: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    };
    defaults.context.use_clipboard = false;
    defaults.context.use_selected_text = false;
    defaults.context.use_custom_vocabulary = false;
    defaults.context.visual_context_mode = VisualContextMode::Auto;
    defaults.context.visual_capture_scope = VisualCaptureScope::ForegroundWindow;

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Rewrite".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Clean up.".into(),
                trigger_words: vec![],
            }],
            openai_api_key: "sk-live".into(),
            gemini_api_key: String::new(),
        },
        Arc::new(CapturingContext {
            screenshot: Some(voicewin_core::context::ImageArtifact {
                data_url: "data:image/png;base64,SGVsbG8=".into(),
            }),
            screenshot_metadata: Some(ScreenshotCaptureMetadata {
                actual_scope: Some(VisualCaptureScope::Display),
                capture_elapsed_ms: Some(17),
                fallback_reason: Some("no_foreground_window".into()),
            }),
            capture_count: capture_count.clone(),
            capture_options: capture_options.clone(),
        }),
        Arc::new(PanicStt),
        Arc::new(InspectingLlm {
            calls: calls.clone(),
        }),
        Arc::new(TestInserter {
            inserted: Arc::new(std::sync::Mutex::new(vec![])),
        }),
    );

    let response = engine
        .run_session_with_transcript_with_hook("voice wen".into(), |_stage| async {})
        .await
        .unwrap();

    assert_eq!(response.final_text.as_deref(), Some("VoiceWin"));
    assert_eq!(response.visual_context.mode, VisualContextMode::Auto);
    assert_eq!(
        response.visual_context.capture_scope,
        VisualCaptureScope::ForegroundWindow
    );
    assert_eq!(
        response.visual_context.capture_actual_scope,
        Some(VisualCaptureScope::Display)
    );
    assert_eq!(
        response.visual_context.screenshot_capture_elapsed_ms,
        Some(17)
    );
    assert_eq!(
        response.visual_context.capture_fallback_reason.as_deref(),
        Some("no_foreground_window")
    );
    assert_eq!(response.visual_context.dispatch, VisualContextDispatch::Ocr);
    assert!(response.visual_context.screen_ocr_elapsed_ms.is_some());
    assert_eq!(response.visual_context.screen_ocr_first_token_ms, None);
    assert_eq!(response.visual_context.screen_ocr_text_chars, Some(8));
    assert_eq!(capture_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        capture_options.lock().unwrap().as_slice(),
        &[ScreenshotCaptureOptions {
            max_edge_px: 640,
            scope: VisualCaptureScope::ForegroundWindow,
        }]
    );

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].api_kind, "responses_sse");
    assert!(calls[0].attached_image);
    assert!(calls[0].user_message.contains("OCR_TASK"));
    assert_eq!(calls[1].api_kind, "chat_completions");
    assert!(!calls[1].attached_image);
    assert!(calls[1].system_message.contains("<SCREEN_OCR_TEXT>"));
    assert!(calls[1].system_message.contains("VOICEWIN"));
}

#[tokio::test]
async fn transcript_override_reuses_precomputed_ocr_from_context_snapshot() {
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));

    let mut defaults = GlobalDefaults {
        enable_enhancement: true,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "elevenlabs".into(),
        stt_model: "scribe_v2_realtime".into(),
        language: "en".into(),
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url: "https://api.example.com/v1".into(),
        llm_model: "gpt-5.4".into(),
        llm_api_kind: "chat_completions".into(),
        llm_preflight_mode: "off".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 640,
        llm_reasoning_effort: None,
        microphone_device: None,
        microphone_device_id: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    };
    defaults.context.use_clipboard = false;
    defaults.context.use_selected_text = false;
    defaults.context.use_custom_vocabulary = false;
    defaults.context.visual_context_mode = VisualContextMode::Ocr;
    defaults.context.visual_capture_scope = VisualCaptureScope::ForegroundWindow;

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Rewrite".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Clean up.".into(),
                trigger_words: vec![],
            }],
            openai_api_key: "sk-live".into(),
            gemini_api_key: String::new(),
        },
        Arc::new(PrecomputedOcrContext),
        Arc::new(PanicStt),
        Arc::new(InspectingLlm {
            calls: calls.clone(),
        }),
        Arc::new(TestInserter {
            inserted: Arc::new(std::sync::Mutex::new(vec![])),
        }),
    );

    let response = engine
        .run_session_with_transcript_with_hook("voice wen".into(), |_stage| async {})
        .await
        .unwrap();

    assert_eq!(response.final_text.as_deref(), Some("VoiceWin"));
    assert_eq!(response.visual_context.mode, VisualContextMode::Ocr);
    assert_eq!(
        response.visual_context.capture_scope,
        VisualCaptureScope::ForegroundWindow
    );
    assert_eq!(response.visual_context.dispatch, VisualContextDispatch::Ocr);
    assert_eq!(response.visual_context.screen_ocr_elapsed_ms, Some(42));
    assert_eq!(response.visual_context.screen_ocr_first_token_ms, Some(21));
    assert_eq!(response.visual_context.screen_ocr_text_chars, Some(8));

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].api_kind, "chat_completions");
    assert!(!calls[0].attached_image);
    assert!(calls[0].system_message.contains("<SCREEN_OCR_TEXT>"));
    assert!(calls[0].system_message.contains("VOICEWIN"));
}

#[tokio::test]
async fn transcript_override_warns_when_visual_capture_produces_no_screenshot() {
    let capture_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let capture_options = Arc::new(std::sync::Mutex::new(Vec::new()));

    let mut defaults = GlobalDefaults {
        enable_enhancement: true,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "elevenlabs".into(),
        stt_model: "scribe_v2_realtime".into(),
        language: "en".into(),
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url: "https://api.example.com/v1".into(),
        llm_model: "gpt-5.4".into(),
        llm_api_kind: "responses_sse".into(),
        llm_preflight_mode: "off".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 640,
        llm_reasoning_effort: None,
        microphone_device: None,
        microphone_device_id: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    };
    defaults.context.use_clipboard = false;
    defaults.context.use_selected_text = false;
    defaults.context.use_custom_vocabulary = false;
    defaults.context.visual_context_mode = VisualContextMode::Screenshot;
    defaults.context.visual_capture_scope = VisualCaptureScope::ForegroundWindow;

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Rewrite".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Clean up.".into(),
                trigger_words: vec![],
            }],
            openai_api_key: "sk-live".into(),
            gemini_api_key: String::new(),
        },
        Arc::new(CapturingContext {
            screenshot: None,
            screenshot_metadata: None,
            capture_count: capture_count.clone(),
            capture_options: capture_options.clone(),
        }),
        Arc::new(PanicStt),
        Arc::new(RecordingLlm {
            seen_images: Arc::new(std::sync::Mutex::new(vec![])),
        }),
        Arc::new(TestInserter {
            inserted: Arc::new(std::sync::Mutex::new(vec![])),
        }),
    );

    let response = engine
        .run_session_with_transcript_with_hook("voice wen".into(), |_stage| async {})
        .await
        .unwrap();

    assert_eq!(response.final_text.as_deref(), Some("VoiceWin"));
    assert_eq!(
        response.warning.as_deref(),
        visual_context_capture_unavailable_warning(
            VisualContextDispatch::Screenshot,
            VisualCaptureScope::ForegroundWindow,
        )
        .as_deref()
    );
    assert_eq!(response.visual_context.dispatch, VisualContextDispatch::Off);
    assert_eq!(capture_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        capture_options.lock().unwrap().as_slice(),
        &[ScreenshotCaptureOptions {
            max_edge_px: 640,
            scope: VisualCaptureScope::ForegroundWindow,
        }]
    );
}

#[tokio::test]
async fn transcript_override_empty_is_failure() {
    let defaults = GlobalDefaults {
        enable_enhancement: false,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "elevenlabs".into(),
        stt_model: "scribe_v2_realtime".into(),
        language: "en".into(),
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url: "https://api.example.com/v1".into(),
        llm_model: "gpt-4o-mini".into(),
        llm_api_kind: "chat_completions".into(),
        llm_preflight_mode: "http_connect".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 1_280,
        llm_reasoning_effort: None,
        microphone_device: None,
        microphone_device_id: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    };

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles: vec![],
            prompts: vec![],
            openai_api_key: String::new(),
            gemini_api_key: String::new(),
        },
        Arc::new(TestContext),
        Arc::new(PanicStt),
        Arc::new(PanicLlm),
        Arc::new(TestInserter {
            inserted: Arc::new(std::sync::Mutex::new(vec![])),
        }),
    );

    let res = engine
        .run_session_with_transcript_with_hook("   ".into(), |_stage| async {})
        .await
        .unwrap();
    assert_eq!(res.stage_label.as_deref(), Some("failed"));
    assert!(
        res.error
            .as_deref()
            .unwrap_or_default()
            .contains("No speech detected")
    );
}
