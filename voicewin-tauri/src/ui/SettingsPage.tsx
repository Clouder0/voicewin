import { useCallback, useEffect, useMemo, useState } from 'react';

import type {
  AppConfig,
  ContextToggles,
  LlmApiKind,
  LlmProviderKind,
  LlmPreflightMode,
  PlatformCapabilities,
  ProviderProbeKind,
  LlmReasoningEffort,
  ProviderProbeResponse,
  ProviderStatus,
  VisualCaptureScope,
  VisualContextMode,
} from '../lib/types';
import { formatBenchmarkCountRange, formatBenchmarkLatency, summarizeNumbers } from './llmBenchmark';
import {
  defaultBaseUrlForProvider,
  defaultModelForProvider,
  llmSupportsAttachedImages,
  looksLikeUnknownModelError,
  normalizeLlmApiKind,
  normalizeLlmPreflightDelayMs,
  normalizeLlmPreflightMode,
  normalizeLlmProviderKind,
  normalizeLlmReasoningEffort,
  normalizeScreenshotMaxEdgePx,
  normalizeVisualCaptureScope,
  normalizeVisualContextMode,
  recommendedCompactScreenshotMaxEdgePxForCurrentGateway,
  recommendedGeminiBaseUrlForCurrentGateway,
  recommendedApiKindForProvider,
  resolveVisualContextDispatch,
  screenshotContextWarning,
  shouldRecommendCompactScreenshotForLatency,
  shouldRecommendGeminiForScreenshotContext,
  shouldRecommendResponsesForCc2OpenAiChatCompletions,
} from './llmConfig';
import {
  contextCapabilityWarnings,
  fallbackPlatformCapabilities,
  loadPlatformCapabilities,
} from './platformCapabilities';

type ModelStatus = {
  bootstrap_ok: boolean;
  bootstrap_path: string;
  preferred_ok: boolean;
  preferred_path: string;
};

type SettingsDraft = {
  enable_enhancement: boolean;
  prompt_id: string;
  context: ContextToggles;
  llm_provider_kind: LlmProviderKind;
  llm_base_url: string;
  llm_model: string;
  llm_api_kind: LlmApiKind;
  llm_preflight_mode: LlmPreflightMode;
  llm_preflight_delay_ms: number;
  screenshot_max_edge_px: number;
  llm_reasoning_effort: '' | LlmReasoningEffort;
  stt_provider: 'local' | 'elevenlabs';
  local_stt_model_path: string;
  elevenlabs_stt_model: 'scribe_v2' | 'scribe_v2_realtime';
};

type ProviderProbeBenchmark = {
  probe_kind: ProviderProbeKind;
  rounds: number;
  provider_kind: string;
  api_kind: string;
  model: string;
  elapsed_min_ms: number;
  elapsed_avg_ms: number;
  elapsed_max_ms: number;
  first_token_min_ms: number | null;
  first_token_avg_ms: number | null;
  first_token_max_ms: number | null;
  input_tokens_min: number | null;
  input_tokens_avg: number | null;
  input_tokens_max: number | null;
  cached_input_tokens_min: number | null;
  cached_input_tokens_avg: number | null;
  cached_input_tokens_max: number | null;
  warning_count: number;
  mismatch_count: number;
  output_variant_count: number;
  expected_output: string;
  last_output: string;
  sample_warning: string | null;
};

function buildProviderProbeBenchmark(rounds: ProviderProbeResponse[]): ProviderProbeBenchmark {
  const last = rounds[rounds.length - 1];
  const elapsed = summarizeNumbers(rounds.map((round) => round.elapsed_ms));
  const firstToken = summarizeNumbers(
    rounds
      .map((round) => round.first_token_ms)
      .filter((value): value is number => value != null),
  );
  const inputTokens = summarizeNumbers(
    rounds
      .map((round) => round.input_tokens)
      .filter((value): value is number => value != null),
  );
  const cachedInputTokens = summarizeNumbers(
    rounds
      .map((round) => round.cached_input_tokens)
      .filter((value): value is number => value != null),
  );

  return {
    probe_kind: last.probe_kind,
    rounds: rounds.length,
    provider_kind: last.provider_kind,
    api_kind: last.api_kind,
    model: last.model,
    elapsed_min_ms: elapsed?.min ?? 0,
    elapsed_avg_ms: elapsed?.avg ?? 0,
    elapsed_max_ms: elapsed?.max ?? 0,
    first_token_min_ms: firstToken?.min ?? null,
    first_token_avg_ms: firstToken?.avg ?? null,
    first_token_max_ms: firstToken?.max ?? null,
    input_tokens_min: inputTokens?.min ?? null,
    input_tokens_avg: inputTokens?.avg ?? null,
    input_tokens_max: inputTokens?.max ?? null,
    cached_input_tokens_min: cachedInputTokens?.min ?? null,
    cached_input_tokens_avg: cachedInputTokens?.avg ?? null,
    cached_input_tokens_max: cachedInputTokens?.max ?? null,
    warning_count: rounds.filter((round) => Boolean(round.warning)).length,
    mismatch_count: rounds.filter((round) => round.final_output !== round.expected_output).length,
    output_variant_count: new Set(rounds.map((round) => round.final_output)).size,
    expected_output: last.expected_output,
    last_output: last.final_output,
    sample_warning: rounds.find((round) => round.warning)?.warning ?? null,
  };
}

function formatProbeLatency(probe: ProviderProbeResponse): string {
  const parts = [`${probe.elapsed_ms} ms total`];
  if (probe.first_token_ms != null) {
    parts.push(`${probe.first_token_ms} ms first token`);
  }
  if (probe.input_tokens != null) {
    parts.push(`${probe.input_tokens} input tok`);
  }
  if (probe.cached_input_tokens != null) {
    parts.push(`${probe.cached_input_tokens} cached tok`);
  }
  return parts.join(' • ');
}

function formatProbeKind(kind: ProviderProbeKind): string {
  return kind === 'screenshot_product_name' ? 'screenshot probe' : 'smoke probe';
}

const EMPTY_PROVIDER_STATUS: ProviderStatus = {
  openai_api_key_present: false,
  openai_api_key_error: null,
  gemini_api_key_present: false,
  gemini_api_key_error: null,
  elevenlabs_api_key_present: false,
  elevenlabs_api_key_error: null,
};

function draftFromConfig(cfg: AppConfig, modelStatus: ModelStatus | null): SettingsDraft {
  const localDefault = modelStatus?.preferred_ok
    ? modelStatus.preferred_path
    : modelStatus?.bootstrap_path ?? '';

  const isLocal = cfg.defaults.stt_provider === 'local';
  const currentEleven = cfg.defaults.stt_provider === 'elevenlabs' ? cfg.defaults.stt_model : 'scribe_v2';
  const normalizedEleven = currentEleven === 'scribe_v2_realtime' ? 'scribe_v2_realtime' : 'scribe_v2';
  const llmProviderKind = normalizeLlmProviderKind(cfg.defaults.llm_provider_kind);

  return {
    enable_enhancement: Boolean(cfg.defaults.enable_enhancement),
    prompt_id: cfg.defaults.prompt_id ?? '',
    context: {
      ...cfg.defaults.context,
      visual_context_mode: normalizeVisualContextMode(cfg.defaults.context.visual_context_mode),
      visual_capture_scope: normalizeVisualCaptureScope(cfg.defaults.context.visual_capture_scope),
    },
    llm_provider_kind: llmProviderKind,
    llm_base_url: cfg.defaults.llm_base_url ?? '',
    llm_model: cfg.defaults.llm_model ?? '',
    llm_api_kind: normalizeLlmApiKind(cfg.defaults.llm_api_kind, llmProviderKind),
    llm_preflight_mode: normalizeLlmPreflightMode(cfg.defaults.llm_preflight_mode),
    llm_preflight_delay_ms: normalizeLlmPreflightDelayMs(cfg.defaults.llm_preflight_delay_ms),
    screenshot_max_edge_px: normalizeScreenshotMaxEdgePx(cfg.defaults.screenshot_max_edge_px),
    llm_reasoning_effort: normalizeLlmReasoningEffort(cfg.defaults.llm_reasoning_effort),
    stt_provider: (cfg.defaults.stt_provider === 'elevenlabs' ? 'elevenlabs' : 'local') as 'local' | 'elevenlabs',
    local_stt_model_path: isLocal ? cfg.defaults.stt_model : localDefault,
    elevenlabs_stt_model: normalizedEleven as 'scribe_v2' | 'scribe_v2_realtime',
  };
}

function buildConfigFromDraft(cfg: AppConfig, draft: SettingsDraft, modelStatus: ModelStatus | null): AppConfig {
  return {
    ...cfg,
    defaults: {
      ...cfg.defaults,
      enable_enhancement: Boolean(draft.enable_enhancement),
      prompt_id: draft.prompt_id || null,
      context: { ...draft.context },
      llm_provider_kind: draft.llm_provider_kind,
      llm_base_url: draft.llm_base_url.trim() || cfg.defaults.llm_base_url,
      llm_model: draft.llm_model.trim() || cfg.defaults.llm_model,
      llm_api_kind: draft.llm_api_kind,
      llm_preflight_mode: draft.llm_preflight_mode,
      llm_preflight_delay_ms: normalizeLlmPreflightDelayMs(draft.llm_preflight_delay_ms),
      screenshot_max_edge_px: normalizeScreenshotMaxEdgePx(draft.screenshot_max_edge_px),
      llm_reasoning_effort: draft.llm_reasoning_effort || null,
      stt_provider: draft.stt_provider,
      stt_model:
        draft.stt_provider === 'local'
          ? (draft.local_stt_model_path.trim() ||
              (modelStatus?.preferred_ok
                ? modelStatus.preferred_path
                : modelStatus?.bootstrap_path ?? cfg.defaults.stt_model))
          : draft.elevenlabs_stt_model,
    },
  };
}

function SettingRow({
  title,
  description,
  right,
}: {
  title: string;
  description?: string;
  right: React.ReactNode;
}) {
  return (
    <div className="vw-settingRow">
      <div className="vw-settingRowLeft">
        <div className="vw-type-bodyStrong">{title}</div>
        {description ? (
          <div className="vw-type-caption" style={{ marginTop: 4 }}>
            {description}
          </div>
        ) : null}
      </div>
      <div className="vw-settingRowRight">{right}</div>
    </div>
  );
}

function Section({ title, subtitle, children }: { title: string; subtitle?: string; children: React.ReactNode }) {
  return (
    <div style={{ marginTop: 'var(--space-16)' }}>
      <div className="vw-type-bodyStrong">{title}</div>
      {subtitle ? (
        <div className="vw-type-caption" style={{ marginTop: 4 }}>
          {subtitle}
        </div>
      ) : null}
      <div className="vw-card" style={{ marginTop: 'var(--space-12)', padding: 0, overflow: 'hidden' }}>
        <div style={{ display: 'grid' }}>{children}</div>
      </div>
    </div>
  );
}

export function SettingsPage() {
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [providers, setProviders] = useState<ProviderStatus | null>(null);
  const [modelStatus, setModelStatus] = useState<ModelStatus | null>(null);
  const [platformCapabilities, setPlatformCapabilities] = useState<PlatformCapabilities>(
    fallbackPlatformCapabilities(),
  );
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // Inline key feedback so failures aren't "offscreen" at the page header.
  const [elevenKeyNotice, setElevenKeyNotice] = useState<string | null>(null);
  const [elevenKeyError, setElevenKeyError] = useState<string | null>(null);
  const [openaiKeyNotice, setOpenaiKeyNotice] = useState<string | null>(null);
  const [openaiKeyError, setOpenaiKeyError] = useState<string | null>(null);
  const [geminiKeyNotice, setGeminiKeyNotice] = useState<string | null>(null);
  const [geminiKeyError, setGeminiKeyError] = useState<string | null>(null);
  const [providerProbe, setProviderProbe] = useState<ProviderProbeResponse | null>(null);
  const [providerProbeError, setProviderProbeError] = useState<string | null>(null);
  const [probingProviderKind, setProbingProviderKind] = useState<ProviderProbeKind | null>(null);
  const [providerBenchmark, setProviderBenchmark] = useState<ProviderProbeBenchmark | null>(null);
  const [providerBenchmarkError, setProviderBenchmarkError] = useState<string | null>(null);
  const [benchmarkingProviderKind, setBenchmarkingProviderKind] = useState<ProviderProbeKind | null>(null);

  const [dirty, setDirty] = useState(false);
  const [draft, setDraft] = useState<SettingsDraft>({
    enable_enhancement: false,
    prompt_id: '',
    context: {
      use_clipboard: false,
      use_selected_text: false,
      use_window_context: false,
      use_custom_vocabulary: false,
      visual_context_mode: 'off',
      visual_capture_scope: 'display',
    },
    llm_provider_kind: 'openai_compatible',
    llm_base_url: '',
    llm_model: '',
    llm_api_kind: 'responses_sse',
    llm_preflight_mode: 'off',
    llm_preflight_delay_ms: 1500,
    screenshot_max_edge_px: 1280,
    llm_reasoning_effort: '',

    stt_provider: 'local',
    local_stt_model_path: '',
    elevenlabs_stt_model: 'scribe_v2',
  });

  const [openaiApiKeyDraft, setOpenaiApiKeyDraft] = useState('');
  const [geminiApiKeyDraft, setGeminiApiKeyDraft] = useState('');
  const [elevenApiKeyDraft, setElevenApiKeyDraft] = useState('');

  const refresh = useCallback(async () => {
    try {
      const { isTauri, invoke } = await import('@tauri-apps/api/core');
      if (!isTauri()) return;

      const nextCfg = await invoke<AppConfig>('get_config');
      setCfg(nextCfg);

      const warnings: string[] = [];

      try {
        const nextProviders = await invoke<ProviderStatus>('get_provider_status');
        setProviders(nextProviders);
      } catch (e) {
        warnings.push(`Provider status unavailable: ${String(e)}`);
        setProviders((prev) => prev ?? EMPTY_PROVIDER_STATUS);
      }

      try {
        const nextModelStatus = await invoke<ModelStatus>('get_model_status');
        setModelStatus(nextModelStatus);
      } catch (e) {
        warnings.push(`Model status unavailable: ${String(e)}`);
        setModelStatus(null);
      }

      setPlatformCapabilities(await loadPlatformCapabilities(invoke));

      setError(warnings.length > 0 ? warnings.join(' | ') : null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const saveConfig = useCallback(
    async (nextCfg: AppConfig, options?: { manageSaving?: boolean }): Promise<boolean> => {
      const manageSaving = options?.manageSaving ?? true;
      if (manageSaving) {
        setSaving(true);
      }
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('set_config', { cfg: nextCfg });
        setCfg(nextCfg);
        setError(null);
        // Refresh so we pick up key-present and any backend normalization.
        await refresh();
        return true;
      } catch (e) {
        setError(String(e));
        return false;
      } finally {
        if (manageSaving) {
          setSaving(false);
        }
      }
    },
    [refresh],
  );

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!cfg) return;
    // Only overwrite drafts when the user has no pending edits.
    if (dirty) return;

    setDraft(draftFromConfig(cfg, modelStatus));
  }, [cfg, dirty, modelStatus]);

  const openaiKeyStatus = useMemo(() => {
    if (!providers) return 'Unknown';
    if (providers.openai_api_key_error) return 'Unavailable';
    return providers.openai_api_key_present ? 'Set' : 'Not set';
  }, [providers]);

  const openaiKeyStatusError = useMemo(() => {
    return providers?.openai_api_key_error ?? null;
  }, [providers]);

  const geminiKeyStatus = useMemo(() => {
    if (!providers) return 'Unknown';
    if (providers.gemini_api_key_error) return 'Unavailable';
    return providers.gemini_api_key_present ? 'Set' : 'Not set';
  }, [providers]);

  const geminiKeyStatusError = useMemo(() => {
    return providers?.gemini_api_key_error ?? null;
  }, [providers]);

  const elevenKeyStatus = useMemo(() => {
    if (!providers) return 'Unknown';
    if (providers.elevenlabs_api_key_error) return 'Unavailable';
    return providers.elevenlabs_api_key_present ? 'Set' : 'Not set';
  }, [providers]);

  const elevenKeyStatusError = useMemo(() => {
    return providers?.elevenlabs_api_key_error ?? null;
  }, [providers]);

  const openaiApiKeyDraftTrimmed = openaiApiKeyDraft.trim();
  const geminiApiKeyDraftTrimmed = geminiApiKeyDraft.trim();
  const elevenApiKeyDraftTrimmed = elevenApiKeyDraft.trim();

  const localModelStatus = useMemo(() => {
    if (!modelStatus) return 'Unknown';
    if (modelStatus.preferred_ok) return 'Ready (Preferred)';
    if (modelStatus.bootstrap_ok) return 'Ready (Bootstrap)';
    return 'Missing';
  }, [modelStatus]);

  const localModelStatusColor = useMemo(() => {
    if (!modelStatus) return 'var(--text-secondary)';
    if (modelStatus.preferred_ok || modelStatus.bootstrap_ok) return 'var(--color-success-fg)';
    return 'var(--color-danger-fg)';
  }, [modelStatus]);

  const selectedLlmApiKeyPresent = useMemo(() => {
    if (!providers) return false;
    return draft.llm_provider_kind === 'gemini'
      ? providers.gemini_api_key_present
      : providers.openai_api_key_present;
  }, [draft.llm_provider_kind, providers]);

  const probingProvider = probingProviderKind != null;
  const benchmarkingProvider = benchmarkingProviderKind != null;
  const providerDiagnosticError = providerProbeError ?? providerBenchmarkError;

  useEffect(() => {
    setProviderProbe(null);
    setProviderProbeError(null);
    setProviderBenchmark(null);
    setProviderBenchmarkError(null);
  }, [
    draft.llm_provider_kind,
    draft.llm_api_kind,
    draft.llm_base_url,
    draft.llm_model,
    draft.llm_reasoning_effort,
  ]);

  const baseUrlLooksMissingV1 = useMemo(() => {
    if (draft.llm_provider_kind !== 'openai_compatible') return false;
    const u = draft.llm_base_url.trim();
    if (!u) return false;
    // Heuristic: OpenAI-compatible endpoints usually require the /v1 prefix.
    // (We keep it as a warning, not a hard validation.)
    return !/\/v1\/?$/.test(u);
  }, [draft.llm_base_url]);

  const geminiBaseUrlLooksUnexpected = useMemo(() => {
    if (draft.llm_provider_kind !== 'gemini') return false;
    const u = draft.llm_base_url.trim();
    if (!u) return false;
    return !/\/v1(beta|alpha)?\/?$/.test(u);
  }, [draft.llm_base_url, draft.llm_provider_kind]);

  const probeDefaultModel = useMemo(() => defaultModelForProvider(draft.llm_provider_kind), [draft.llm_provider_kind]);

  const probeUnknownModelActionable = useMemo(() => {
    if (!looksLikeUnknownModelError(providerDiagnosticError)) return false;
    return draft.llm_model.trim() !== probeDefaultModel;
  }, [draft.llm_model, probeDefaultModel, providerDiagnosticError]);

  const showOpenAiRecommendedCallout = useMemo(() => {
    if (draft.llm_provider_kind !== 'openai_compatible') return false;
    if (draft.llm_base_url.trim() !== 'https://api.openai.com/v1') return false;

    return (
      draft.llm_api_kind !== 'responses_sse' ||
      draft.llm_model.trim() !== 'gpt-5.4' ||
      draft.llm_preflight_mode !== 'off' ||
      draft.llm_preflight_delay_ms !== 1500 ||
      draft.llm_reasoning_effort !== ''
    );
  }, [
    draft.llm_api_kind,
    draft.llm_base_url,
    draft.llm_model,
    draft.llm_preflight_mode,
    draft.llm_preflight_delay_ms,
    draft.llm_provider_kind,
    draft.llm_reasoning_effort,
  ]);

  const showCc2ResponsesRecommendation = useMemo(() => {
    return shouldRecommendResponsesForCc2OpenAiChatCompletions(
      draft.llm_provider_kind,
      draft.llm_api_kind,
      draft.llm_base_url,
    );
  }, [draft.llm_api_kind, draft.llm_base_url, draft.llm_provider_kind]);

  const visualContextEnabled = useMemo(() => {
    return draft.context.visual_context_mode !== 'off';
  }, [draft.context.visual_context_mode]);

  const platformContextWarnings = useMemo(() => {
    return contextCapabilityWarnings(platformCapabilities, {
      useSelectedText: draft.context.use_selected_text,
      useWindowContext: draft.context.use_window_context,
      visualMode: draft.context.visual_context_mode,
      captureScope: draft.context.visual_capture_scope,
    });
  }, [
    draft.context.use_selected_text,
    draft.context.use_window_context,
    draft.context.visual_capture_scope,
    draft.context.visual_context_mode,
    platformCapabilities,
  ]);

  const visualContextDispatch = useMemo(() => {
    return resolveVisualContextDispatch(
      draft.context.visual_context_mode,
      draft.llm_provider_kind,
      draft.llm_api_kind,
    );
  }, [draft.context.visual_context_mode, draft.llm_api_kind, draft.llm_provider_kind]);

  const screenshotContextSupported = useMemo(() => {
    return llmSupportsAttachedImages(draft.llm_provider_kind, draft.llm_api_kind);
  }, [draft.llm_api_kind, draft.llm_provider_kind]);

  const screenshotContextWarningText = useMemo(() => {
    if (draft.context.visual_context_mode !== 'screenshot' || visualContextDispatch !== 'off') return null;
    return `${screenshotContextWarning(draft.llm_provider_kind, draft.llm_api_kind)} Switch to OpenAI Responses or Gemini native SSE, or use Auto/OCR mode instead.`;
  }, [draft.context.visual_context_mode, draft.llm_api_kind, draft.llm_provider_kind, visualContextDispatch]);

  const showGeminiScreenshotRecommendation = useMemo(() => {
    return shouldRecommendGeminiForScreenshotContext(
      draft.llm_provider_kind,
      draft.llm_api_kind,
      draft.llm_base_url,
      draft.context.visual_context_mode,
    );
  }, [draft.context.visual_context_mode, draft.llm_api_kind, draft.llm_base_url, draft.llm_provider_kind]);

  const compactScreenshotRecommendationPx = useMemo(() => {
    return recommendedCompactScreenshotMaxEdgePxForCurrentGateway(draft.llm_provider_kind, draft.llm_base_url);
  }, [draft.llm_base_url, draft.llm_provider_kind]);

  const showCompactScreenshotRecommendation = useMemo(() => {
    return shouldRecommendCompactScreenshotForLatency(
      draft.llm_provider_kind,
      draft.llm_base_url,
      draft.context.visual_context_mode,
      draft.screenshot_max_edge_px,
    );
  }, [draft.context.visual_context_mode, draft.llm_base_url, draft.llm_provider_kind, draft.screenshot_max_edge_px]);

  const providerProbeMatchesExpected = useMemo(() => {
    if (!providerProbe) return true;
    return providerProbe.final_output === providerProbe.expected_output;
  }, [providerProbe]);

  const lastScreenshotProbeNeededCleanup = useMemo(() => {
    return (
      providerProbe?.probe_kind === 'screenshot_product_name' &&
      draft.llm_provider_kind === 'openai_compatible' &&
      (!!providerProbe.warning || !providerProbeMatchesExpected)
    );
  }, [draft.llm_provider_kind, providerProbe, providerProbeMatchesExpected]);

  const runProviderProbe = useCallback(
    async (probeKind: ProviderProbeKind) => {
      try {
        setProbingProviderKind(probeKind);
        setProviderProbe(null);
        setProviderProbeError(null);
        setProviderBenchmark(null);
        setProviderBenchmarkError(null);
        const { invoke } = await import('@tauri-apps/api/core');
        const next = await invoke<ProviderProbeResponse>('probe_llm_provider', {
          req: {
            provider_kind: draft.llm_provider_kind,
            api_kind: draft.llm_api_kind,
            base_url: draft.llm_base_url,
            model: draft.llm_model,
            reasoning_effort: draft.llm_reasoning_effort || null,
            probe_kind: probeKind,
          },
        });
        setProviderProbe(next);
      } catch (e) {
        const msg = String(e);
        setProviderProbeError(msg);
      } finally {
        setProbingProviderKind(null);
      }
    },
    [
      draft.llm_api_kind,
      draft.llm_base_url,
      draft.llm_model,
      draft.llm_provider_kind,
      draft.llm_reasoning_effort,
    ],
  );

  const runProviderBenchmark = useCallback(
    async (probeKind: ProviderProbeKind) => {
      const rounds = 3;
      const collected: ProviderProbeResponse[] = [];

      try {
        setBenchmarkingProviderKind(probeKind);
        setProviderBenchmark(null);
        setProviderBenchmarkError(null);
        setProviderProbe(null);
        setProviderProbeError(null);
        const { invoke } = await import('@tauri-apps/api/core');

        for (let index = 0; index < rounds; index += 1) {
          const next = await invoke<ProviderProbeResponse>('probe_llm_provider', {
            req: {
              provider_kind: draft.llm_provider_kind,
              api_kind: draft.llm_api_kind,
              base_url: draft.llm_base_url,
              model: draft.llm_model,
              reasoning_effort: draft.llm_reasoning_effort || null,
              probe_kind: probeKind,
            },
          });
          collected.push(next);
        }

        setProviderBenchmark(buildProviderProbeBenchmark(collected));
      } catch (e) {
        const failedRound = Math.min(collected.length + 1, rounds);
        setProviderBenchmarkError(`Benchmark failed on round ${failedRound}/${rounds}: ${String(e)}`);
      } finally {
        setBenchmarkingProviderKind(null);
      }
    },
    [
      draft.llm_api_kind,
      draft.llm_base_url,
      draft.llm_model,
      draft.llm_provider_kind,
      draft.llm_reasoning_effort,
    ],
  );

  const applyRecommendedProviderStack = useCallback(() => {
    setDirty(true);
    setDraft((d) => ({
      ...d,
      llm_api_kind: recommendedApiKindForProvider(d.llm_provider_kind),
      llm_base_url: defaultBaseUrlForProvider(d.llm_provider_kind),
      llm_model: defaultModelForProvider(d.llm_provider_kind),
      llm_preflight_mode: 'off',
      llm_preflight_delay_ms: 1500,
      llm_reasoning_effort: '',
    }));
  }, []);

  const applyGeminiScreenshotRecommendedStack = useCallback(() => {
    setDirty(true);
    setProviderProbe(null);
    setProviderProbeError(null);
    setDraft((d) => ({
      ...d,
      llm_provider_kind: 'gemini',
      llm_api_kind: 'stream_generate_content_sse',
      llm_base_url: recommendedGeminiBaseUrlForCurrentGateway(d.llm_base_url),
      llm_model: 'gemini-3-flash-preview',
      llm_preflight_mode: 'off',
      llm_preflight_delay_ms: 1500,
      llm_reasoning_effort: '',
    }));
  }, []);

  const applyCompactScreenshotRecommendation = useCallback(() => {
    setDirty(true);
    setDraft((d) => ({
      ...d,
      screenshot_max_edge_px:
        recommendedCompactScreenshotMaxEdgePxForCurrentGateway(d.llm_provider_kind, d.llm_base_url) ?? d.screenshot_max_edge_px,
    }));
  }, []);

  const applyCc2ResponsesRecommendation = useCallback(() => {
    setDirty(true);
    setProviderProbe(null);
    setProviderProbeError(null);
    setProviderBenchmark(null);
    setProviderBenchmarkError(null);
    setDraft((d) => ({
      ...d,
      llm_provider_kind: 'openai_compatible',
      llm_api_kind: 'responses_sse',
      llm_model: 'gpt-5.4',
      llm_preflight_mode: 'off',
      llm_preflight_delay_ms: 1500,
      llm_reasoning_effort: '',
    }));
  }, []);

  if (!cfg) {
    return (
      <div
        style={{
          maxWidth: 720,
          margin: '0 auto',
          paddingTop: 64,
          paddingInline: 'var(--space-24)',
          paddingBottom: 'var(--space-32)',
        }}
      >
        <div className="vw-type-title">Settings</div>
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)' }}>
          Loading…
        </div>
        {error ? (
          <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            {error}
          </div>
        ) : null}
      </div>
    );
  }

  return (
    <div
      style={{
        maxWidth: 720,
        margin: '0 auto',
        paddingTop: 64,
        paddingInline: 'var(--space-24)',
        paddingBottom: 'var(--space-32)',
      }}
    >
      <div className="vw-type-title">Settings</div>

      {error ? (
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
          {error}
        </div>
      ) : null}

      <Section
        title="Speech-to-Text"
        subtitle="Choose the transcription engine. Local Whisper runs on-device; ElevenLabs uses cloud STT."
      >
        <SettingRow
          title="Provider"
          description="Local is private but can be slower on low-power CPUs. ElevenLabs can be faster but sends audio to the cloud."
          right={
            <select
              className="vw-input"
              value={draft.stt_provider}
              disabled={saving}
              onChange={(e) => {
                const next = e.target.value === 'elevenlabs' ? 'elevenlabs' : 'local';
                setDirty(true);
                setDraft((d) => ({ ...d, stt_provider: next }));
              }}
            >
              <option value="local">Local Whisper</option>
              <option value="elevenlabs">ElevenLabs</option>
            </select>
          }
        />

        {draft.stt_provider === 'local' ? (
          <SettingRow
            title="Local model"
            description="Use the Models tab to download/switch local Whisper models."
            right={<span className="vw-type-caption" style={{ color: localModelStatusColor }}>{localModelStatus}</span>}
          />
        ) : (
          <SettingRow
            title="ElevenLabs model"
            description="Batch sends audio on stop. Realtime streams during recording (VAD + stop flush) but still inserts only on stop."
            right={
              <select
                className="vw-input"
                value={draft.elevenlabs_stt_model}
                disabled={saving}
                onChange={(e) => {
                  const v = e.target.value === 'scribe_v2_realtime' ? 'scribe_v2_realtime' : 'scribe_v2';
                  setDirty(true);
                  setDraft((d) => ({ ...d, elevenlabs_stt_model: v }));
                }}
              >
                <option value="scribe_v2">Scribe v2 (Batch)</option>
                <option value="scribe_v2_realtime">Scribe v2 (Realtime)</option>
              </select>
            }
          />
        )}
      </Section>

      <Section
        title="ElevenLabs"
        subtitle="Required only when ElevenLabs STT is selected. The key is stored in VoiceWin secret storage (not in config.json)."
      >
        <SettingRow
          title="API key"
          description={`Status: ${elevenKeyStatus}.`}
          right={
            <div className="vw-settingControls">
              <input
                className="vw-input"
                type="password"
                placeholder="Paste xi-api-key…"
                value={elevenApiKeyDraft}
                onChange={(e) => setElevenApiKeyDraft(e.target.value)}
                style={{ width: 260 }}
                disabled={saving}
              />
              <button
                type="button"
                className="vw-button vw-button--secondary"
                disabled={saving || elevenApiKeyDraftTrimmed.length === 0}
                onClick={async () => {
                  try {
                    setSaving(true);
                    setElevenKeyError(null);
                    setElevenKeyNotice(null);

                    if (elevenApiKeyDraftTrimmed.length === 0) {
                      setElevenKeyError('API key cannot be empty. Use Clear to remove it.');
                      return;
                    }

                    const { invoke } = await import('@tauri-apps/api/core');
                    const next = await invoke<ProviderStatus>('set_elevenlabs_api_key', { apiKey: elevenApiKeyDraftTrimmed });
                    setProviders(next);

                    if (next.elevenlabs_api_key_error) {
                      setElevenKeyError(`Secret storage error: ${next.elevenlabs_api_key_error}`);
                      return;
                    }
                    if (!next.elevenlabs_api_key_present) {
                      setElevenKeyError('Saved key but it is still not present in secret storage.');
                      return;
                    }

                    const hadDirty = dirty;
                    if (hadDirty) {
                      const nextCfg = buildConfigFromDraft(cfg, draft, modelStatus);
                      const ok = await saveConfig(nextCfg, { manageSaving: false });
                      if (ok) {
                        setDirty(false);
                        setElevenKeyNotice('Saved key and settings');
                      } else {
                        setElevenKeyNotice('Saved key. Fix settings errors, then click Save Changes.');
                      }
                    } else {
                      await refresh();
                      setElevenKeyNotice('Saved');
                    }

                    setElevenApiKeyDraft('');

                    window.setTimeout(() => setElevenKeyNotice(null), 2000);
                  } catch (e) {
                    const msg = String(e);
                    setError(msg);
                    setElevenKeyError(msg);
                  } finally {
                    setSaving(false);
                  }
                }}
              >
                Save
              </button>
              <button
                type="button"
                className="vw-button vw-button--secondary"
                disabled={saving}
                onClick={async () => {
                  try {
                    setSaving(true);
                    setElevenKeyError(null);
                    setElevenKeyNotice(null);
                    const { invoke } = await import('@tauri-apps/api/core');
                    const next = await invoke<ProviderStatus>('clear_elevenlabs_api_key');
                    setProviders(next);
                    setElevenApiKeyDraft('');

                    setElevenKeyNotice('Cleared');
                    window.setTimeout(() => setElevenKeyNotice(null), 2000);
                    await refresh();
                  } catch (e) {
                    const msg = String(e);
                    setError(msg);
                    setElevenKeyError(msg);
                  } finally {
                    setSaving(false);
                  }
                }}
              >
                Clear
              </button>
            </div>
          }
        />

        {elevenKeyStatusError ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            Secret storage error: {elevenKeyStatusError}
          </div>
        ) : null}
        {elevenKeyError ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            {elevenKeyError}
          </div>
        ) : null}
        {elevenKeyNotice ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-accent)' }}>
            {elevenKeyNotice}
          </div>
        ) : null}

        {draft.stt_provider === 'elevenlabs' && providers?.elevenlabs_api_key_error ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            ElevenLabs is selected but secret storage is unavailable. Recording will fail until this is resolved.
          </div>
        ) : null}

        {draft.stt_provider === 'elevenlabs' && !providers?.elevenlabs_api_key_error && !providers?.elevenlabs_api_key_present ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            ElevenLabs is selected but no API key is set. Recording will fail until you add a key.
          </div>
        ) : null}
      </Section>

      <Section
        title="Enhancement"
        subtitle="Optional: refine the transcript using a cloud LLM. Local dictation works without this."
      >
        <SettingRow
          title="Enhance transcript"
          description="When enabled, VoiceWin will call the selected LLM provider after transcription."
          right={
            <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                type="checkbox"
                aria-label="Enhance transcript"
                checked={Boolean(draft.enable_enhancement)}
                onChange={(e) => {
                  setDirty(true);
                  setDraft((d) => ({ ...d, enable_enhancement: e.target.checked }));
                }}
                disabled={saving}
              />
              <span className="vw-type-caption">{draft.enable_enhancement ? 'On' : 'Off'}</span>
            </label>
          }
        />

        <SettingRow
          title="Default prompt"
          description="Used when enhancement runs without a trigger word. Trigger words can still switch prompts for a single session."
          right={
            <select
              className="vw-input"
              value={draft.prompt_id}
              disabled={saving || cfg.prompts.length === 0}
              onChange={(e) => {
                setDirty(true);
                setDraft((d) => ({ ...d, prompt_id: e.target.value }));
              }}
            >
              <option value="">
                {cfg.prompts[0] ? `Automatic (${cfg.prompts[0].title})` : 'No prompts loaded'}
              </option>
              {cfg.prompts.map((prompt) => (
                <option key={prompt.id} value={prompt.id}>
                  {prompt.title}
                </option>
              ))}
            </select>
          }
        />
      </Section>

      <Section
        title="Context"
        subtitle="Context is captured near recording start and frozen for the session. Visual context can attach a screenshot directly or run OCR first, depending on mode and provider capabilities."
      >
        <SettingRow
          title="Clipboard context"
          description="Include the current clipboard text when enhancement runs."
          right={
            <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                type="checkbox"
                aria-label="Clipboard context"
                checked={draft.context.use_clipboard}
                onChange={(e) => {
                  setDirty(true);
                  setDraft((d) => ({
                    ...d,
                    context: { ...d.context, use_clipboard: e.target.checked },
                  }));
                }}
                disabled={saving}
              />
              <span className="vw-type-caption">{draft.context.use_clipboard ? 'On' : 'Off'}</span>
            </label>
          }
        />

        <SettingRow
          title="Selected text"
          description="Best-effort. Use the active selection when the platform can capture it."
          right={
            <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                type="checkbox"
                aria-label="Selected text context"
                checked={draft.context.use_selected_text}
                onChange={(e) => {
                  setDirty(true);
                  setDraft((d) => ({
                    ...d,
                    context: { ...d.context, use_selected_text: e.target.checked },
                  }));
                }}
                disabled={saving}
              />
              <span className="vw-type-caption">{draft.context.use_selected_text ? 'On' : 'Off'}</span>
            </label>
          }
        />

        <SettingRow
          title="Window context"
          description="Include the active window title or text snapshot captured at recording start."
          right={
            <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                type="checkbox"
                aria-label="Window context"
                checked={draft.context.use_window_context}
                onChange={(e) => {
                  setDirty(true);
                  setDraft((d) => ({
                    ...d,
                    context: { ...d.context, use_window_context: e.target.checked },
                  }));
                }}
                disabled={saving}
              />
              <span className="vw-type-caption">{draft.context.use_window_context ? 'On' : 'Off'}</span>
            </label>
          }
        />

        <SettingRow
          title="Custom vocabulary"
          description="Include terms from custom_vocabulary.txt in the VoiceWin app data folder when present."
          right={
            <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                type="checkbox"
                aria-label="Custom vocabulary context"
                checked={draft.context.use_custom_vocabulary}
                onChange={(e) => {
                  setDirty(true);
                  setDraft((d) => ({
                    ...d,
                    context: { ...d.context, use_custom_vocabulary: e.target.checked },
                  }));
                }}
                disabled={saving}
              />
              <span className="vw-type-caption">{draft.context.use_custom_vocabulary ? 'On' : 'Off'}</span>
            </label>
          }
        />

        <SettingRow
          title="Visual mode"
          description="Auto prefers direct screenshot input on multimodal APIs and falls back to OCR on text-only APIs."
          right={
            <select
              className="vw-input"
              aria-label="Visual context mode"
              value={draft.context.visual_context_mode}
              disabled={saving}
              onChange={(e) => {
                const nextValue = e.target.value as VisualContextMode;
                setDirty(true);
                setDraft((d) => ({
                  ...d,
                  context: { ...d.context, visual_context_mode: nextValue },
                }));
              }}
            >
              <option value="off">Off</option>
              <option value="auto">Auto</option>
              <option value="screenshot">Screenshot Only</option>
              <option value="ocr">OCR Only</option>
            </select>
          }
        />

        <SettingRow
          title="Capture target"
          description="Display preserves current behavior. Foreground window is more private and more relevant when the platform supports it."
          right={
            <select
              className="vw-input"
              aria-label="Visual capture target"
              value={draft.context.visual_capture_scope}
              disabled={saving || !visualContextEnabled}
              onChange={(e) => {
                const nextValue = e.target.value as VisualCaptureScope;
                setDirty(true);
                setDraft((d) => ({
                  ...d,
                  context: { ...d.context, visual_capture_scope: nextValue },
                }));
              }}
            >
              <option value="display">Display</option>
              <option value="foreground_window">Foreground Window</option>
            </select>
          }
        />

        <SettingRow
          title="Screenshot max edge"
          description="Longest edge for the captured screenshot before upload or OCR. Smaller values can reduce visual-context latency but may change quality."
          right={
            <div className="vw-settingControls" style={{ alignItems: 'center' }}>
              <input
                className="vw-input"
                type="number"
                aria-label="Screenshot max edge"
                min={256}
                max={3840}
                step={64}
                value={draft.screenshot_max_edge_px}
                disabled={saving || !visualContextEnabled}
                onChange={(e) => {
                  setDirty(true);
                  setDraft((d) => ({
                    ...d,
                    screenshot_max_edge_px: normalizeScreenshotMaxEdgePx(e.target.value),
                  }));
                }}
                style={{ width: 140 }}
              />
              <span className="vw-type-caption">px</span>
            </div>
          }
        />

        {platformContextWarnings.map((warning) => (
          <div
            key={warning}
            className="vw-type-caption"
            style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}
          >
            {warning}
          </div>
        ))}

        {visualContextEnabled && visualContextDispatch === 'screenshot' && screenshotContextSupported && platformCapabilities.screenshot_capture ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--text-secondary)' }}>
            Visual context will attach one screenshot directly to the enhancement request.
          </div>
        ) : null}

        {visualContextEnabled && visualContextDispatch === 'ocr' && platformCapabilities.screenshot_capture ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--text-secondary)' }}>
            Visual context will capture one screenshot, run an OCR sidecar request, and inject the recognized text into the main prompt.
          </div>
        ) : null}

        {showCompactScreenshotRecommendation && compactScreenshotRecommendationPx != null ? (
          <div
            className="vw-type-caption"
            style={{
              padding: 'var(--space-12)',
              color: 'var(--text-secondary)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 'var(--space-12)',
            }}
          >
            <span>
              On <strong>cc2.caaa.tech</strong>, Gemini screenshot cleanup stayed solid at <strong>{compactScreenshotRecommendationPx} px</strong> in
              {' '}VoiceWin live validation and reduced warm multimodal latency materially. Keep {draft.screenshot_max_edge_px}px if you want the more
              {' '}conservative default, or switch to {compactScreenshotRecommendationPx}px if latency matters more.
            </span>
            <button
              type="button"
              className="vw-button vw-button--secondary"
              disabled={saving}
              onClick={applyCompactScreenshotRecommendation}
            >
              Use {compactScreenshotRecommendationPx}px
            </button>
          </div>
        ) : null}
      </Section>

      <Section
        title="LLM Provider"
        subtitle="Choose the enhancement provider and transport. OpenAI-compatible defaults to Responses + gpt-5.4; Chat Completions remains a legacy compatibility fallback. Gemini uses native streamGenerateContent over SSE."
      >
        <SettingRow
          title="Provider"
          description="OpenAI-compatible covers OpenAI Responses plus OpenAI-style gateways. Gemini uses the native Google Gemini API shape."
          right={
            <select
              className="vw-input"
              value={draft.llm_provider_kind}
              disabled={saving}
              onChange={(e) => {
                const next = e.target.value === 'gemini' ? 'gemini' : 'openai_compatible';
                setDirty(true);
                setDraft((d) => {
                  if (next === 'gemini') {
                    return {
                      ...d,
                      llm_provider_kind: 'gemini',
                      llm_api_kind: 'stream_generate_content_sse',
                      llm_base_url:
                        d.llm_provider_kind === 'gemini' ? d.llm_base_url : defaultBaseUrlForProvider('gemini'),
                      llm_model: d.llm_provider_kind === 'gemini' ? d.llm_model : defaultModelForProvider('gemini'),
                    };
                  }

                  return {
                    ...d,
                    llm_provider_kind: 'openai_compatible',
                    llm_api_kind:
                      d.llm_api_kind === 'stream_generate_content_sse' ? 'responses_sse' : d.llm_api_kind,
                    llm_base_url:
                      d.llm_provider_kind === 'openai_compatible'
                        ? d.llm_base_url
                        : defaultBaseUrlForProvider('openai_compatible'),
                    llm_model:
                      d.llm_provider_kind === 'openai_compatible' ? d.llm_model : defaultModelForProvider('openai_compatible'),
                  };
                });
              }}
            >
              <option value="openai_compatible">OpenAI-Compatible</option>
              <option value="gemini">Google Gemini</option>
            </select>
          }
        />

        {showOpenAiRecommendedCallout ? (
          <div
            className="vw-type-caption"
            style={{
              padding: 'var(--space-12)',
              color: 'var(--text-secondary)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 'var(--space-12)',
            }}
          >
            <span>
              Recommended OpenAI stack: <strong>Responses (HTTP SSE)</strong> + <strong>gpt-5.4</strong> +
              {' '}<strong>Preflight Off</strong>. This matches the validated VoiceWin default path.
            </span>
            <button
              type="button"
              className="vw-button vw-button--secondary"
              disabled={saving}
              onClick={applyRecommendedProviderStack}
            >
              Apply Recommended
            </button>
          </div>
        ) : null}

        {screenshotContextWarningText ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            {screenshotContextWarningText}
          </div>
        ) : null}

        {showGeminiScreenshotRecommendation ? (
          <div
            className="vw-type-caption"
            style={{
              padding: 'var(--space-12)',
              color: 'var(--text-secondary)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 'var(--space-12)',
            }}
          >
            <span>
              On <strong>cc2.caaa.tech</strong>, Gemini has been the cleaner screenshot-assisted path in VoiceWin live validation.
              {' '}This is a quality recommendation, not a speed guarantee. OpenAI-compatible still works here, but often needs wrapper stripping.
              {lastScreenshotProbeNeededCleanup ? ' Your last screenshot probe also needed cleanup.' : ''}
            </span>
            <button
              type="button"
              className="vw-button vw-button--secondary"
              disabled={saving}
              onClick={applyGeminiScreenshotRecommendedStack}
            >
              Switch to Gemini
            </button>
          </div>
        ) : null}

        {showCc2ResponsesRecommendation ? (
          <div
            className="vw-type-caption"
            style={{
              padding: 'var(--space-12)',
              color: 'var(--text-secondary)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 'var(--space-12)',
            }}
          >
            <span>
              On <strong>cc2.caaa.tech</strong>, OpenAI-compatible Chat Completions has been the unstable path in
              {' '}VoiceWin live validation and may return no available providers. Responses + <strong>gpt-5.4</strong>
              {' '}is the safer default on this gateway.
            </span>
            <button
              type="button"
              className="vw-button vw-button--secondary"
              disabled={saving}
              onClick={applyCc2ResponsesRecommendation}
            >
              Use Responses + GPT-5.4
            </button>
          </div>
        ) : null}

        <SettingRow
          title="API mode"
          description={
            draft.llm_provider_kind === 'gemini'
              ? 'Gemini uses models/*:streamGenerateContent?alt=sse for low-latency native generation.'
              : 'Responses uses /v1/responses over HTTP SSE with stream:true and store:false. Chat Completions remains the older compatibility fallback.'
          }
          right={
            <select
              className="vw-input"
              value={draft.llm_api_kind}
              disabled={saving}
              onChange={(e) => {
                setDirty(true);
                setDraft((d) => ({
                  ...d,
                  llm_api_kind:
                    d.llm_provider_kind === 'gemini'
                      ? 'stream_generate_content_sse'
                      : e.target.value === 'responses_sse'
                        ? 'responses_sse'
                        : 'chat_completions',
                }));
              }}
            >
              {draft.llm_provider_kind === 'gemini' ? (
                <option value="stream_generate_content_sse">streamGenerateContent (HTTP SSE)</option>
              ) : (
                <>
                  <option value="chat_completions">Chat Completions (Legacy)</option>
                  <option value="responses_sse">OpenAI Responses (HTTP SSE)</option>
                </>
              )}
            </select>
          }
        />

        <SettingRow
          title="Preflight"
          description="Best-effort connection warmup on recording start. HTTP connect sends an authenticated GET /models with the persistent client; Off skips warmup."
          right={
            <select
              className="vw-input"
              value={draft.llm_preflight_mode}
              disabled={saving}
              onChange={(e) => {
                setDirty(true);
                setDraft((d) => ({
                  ...d,
                  llm_preflight_mode: e.target.value === 'off' ? 'off' : 'http_connect',
                }));
              }}
            >
              <option value="off">Off</option>
              <option value="http_connect">HTTP Connect</option>
            </select>
          }
        />

        <SettingRow
          title="Preflight delay"
          description="How long VoiceWin waits after recording starts before sending the optional warmup request. A delay helps avoid hurting short recordings."
          right={
            <div className="vw-settingControls" style={{ alignItems: 'center' }}>
              <input
                className="vw-input"
                type="number"
                aria-label="Preflight delay"
                min={0}
                max={60000}
                step={100}
                value={draft.llm_preflight_delay_ms}
                disabled={saving || draft.llm_preflight_mode === 'off'}
                onChange={(e) => {
                  setDirty(true);
                  setDraft((d) => ({
                    ...d,
                    llm_preflight_delay_ms: normalizeLlmPreflightDelayMs(e.target.value),
                  }));
                }}
                style={{ width: 140 }}
              />
              <span className="vw-type-caption">ms</span>
            </div>
          }
        />

        <SettingRow
          title="Reasoning effort"
          description="Optional. Disabled omits the reasoning block; use it only with reasoning-capable models."
          right={
            <select
              className="vw-input"
              value={draft.llm_reasoning_effort}
              disabled={saving}
              onChange={(e) => {
                setDirty(true);
                setDraft((d) => ({
                  ...d,
                  llm_reasoning_effort: normalizeLlmReasoningEffort(e.target.value),
                }));
              }}
            >
              <option value="">Disabled</option>
              <option value="minimal">Minimal</option>
              <option value="low">Low</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
            </select>
          }
        />

        <SettingRow
          title="Base URL"
          description={
            draft.llm_provider_kind === 'gemini'
              ? 'Example: https://generativelanguage.googleapis.com/v1beta'
              : 'Example: https://api.openai.com/v1 or http://localhost:11434/v1'
          }
          right={
            <div className="vw-settingControls">
              <input
                className="vw-input"
                type="text"
                value={draft.llm_base_url}
                onChange={(e) => {
                  setDirty(true);
                  setDraft((d) => ({ ...d, llm_base_url: e.target.value }));
                }}
                style={{ width: 420 }}
                disabled={saving}
              />
              <button
                type="button"
                className="vw-button vw-button--secondary"
                disabled={saving}
                onClick={() => {
                  setDirty(true);
                  setDraft((d) => ({
                    ...d,
                    llm_base_url: defaultBaseUrlForProvider(d.llm_provider_kind),
                  }));
                }}
              >
                Reset
              </button>
            </div>
          }
        />

        <SettingRow
          title="Model"
          description={
            draft.llm_provider_kind === 'gemini'
              ? 'Example: gemini-3-flash-preview'
              : 'Example: gpt-5.4'
          }
          right={
            <div className="vw-settingControls">
              <input
                className="vw-input"
                type="text"
                value={draft.llm_model}
                onChange={(e) => {
                  setDirty(true);
                  setDraft((d) => ({ ...d, llm_model: e.target.value }));
                }}
                style={{ width: 260 }}
                disabled={saving}
              />
              <button
                type="button"
                className="vw-button vw-button--secondary"
                disabled={saving}
                onClick={() => {
                  setDirty(true);
                  setDraft((d) => ({
                    ...d,
                    llm_model: defaultModelForProvider(d.llm_provider_kind),
                  }));
                }}
              >
                Reset
              </button>
            </div>
          }
        />

        <SettingRow
          title="Provider probe"
          description="Quick smoke probe uses exact-output text only. Screenshot probe uses a built-in VoiceWin image to validate multimodal image-input wiring on the selected provider/model. The 3-round benchmark buttons reuse the same draft and same client for rough latency comparison, but they do not measure the full recording stop path."
          right={
            <div className="vw-settingControls" style={{ alignItems: 'center', flexWrap: 'wrap' }}>
              <button
                type="button"
                className="vw-button vw-button--secondary"
                disabled={saving || probingProvider || benchmarkingProvider || !selectedLlmApiKeyPresent}
                onClick={() => {
                  void runProviderProbe('smoke');
                }}
              >
                {probingProviderKind === 'smoke' ? 'Running…' : 'Run Probe'}
              </button>
              <button
                type="button"
                className="vw-button vw-button--secondary"
                disabled={saving || probingProvider || benchmarkingProvider || !selectedLlmApiKeyPresent}
                onClick={() => {
                  void runProviderBenchmark('smoke');
                }}
              >
                {benchmarkingProviderKind === 'smoke' ? 'Benchmarking…' : 'Run 3-Round Benchmark'}
              </button>
              {llmSupportsAttachedImages(draft.llm_provider_kind, draft.llm_api_kind) ? (
                <>
                  <button
                    type="button"
                    className="vw-button vw-button--secondary"
                    disabled={saving || probingProvider || benchmarkingProvider || !selectedLlmApiKeyPresent}
                    onClick={() => {
                      void runProviderProbe('screenshot_product_name');
                    }}
                  >
                    {probingProviderKind === 'screenshot_product_name' ? 'Running…' : 'Run Screenshot Probe'}
                  </button>
                  <button
                    type="button"
                    className="vw-button vw-button--secondary"
                    disabled={saving || probingProvider || benchmarkingProvider || !selectedLlmApiKeyPresent}
                    onClick={() => {
                      void runProviderBenchmark('screenshot_product_name');
                    }}
                  >
                    {benchmarkingProviderKind === 'screenshot_product_name'
                      ? 'Benchmarking…'
                      : 'Run 3-Round Screenshot Benchmark'}
                  </button>
                </>
              ) : null}
              {!selectedLlmApiKeyPresent ? (
                <span className="vw-type-caption" style={{ color: 'var(--color-danger-fg)' }}>
                  Save a {draft.llm_provider_kind === 'gemini' ? 'Gemini' : 'OpenAI-compatible'} key first.
                </span>
              ) : null}
            </div>
          }
        />

        {providerProbe ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--text-secondary)' }}>
            {formatProbeKind(providerProbe.probe_kind)} • {providerProbe.provider_kind} • {providerProbe.api_kind} • {providerProbe.model} • {formatProbeLatency(providerProbe)} • Output: {providerProbe.final_output}
          </div>
        ) : null}
        {providerProbe && !providerProbeMatchesExpected ? (
          <div className="vw-type-caption" style={{ padding: '0 var(--space-12) var(--space-12)', color: 'var(--color-danger-fg)' }}>
            Expected: {providerProbe.expected_output}
          </div>
        ) : null}
        {providerProbe?.warning ? (
          <div className="vw-type-caption" style={{ padding: '0 var(--space-12) var(--space-12)', color: 'var(--text-secondary)' }}>
            Warning: {providerProbe.warning}
          </div>
        ) : null}
        {providerBenchmark ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--text-secondary)' }}>
            {[
              `${formatProbeKind(providerBenchmark.probe_kind).replace('probe', 'benchmark')} • ${providerBenchmark.rounds} rounds`,
              providerBenchmark.provider_kind,
              providerBenchmark.api_kind,
              providerBenchmark.model,
              formatBenchmarkLatency(
                providerBenchmark.elapsed_min_ms,
                providerBenchmark.elapsed_avg_ms,
                providerBenchmark.elapsed_max_ms,
                'total',
              ),
              formatBenchmarkLatency(
                providerBenchmark.first_token_min_ms,
                providerBenchmark.first_token_avg_ms,
                providerBenchmark.first_token_max_ms,
                'first token',
              ),
              formatBenchmarkCountRange(
                providerBenchmark.input_tokens_min,
                providerBenchmark.input_tokens_avg,
                providerBenchmark.input_tokens_max,
                'input',
                'tok',
              ),
              formatBenchmarkCountRange(
                providerBenchmark.cached_input_tokens_min,
                providerBenchmark.cached_input_tokens_avg,
                providerBenchmark.cached_input_tokens_max,
                'cache',
                'tok',
              ),
              `warnings ${providerBenchmark.warning_count}/${providerBenchmark.rounds}`,
              `mismatches ${providerBenchmark.mismatch_count}/${providerBenchmark.rounds}`,
              `output variants ${providerBenchmark.output_variant_count}`,
              `Last output: ${providerBenchmark.last_output}`,
            ]
              .filter((value): value is string => Boolean(value))
              .join(' • ')}
          </div>
        ) : null}
        {providerBenchmark?.sample_warning ? (
          <div className="vw-type-caption" style={{ padding: '0 var(--space-12) var(--space-12)', color: 'var(--text-secondary)' }}>
            Sample warning: {providerBenchmark.sample_warning}
          </div>
        ) : null}
        {providerBenchmark && providerBenchmark.mismatch_count > 0 ? (
          <div className="vw-type-caption" style={{ padding: '0 var(--space-12) var(--space-12)', color: 'var(--color-danger-fg)' }}>
            Expected: {providerBenchmark.expected_output}
          </div>
        ) : null}
        {providerProbeError ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            {providerProbeError}
          </div>
        ) : null}
        {providerBenchmarkError ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            {providerBenchmarkError}
          </div>
        ) : null}
        {probeUnknownModelActionable ? (
          <div
            className="vw-type-caption"
            style={{
              padding: 'var(--space-12)',
              color: 'var(--text-secondary)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 'var(--space-12)',
            }}
          >
            <span>
              The provider rejected this model name. Reset to <strong>{probeDefaultModel}</strong> and probe again.
            </span>
            <button
              type="button"
              className="vw-button vw-button--secondary"
              disabled={saving || probingProvider}
              onClick={() => {
                setDirty(true);
                setProviderProbe(null);
                setProviderProbeError(null);
                setDraft((d) => ({
                  ...d,
                  llm_model: defaultModelForProvider(d.llm_provider_kind),
                }));
              }}
            >
              Use Default Model
            </button>
          </div>
        ) : null}
      </Section>

      <Section
        title="OpenAI-Compatible"
        subtitle="Store the key used for OpenAI Responses or OpenAI-style gateways. The key is stored in VoiceWin secret storage, not config.json."
      >
        <SettingRow
          title="API key"
          description={`Status: ${openaiKeyStatus}.`}
          right={
            <div className="vw-settingControls">
              <input
                className="vw-input"
                type="password"
                placeholder="Paste key…"
                value={openaiApiKeyDraft}
                onChange={(e) => setOpenaiApiKeyDraft(e.target.value)}
                style={{ width: 260 }}
                disabled={saving}
              />
              <button
                type="button"
                className="vw-button vw-button--secondary"
                disabled={saving || openaiApiKeyDraftTrimmed.length === 0}
                onClick={async () => {
                  try {
                    setSaving(true);
                    setOpenaiKeyError(null);
                    setOpenaiKeyNotice(null);

                    if (openaiApiKeyDraftTrimmed.length === 0) {
                      setOpenaiKeyError('API key cannot be empty. Use Clear to remove it.');
                      return;
                    }

                    const { invoke } = await import('@tauri-apps/api/core');
                    const next = await invoke<ProviderStatus>('set_openai_api_key', { apiKey: openaiApiKeyDraftTrimmed });
                    setProviders(next);

                    if (next.openai_api_key_error) {
                      setOpenaiKeyError(`Secret storage error: ${next.openai_api_key_error}`);
                      return;
                    }
                    if (!next.openai_api_key_present) {
                      setOpenaiKeyError('Saved key but it is still not present in secret storage.');
                      return;
                    }

                    setOpenaiApiKeyDraft('');
                    setOpenaiKeyNotice('Saved');
                    window.setTimeout(() => setOpenaiKeyNotice(null), 2000);
                    await refresh();
                  } catch (e) {
                    const msg = String(e);
                    setError(msg);
                    setOpenaiKeyError(msg);
                  } finally {
                    setSaving(false);
                  }
                }}
              >
                Save
              </button>
              <button
                type="button"
                className="vw-button vw-button--secondary"
                disabled={saving}
                onClick={async () => {
                  try {
                    setSaving(true);
                    setOpenaiKeyError(null);
                    setOpenaiKeyNotice(null);
                    const { invoke } = await import('@tauri-apps/api/core');
                    const next = await invoke<ProviderStatus>('clear_openai_api_key');
                    setProviders(next);
                    setOpenaiApiKeyDraft('');

                    setOpenaiKeyNotice('Cleared');
                    window.setTimeout(() => setOpenaiKeyNotice(null), 2000);
                    await refresh();
                  } catch (e) {
                    const msg = String(e);
                    setError(msg);
                    setOpenaiKeyError(msg);
                  } finally {
                    setSaving(false);
                  }
                }}
              >
                Clear
              </button>
            </div>
          }
        />

        {openaiKeyStatusError ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            Secret storage error: {openaiKeyStatusError}
          </div>
        ) : null}
        {openaiKeyError ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            {openaiKeyError}
          </div>
        ) : null}
        {openaiKeyNotice ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-accent)' }}>
            {openaiKeyNotice}
          </div>
        ) : null}

        {draft.llm_provider_kind === 'openai_compatible' && !providers?.openai_api_key_error && !providers?.openai_api_key_present ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            OpenAI-compatible enhancement is selected but no API key is set. VoiceWin will fall back to the raw transcript.
          </div>
        ) : null}
      </Section>

      <Section
        title="Google Gemini"
        subtitle="Store the key used for the native Gemini API. The key is stored in VoiceWin secret storage, not config.json."
      >
        <SettingRow
          title="API key"
          description={`Status: ${geminiKeyStatus}.`}
          right={
            <div className="vw-settingControls">
              <input
                className="vw-input"
                type="password"
                placeholder="Paste Gemini key…"
                value={geminiApiKeyDraft}
                onChange={(e) => setGeminiApiKeyDraft(e.target.value)}
                style={{ width: 260 }}
                disabled={saving}
              />
              <button
                type="button"
                className="vw-button vw-button--secondary"
                disabled={saving || geminiApiKeyDraftTrimmed.length === 0}
                onClick={async () => {
                  try {
                    setSaving(true);
                    setGeminiKeyError(null);
                    setGeminiKeyNotice(null);

                    if (geminiApiKeyDraftTrimmed.length === 0) {
                      setGeminiKeyError('API key cannot be empty. Use Clear to remove it.');
                      return;
                    }

                    const { invoke } = await import('@tauri-apps/api/core');
                    const next = await invoke<ProviderStatus>('set_gemini_api_key', { apiKey: geminiApiKeyDraftTrimmed });
                    setProviders(next);

                    if (next.gemini_api_key_error) {
                      setGeminiKeyError(`Secret storage error: ${next.gemini_api_key_error}`);
                      return;
                    }
                    if (!next.gemini_api_key_present) {
                      setGeminiKeyError('Saved key but it is still not present in secret storage.');
                      return;
                    }

                    setGeminiApiKeyDraft('');
                    setGeminiKeyNotice('Saved');
                    window.setTimeout(() => setGeminiKeyNotice(null), 2000);
                    await refresh();
                  } catch (e) {
                    const msg = String(e);
                    setError(msg);
                    setGeminiKeyError(msg);
                  } finally {
                    setSaving(false);
                  }
                }}
              >
                Save
              </button>
              <button
                type="button"
                className="vw-button vw-button--secondary"
                disabled={saving}
                onClick={async () => {
                  try {
                    setSaving(true);
                    setGeminiKeyError(null);
                    setGeminiKeyNotice(null);
                    const { invoke } = await import('@tauri-apps/api/core');
                    const next = await invoke<ProviderStatus>('clear_gemini_api_key');
                    setProviders(next);
                    setGeminiApiKeyDraft('');

                    setGeminiKeyNotice('Cleared');
                    window.setTimeout(() => setGeminiKeyNotice(null), 2000);
                    await refresh();
                  } catch (e) {
                    const msg = String(e);
                    setError(msg);
                    setGeminiKeyError(msg);
                  } finally {
                    setSaving(false);
                  }
                }}
              >
                Clear
              </button>
            </div>
          }
        />

        {geminiKeyStatusError ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            Secret storage error: {geminiKeyStatusError}
          </div>
        ) : null}
        {geminiKeyError ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            {geminiKeyError}
          </div>
        ) : null}
        {geminiKeyNotice ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-accent)' }}>
            {geminiKeyNotice}
          </div>
        ) : null}

        {draft.llm_provider_kind === 'gemini' && !providers?.gemini_api_key_error && !providers?.gemini_api_key_present ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            Gemini enhancement is selected but no API key is set. VoiceWin will fall back to the raw transcript.
          </div>
        ) : null}
      </Section>

      {baseUrlLooksMissingV1 ? (
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
          Warning: your Base URL does not end with <code>/v1</code>. Many OpenAI-compatible servers require it.
        </div>
      ) : null}

      {geminiBaseUrlLooksUnexpected ? (
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
          Warning: Gemini base URLs usually end with <code>/v1beta</code> or <code>/v1alpha</code>.
        </div>
      ) : null}

      {dirty ? (
        <div
          style={{
            display: 'flex',
            gap: 'var(--space-12)',
            marginTop: 'var(--space-16)',
            marginBottom: 'var(--space-24)',
          }}
        >
          <button
            type="button"
            className="vw-button vw-button--secondary"
            disabled={saving}
            onClick={() => {
              setDirty(false);
              setDraft(draftFromConfig(cfg, modelStatus));
            }}
          >
            Cancel
          </button>

          <button
            type="button"
            className="vw-button vw-button--primary"
            disabled={saving}
            onClick={() => {
              const nextCfg = buildConfigFromDraft(cfg, draft, modelStatus);
              void (async () => {
                const ok = await saveConfig(nextCfg);
                if (ok) setDirty(false);
              })();
            }}
          >
            Save Changes
          </button>
        </div>
      ) : (
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)' }}>
          Tip: If enhancement is On but no API key is set, VoiceWin will fall back to the raw transcript.
        </div>
      )}
    </div>
  );
}
