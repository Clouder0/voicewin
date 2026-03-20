export type VisualContextMode = 'off' | 'auto' | 'screenshot' | 'ocr';
export type VisualCaptureScope = 'display' | 'foreground_window';
export type VisualContextDispatch = 'off' | 'screenshot' | 'ocr';
export type ScreenOcrSource = 'inline' | 'prepared';
export type PlatformName = 'windows' | 'macos' | 'linux' | 'unknown';

export type PlatformCapabilities = {
  platform: PlatformName;
  foreground_app_identity: boolean;
  clipboard_context: boolean;
  selected_text_context: boolean;
  window_context: boolean;
  screenshot_capture: boolean;
  foreground_window_capture: boolean;
  auto_insert: boolean;
};

export type VisualContextRuntime = {
  mode: VisualContextMode;
  capture_scope: VisualCaptureScope;
  capture_actual_scope?: VisualCaptureScope | null;
  dispatch: VisualContextDispatch;
  screenshot_capture_elapsed_ms?: number | null;
  capture_fallback_reason?: string | null;
  screen_ocr_source?: ScreenOcrSource | null;
  screen_ocr_elapsed_ms?: number | null;
  screen_ocr_first_token_ms?: number | null;
  screen_ocr_text_chars?: number | null;
};

export type ContextToggles = {
  use_clipboard: boolean;
  use_selected_text: boolean;
  use_window_context: boolean;
  use_custom_vocabulary: boolean;
  visual_context_mode: VisualContextMode;
  visual_capture_scope: VisualCaptureScope;
};

export type LlmProviderKind = 'openai_compatible' | 'gemini';
export type LlmApiKind = 'chat_completions' | 'responses_sse' | 'stream_generate_content_sse';
export type LlmPreflightMode = 'off' | 'http_connect';
export type LlmReasoningEffort = 'minimal' | 'low' | 'medium' | 'high';

export type GlobalDefaults = {
  enable_enhancement: boolean;
  prompt_id?: string | null;
  insert_mode: 'Paste' | 'PasteAndEnter' | 'ShiftInsert';
  stt_provider: string;
  stt_model: string;
  language: string;
  llm_provider_kind: string;
  llm_base_url: string;
  llm_model: string;
  llm_api_kind: string;
  llm_preflight_mode: string;
  llm_preflight_delay_ms: number;
  screenshot_max_edge_px: number;
  llm_reasoning_effort?: string | null;
  microphone_device?: string | null;
  microphone_device_id?: string | null;
  history_enabled: boolean;
  context: ContextToggles;
};

export type PromptTemplate = {
  id: string;
  title: string;
  mode: 'Enhancer' | 'Assistant';
  prompt_text: string;
  trigger_words: string[];
};

export type PromptPreviewContextOverride = {
  clipboard?: string | null;
  selected_text?: string | null;
  window_context?: string | null;
  screenshot_data_url?: string | null;
};

// Rust serializes `AppMatcher` as an externally tagged enum.
// Example: { "ProcessNameEquals": "slack.exe" }
export type AppMatcherWire =
  | { ExePathEquals: string }
  | { ProcessNameEquals: string }
  | { WindowTitleContains: string };

// Rust serializes `PowerModeOverrides` as an object with optional fields.
export type PowerModeOverridesWire = {
  enable_enhancement?: boolean;
  prompt_id?: string;
  insert_mode?: 'Paste' | 'PasteAndEnter' | 'ShiftInsert';
  stt_provider?: string;
  stt_model?: string;
  language?: string;
  llm_provider_kind?: string;
  llm_base_url?: string;
  llm_model?: string;
  llm_api_kind?: string;
  llm_preflight_mode?: string;
  llm_reasoning_effort?: string;
  context?: Partial<ContextToggles>;
};

export type PowerModeProfileWire = {
  id: string;
  name: string;
  enabled: boolean;
  matchers: AppMatcherWire[];
  overrides: PowerModeOverridesWire;
};

export type AppMatcher =
  | { kind: 'ExePathEquals'; value: string }
  | { kind: 'ProcessNameEquals'; value: string }
  | { kind: 'WindowTitleContains'; value: string };

export type PowerModeOverrides = {
  enable_enhancement?: boolean | null;
  prompt_id?: string | null;
  insert_mode?: 'Paste' | 'PasteAndEnter' | 'ShiftInsert' | null;
  stt_provider?: string | null;
  stt_model?: string | null;
  language?: string | null;
  llm_provider_kind?: string | null;
  llm_base_url?: string | null;
  llm_model?: string | null;
  llm_api_kind?: string | null;
  llm_preflight_mode?: string | null;
  llm_reasoning_effort?: string | null;
  context?: Partial<ContextToggles> | null;
};

export type PowerModeProfile = {
  id: string;
  name: string;
  enabled: boolean;
  matchers: AppMatcher[];
  overrides: PowerModeOverrides;
};

export type AppConfig = {
  defaults: GlobalDefaults;
  profiles: PowerModeProfileWire[];
  prompts: PromptTemplate[];
  llm_api_key_present: boolean;
};

export type HistoryEntry = {
  id?: string | null;
  ts_unix_ms: number;
  app_process_name?: string | null;
  app_exe_path?: string | null;
  app_window_title?: string | null;
  text: string;
  raw_transcript?: string | null;
  enhanced_text?: string | null;
  prompt_id?: string | null;
  prompt_title?: string | null;
  matched_profile_name?: string | null;
  detected_trigger_word?: string | null;
  stt_provider?: string | null;
  stt_model?: string | null;
  llm_provider?: string | null;
  llm_model?: string | null;
  transcription_ms?: number | null;
  enhancement_ms?: number | null;
  enhancement_first_token_ms?: number | null;
  enhancement_input_tokens?: number | null;
  enhancement_cached_input_tokens?: number | null;
  context_flags?: ContextToggles | null;
  visual_context_runtime?: VisualContextRuntime | null;
  stage: string;
  warning?: string | null;
  error?: string | null;
};

export type PromptPreviewResponse = {
  elapsed_ms: number;
  first_token_ms?: number | null;
  input_tokens?: number | null;
  cached_input_tokens?: number | null;
  visual_context_runtime?: VisualContextRuntime | null;
  app_process_name?: string | null;
  app_window_title?: string | null;
  matched_profile_name?: string | null;
  provider_kind: string;
  api_kind: string;
  model: string;
  system_message: string;
  user_message: string;
  raw_output: string;
  final_output: string;
  warning?: string | null;
};

export function decodeAppMatcherWire(m: AppMatcherWire): AppMatcher {
  if ('ExePathEquals' in m) return { kind: 'ExePathEquals', value: m.ExePathEquals };
  if ('ProcessNameEquals' in m) return { kind: 'ProcessNameEquals', value: m.ProcessNameEquals };
  return { kind: 'WindowTitleContains', value: m.WindowTitleContains };
}

export function encodeAppMatcherWire(m: AppMatcher): AppMatcherWire {
  switch (m.kind) {
    case 'ExePathEquals':
      return { ExePathEquals: m.value };
    case 'ProcessNameEquals':
      return { ProcessNameEquals: m.value };
    case 'WindowTitleContains':
      return { WindowTitleContains: m.value };
  }
}

export function decodePowerModeProfile(p: PowerModeProfileWire): PowerModeProfile {
  return {
    id: p.id,
    name: p.name,
    enabled: p.enabled,
    matchers: p.matchers.map(decodeAppMatcherWire),
    overrides: {
      enable_enhancement: p.overrides.enable_enhancement ?? null,
      prompt_id: p.overrides.prompt_id ?? null,
      insert_mode: p.overrides.insert_mode ?? null,
      stt_provider: p.overrides.stt_provider ?? null,
      stt_model: p.overrides.stt_model ?? null,
      language: p.overrides.language ?? null,
      llm_provider_kind: p.overrides.llm_provider_kind ?? null,
      llm_base_url: p.overrides.llm_base_url ?? null,
      llm_model: p.overrides.llm_model ?? null,
      llm_api_kind: p.overrides.llm_api_kind ?? null,
      llm_preflight_mode: p.overrides.llm_preflight_mode ?? null,
      llm_reasoning_effort: p.overrides.llm_reasoning_effort ?? null,
      context: p.overrides.context ?? null,
    },
  };
}

export function encodePowerModeProfile(p: PowerModeProfile): PowerModeProfileWire {
  return {
    id: p.id,
    name: p.name,
    enabled: p.enabled,
    matchers: p.matchers.map(encodeAppMatcherWire),
    overrides: {
      enable_enhancement: p.overrides.enable_enhancement ?? undefined,
      prompt_id: p.overrides.prompt_id ?? undefined,
      insert_mode: p.overrides.insert_mode ?? undefined,
      stt_provider: p.overrides.stt_provider ?? undefined,
      stt_model: p.overrides.stt_model ?? undefined,
      language: p.overrides.language ?? undefined,
      llm_provider_kind: p.overrides.llm_provider_kind ?? undefined,
      llm_base_url: p.overrides.llm_base_url ?? undefined,
      llm_model: p.overrides.llm_model ?? undefined,
      llm_api_kind: p.overrides.llm_api_kind ?? undefined,
      llm_preflight_mode: p.overrides.llm_preflight_mode ?? undefined,
      llm_reasoning_effort: p.overrides.llm_reasoning_effort ?? undefined,
      context: p.overrides.context ?? undefined,
    },
  };
}

export type ProviderStatus = {
  openai_api_key_present: boolean;
  openai_api_key_error?: string | null;
  gemini_api_key_present: boolean;
  gemini_api_key_error?: string | null;
  elevenlabs_api_key_present: boolean;
  elevenlabs_api_key_error?: string | null;
};

export type ProviderProbeKind = 'smoke' | 'screenshot_product_name';

export type ProviderProbeRequest = {
  provider_kind: string;
  api_kind: string;
  base_url: string;
  model: string;
  reasoning_effort?: string | null;
  probe_kind?: ProviderProbeKind;
};

export type ProviderProbeResponse = {
  probe_kind: ProviderProbeKind;
  elapsed_ms: number;
  first_token_ms?: number | null;
  input_tokens?: number | null;
  cached_input_tokens?: number | null;
  provider_kind: string;
  api_kind: string;
  model: string;
  expected_output: string;
  final_output: string;
  warning?: string | null;
};
