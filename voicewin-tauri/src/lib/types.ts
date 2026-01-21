export type ContextToggles = {
  use_clipboard: boolean;
  use_selected_text: boolean;
  use_window_context: boolean;
  use_custom_vocabulary: boolean;
  use_ocr: boolean;
};

export type GlobalDefaults = {
  enable_enhancement: boolean;
  prompt_id?: string | null;
  insert_mode: 'Paste' | 'PasteAndEnter';
  stt_provider: string;
  stt_model: string;
  language: string;
  llm_base_url: string;
  llm_model: string;
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
  insert_mode?: 'Paste' | 'PasteAndEnter';
  stt_provider?: string;
  stt_model?: string;
  language?: string;
  llm_base_url?: string;
  llm_model?: string;
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
  insert_mode?: 'Paste' | 'PasteAndEnter' | null;
  stt_provider?: string | null;
  stt_model?: string | null;
  language?: string | null;
  llm_base_url?: string | null;
  llm_model?: string | null;
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
  ts_unix_ms: number;
  app_process_name?: string | null;
  app_exe_path?: string | null;
  app_window_title?: string | null;
  text: string;
  stage: string;
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
      llm_base_url: p.overrides.llm_base_url ?? null,
      llm_model: p.overrides.llm_model ?? null,
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
      llm_base_url: p.overrides.llm_base_url ?? undefined,
      llm_model: p.overrides.llm_model ?? undefined,
      context: p.overrides.context ?? undefined,
    },
  };
}

export type ProviderStatus = {
  openai_api_key_present: boolean;
  elevenlabs_api_key_present: boolean;
};
