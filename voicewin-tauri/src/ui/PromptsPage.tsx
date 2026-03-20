import { useCallback, useEffect, useMemo, useState } from 'react';

import type {
  AppConfig,
  PromptPreviewContextOverride,
  PromptPreviewResponse,
  PromptTemplate,
  VisualCaptureScope,
  VisualContextRuntime,
} from '../lib/types';
import { formatBenchmarkCountRange, formatBenchmarkLatency, summarizeNumbers } from './llmBenchmark';

type PreviewScope = 'current_app' | 'defaults' | `profile:${string}`;
type PreviewContextDraft = {
  clipboard: string;
  selected_text: string;
  window_context: string;
  screenshot_data_url: string;
  screenshot_name: string;
};

type PromptPreviewBenchmark = {
  rounds: number;
  provider_kind: string;
  api_kind: string;
  model: string;
  last_visual_label: string | null;
  visual_variant_count: number;
  elapsed_min_ms: number;
  elapsed_avg_ms: number;
  elapsed_max_ms: number;
  first_token_min_ms: number | null;
  first_token_avg_ms: number | null;
  first_token_max_ms: number | null;
  capture_elapsed_min_ms: number | null;
  capture_elapsed_avg_ms: number | null;
  capture_elapsed_max_ms: number | null;
  ocr_elapsed_min_ms: number | null;
  ocr_elapsed_avg_ms: number | null;
  ocr_elapsed_max_ms: number | null;
  ocr_first_token_min_ms: number | null;
  ocr_first_token_avg_ms: number | null;
  ocr_first_token_max_ms: number | null;
  ocr_text_chars_min: number | null;
  ocr_text_chars_avg: number | null;
  ocr_text_chars_max: number | null;
  input_tokens_min: number | null;
  input_tokens_avg: number | null;
  input_tokens_max: number | null;
  cached_input_tokens_min: number | null;
  cached_input_tokens_avg: number | null;
  cached_input_tokens_max: number | null;
  warning_count: number;
  final_output_variant_count: number;
  raw_output_variant_count: number;
  capture_fallback_count: number;
  sample_capture_fallback_reason: string | null;
  sample_warning: string | null;
  last_final_output: string;
};

function formatCaptureScope(scope: VisualCaptureScope): string {
  return scope === 'foreground_window' ? 'foreground-window' : 'display';
}

function formatBenchmarkVisualLabel(runtime?: VisualContextRuntime | null): string | null {
  if (!runtime || runtime.mode === 'off') return null;
  const visual = runtime.dispatch === runtime.mode ? runtime.mode : `${runtime.mode}->${runtime.dispatch}`;
  const scope = formatCaptureScope(runtime.capture_scope);
  const source =
    runtime.screen_ocr_source != null && runtime.dispatch === 'ocr'
      ? ` / ocr-${runtime.screen_ocr_source}`
      : '';
  const actualScope =
    runtime.capture_actual_scope != null && runtime.capture_actual_scope !== runtime.capture_scope
      ? ` / actual-${formatCaptureScope(runtime.capture_actual_scope)}`
      : '';
  return `${visual} / ${scope}${source}${actualScope}`;
}

function buildPromptPreviewBenchmark(rounds: PromptPreviewResponse[]): PromptPreviewBenchmark {
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
  const captureElapsed = summarizeNumbers(
    rounds
      .map((round) => round.visual_context_runtime?.screenshot_capture_elapsed_ms)
      .filter((value): value is number => value != null),
  );
  const ocrElapsed = summarizeNumbers(
    rounds
      .map((round) => round.visual_context_runtime?.screen_ocr_elapsed_ms)
      .filter((value): value is number => value != null),
  );
  const ocrFirstToken = summarizeNumbers(
    rounds
      .map((round) => round.visual_context_runtime?.screen_ocr_first_token_ms)
      .filter((value): value is number => value != null),
  );
  const ocrTextChars = summarizeNumbers(
    rounds
      .map((round) => round.visual_context_runtime?.screen_ocr_text_chars)
      .filter((value): value is number => value != null),
  );
  const visualLabels = rounds
    .map((round) => formatBenchmarkVisualLabel(round.visual_context_runtime))
    .filter((value): value is string => value != null);

  return {
    rounds: rounds.length,
    provider_kind: last.provider_kind,
    api_kind: last.api_kind,
    model: last.model,
    last_visual_label: formatBenchmarkVisualLabel(last.visual_context_runtime),
    visual_variant_count: new Set(visualLabels).size,
    elapsed_min_ms: elapsed?.min ?? 0,
    elapsed_avg_ms: elapsed?.avg ?? 0,
    elapsed_max_ms: elapsed?.max ?? 0,
    first_token_min_ms: firstToken?.min ?? null,
    first_token_avg_ms: firstToken?.avg ?? null,
    first_token_max_ms: firstToken?.max ?? null,
    capture_elapsed_min_ms: captureElapsed?.min ?? null,
    capture_elapsed_avg_ms: captureElapsed?.avg ?? null,
    capture_elapsed_max_ms: captureElapsed?.max ?? null,
    ocr_elapsed_min_ms: ocrElapsed?.min ?? null,
    ocr_elapsed_avg_ms: ocrElapsed?.avg ?? null,
    ocr_elapsed_max_ms: ocrElapsed?.max ?? null,
    ocr_first_token_min_ms: ocrFirstToken?.min ?? null,
    ocr_first_token_avg_ms: ocrFirstToken?.avg ?? null,
    ocr_first_token_max_ms: ocrFirstToken?.max ?? null,
    ocr_text_chars_min: ocrTextChars?.min ?? null,
    ocr_text_chars_avg: ocrTextChars?.avg ?? null,
    ocr_text_chars_max: ocrTextChars?.max ?? null,
    input_tokens_min: inputTokens?.min ?? null,
    input_tokens_avg: inputTokens?.avg ?? null,
    input_tokens_max: inputTokens?.max ?? null,
    cached_input_tokens_min: cachedInputTokens?.min ?? null,
    cached_input_tokens_avg: cachedInputTokens?.avg ?? null,
    cached_input_tokens_max: cachedInputTokens?.max ?? null,
    warning_count: rounds.filter((round) => Boolean(round.warning)).length,
    final_output_variant_count: new Set(rounds.map((round) => round.final_output)).size,
    raw_output_variant_count: new Set(rounds.map((round) => round.raw_output)).size,
    capture_fallback_count: rounds.filter((round) => Boolean(round.visual_context_runtime?.capture_fallback_reason)).length,
    sample_capture_fallback_reason:
      rounds.find((round) => round.visual_context_runtime?.capture_fallback_reason)?.visual_context_runtime
        ?.capture_fallback_reason ?? null,
    sample_warning: rounds.find((round) => round.warning)?.warning ?? null,
    last_final_output: last.final_output,
  };
}

async function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error('Failed to read image file.'));
    reader.onload = () => {
      if (typeof reader.result !== 'string') {
        reject(new Error('Image file could not be converted to a data URL.'));
        return;
      }
      resolve(reader.result);
    };
    reader.readAsDataURL(file);
  });
}

function newPrompt(title = 'New Prompt'): PromptTemplate {
  return {
    id: crypto.randomUUID(),
    title,
    mode: 'Enhancer',
    prompt_text: '',
    trigger_words: [],
  };
}

function Section({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="vw-card" style={{ padding: 'var(--space-20)', display: 'grid', gap: 'var(--space-16)' }}>
      <div>
        <div className="vw-type-bodyStrong">{title}</div>
        {subtitle ? (
          <div className="vw-type-caption" style={{ marginTop: 4 }}>
            {subtitle}
          </div>
        ) : null}
      </div>
      {children}
    </div>
  );
}

function formatPreviewScope(preview: PromptPreviewResponse): string | null {
  const parts: string[] = [];

  if (preview.app_process_name) {
    parts.push(preview.app_process_name);
  }
  if (preview.matched_profile_name) {
    parts.push(`Profile ${preview.matched_profile_name}`);
  } else {
    parts.push('Using defaults');
  }

  return parts.join(' • ');
}

function formatPreviewLatency(preview: PromptPreviewResponse): string {
  const parts = [`${preview.elapsed_ms} ms total`];
  if (preview.first_token_ms != null) {
    parts.push(`${preview.first_token_ms} ms first token`);
  }
  if (preview.input_tokens != null) {
    parts.push(`${preview.input_tokens} input tok`);
  }
  if (preview.cached_input_tokens != null) {
    parts.push(`${preview.cached_input_tokens} cached tok`);
  }
  return parts.join(' • ');
}

function formatPreviewVisualRuntime(preview: PromptPreviewResponse): string | null {
  const runtime = preview.visual_context_runtime;
  if (!runtime || runtime.mode === 'off') return null;

  const parts = [
    runtime.dispatch === runtime.mode
      ? `Visual ${runtime.mode}`
      : `Visual ${runtime.mode} -> ${runtime.dispatch}`,
    `capture ${formatCaptureScope(runtime.capture_scope)}`,
  ];
  if (runtime.capture_actual_scope && runtime.capture_actual_scope !== runtime.capture_scope) {
    parts.push(`captured ${formatCaptureScope(runtime.capture_actual_scope)}`);
  }
  if (runtime.screenshot_capture_elapsed_ms != null) {
    parts.push(`Capture ${runtime.screenshot_capture_elapsed_ms} ms`);
  }
  if (runtime.capture_fallback_reason) {
    parts.push(`fallback ${runtime.capture_fallback_reason}`);
  }
  if (runtime.dispatch === 'ocr' && runtime.screen_ocr_source) {
    parts.push(`OCR ${runtime.screen_ocr_source}`);
  }
  if (runtime.screen_ocr_elapsed_ms != null) {
    parts.push(`OCR ${runtime.screen_ocr_elapsed_ms} ms`);
  }
  if (runtime.screen_ocr_first_token_ms != null) {
    parts.push(`OCR first token ${runtime.screen_ocr_first_token_ms} ms`);
  }
  if (runtime.screen_ocr_text_chars != null) {
    parts.push(`OCR text ${runtime.screen_ocr_text_chars} chars`);
  }
  return parts.join(' • ');
}

function trimToNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

export function PromptsPage() {
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [prompts, setPrompts] = useState<PromptTemplate[] | null>(null);
  const [defaultPromptId, setDefaultPromptId] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [previewScope, setPreviewScope] = useState<PreviewScope>('current_app');
  const [previewText, setPreviewText] = useState('hello voicewin world');
  const [previewContext, setPreviewContext] = useState<PreviewContextDraft>({
    clipboard: '',
    selected_text: '',
    window_context: '',
    screenshot_data_url: '',
    screenshot_name: '',
  });
  const [preview, setPreview] = useState<PromptPreviewResponse | null>(null);
  const [previewBenchmark, setPreviewBenchmark] = useState<PromptPreviewBenchmark | null>(null);
  const [previewBenchmarkError, setPreviewBenchmarkError] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [benchmarkingPreview, setBenchmarkingPreview] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const nextCfg = await invoke<AppConfig>('get_config');
      setCfg(nextCfg);
      setPrompts(nextCfg.prompts);
      setDefaultPromptId(nextCfg.defaults.prompt_id ?? null);
      setSelectedId((current) => {
        if (nextCfg.prompts.length === 0) return null;
        if (current && nextCfg.prompts.some((prompt) => prompt.id === current)) return current;
        return nextCfg.prompts[0].id;
      });
      setPreviewScope((current) => {
        const enabledProfiles = nextCfg.profiles.filter((profile) => profile.enabled);
        if (current.startsWith('profile:')) {
          const profileId = current.slice('profile:'.length);
          if (!enabledProfiles.some((profile) => profile.id === profileId)) {
            return 'current_app';
          }
        }
        return current;
      });
      setDirty(false);
      setError(null);
    } catch (e) {
      setCfg(null);
      setPrompts([]);
      setDefaultPromptId(null);
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const selected = useMemo(() => {
    if (!prompts || !selectedId) return null;
    return prompts.find((prompt) => prompt.id === selectedId) ?? null;
  }, [prompts, selectedId]);

  const selectableProfiles = useMemo(
    () => (cfg?.profiles ?? []).filter((profile) => profile.enabled),
    [cfg],
  );

  const effectiveDefaultPromptId = useMemo(() => {
    if (!prompts || prompts.length === 0) return null;
    return defaultPromptId ?? prompts[0].id;
  }, [defaultPromptId, prompts]);

  const selectedPreviewFingerprint = useMemo(() => {
    if (!selected) return '';
    return JSON.stringify({
      id: selected.id,
      title: selected.title,
      mode: selected.mode,
      prompt_text: selected.prompt_text,
      trigger_words: selected.trigger_words,
    });
  }, [selected]);

  const previewScopeArgs = useMemo(() => {
    if (previewScope === 'defaults') {
      return {
        force_defaults: true,
        forced_profile_id: null,
      };
    }
    if (previewScope.startsWith('profile:')) {
      return {
        force_defaults: false,
        forced_profile_id: previewScope.slice('profile:'.length),
      };
    }
    return {
      force_defaults: false,
      forced_profile_id: null,
    };
  }, [previewScope]);

  const previewContextArgs = useMemo(() => {
    const clipboard = trimToNull(previewContext.clipboard ?? '');
    const selectedText = trimToNull(previewContext.selected_text ?? '');
    const windowContext = trimToNull(previewContext.window_context ?? '');
    const screenshotDataUrl = trimToNull(previewContext.screenshot_data_url ?? '');

    if (!clipboard && !selectedText && !windowContext && !screenshotDataUrl) {
      return null;
    }

    return {
      clipboard,
      selected_text: selectedText,
      window_context: windowContext,
      screenshot_data_url: screenshotDataUrl,
    } satisfies PromptPreviewContextOverride;
  }, [previewContext]);

  useEffect(() => {
    setPreview(null);
    setPreviewBenchmark(null);
    setPreviewBenchmarkError(null);
  }, [previewScope, previewText, previewContextArgs, selectedPreviewFingerprint]);

  const runPreview = useCallback(async () => {
    if (!selected) return;

    setPreviewing(true);
    try {
      setError(null);
      setPreviewBenchmark(null);
      setPreviewBenchmarkError(null);
      const { invoke } = await import('@tauri-apps/api/core');
      const nextPreview = await invoke<PromptPreviewResponse>('preview_prompt', {
        req: {
          prompt: selected,
          transcript: previewText,
          ...previewScopeArgs,
          context_override: previewContextArgs,
        },
      });
      setPreview(nextPreview);
    } catch (e) {
      setPreview(null);
      setError(String(e));
    } finally {
      setPreviewing(false);
    }
  }, [previewContextArgs, previewScopeArgs, previewText, selected]);

  const runPreviewBenchmark = useCallback(async () => {
    if (!selected) return;

    const rounds = 3;
    const collected: PromptPreviewResponse[] = [];

    setBenchmarkingPreview(true);
    try {
      setError(null);
      setPreviewBenchmark(null);
      setPreviewBenchmarkError(null);
      const { invoke } = await import('@tauri-apps/api/core');

      for (let index = 0; index < rounds; index += 1) {
        const nextPreview = await invoke<PromptPreviewResponse>('preview_prompt', {
          req: {
            prompt: selected,
            transcript: previewText,
            ...previewScopeArgs,
            context_override: previewContextArgs,
          },
        });
        collected.push(nextPreview);
      }

      const lastPreview = collected[collected.length - 1] ?? null;
      setPreview(lastPreview);
      if (collected.length > 0) {
        setPreviewBenchmark(buildPromptPreviewBenchmark(collected));
      }
    } catch (e) {
      const failedRound = Math.min(collected.length + 1, rounds);
      setPreviewBenchmarkError(`Preview benchmark failed on round ${failedRound}/${rounds}: ${String(e)}`);
    } finally {
      setBenchmarkingPreview(false);
    }
  }, [previewContextArgs, previewScopeArgs, previewText, selected]);

  const save = useCallback(async () => {
    if (!cfg || !prompts) return;
    try {
      setSaving(true);
      const { invoke } = await import('@tauri-apps/api/core');

      let nextDefaultPromptId = defaultPromptId;
      if (nextDefaultPromptId && !prompts.some((prompt) => prompt.id === nextDefaultPromptId)) {
        nextDefaultPromptId = null;
      }

      const nextCfg: AppConfig = {
        ...cfg,
        defaults: {
          ...cfg.defaults,
          prompt_id: nextDefaultPromptId,
        },
        profiles: cfg.profiles.map((profile) => ({
          ...profile,
          overrides:
            profile.overrides.prompt_id && !prompts.some((prompt) => prompt.id === profile.overrides.prompt_id)
              ? { ...profile.overrides, prompt_id: undefined }
              : profile.overrides,
        })),
        prompts,
      };

      await invoke('set_config', { cfg: nextCfg });
      setCfg(nextCfg);
      setDefaultPromptId(nextCfg.defaults.prompt_id ?? null);
      setDirty(false);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }, [cfg, defaultPromptId, prompts]);

  const mutateSelected = useCallback(
    (mutate: (prompt: PromptTemplate) => PromptTemplate) => {
      if (!prompts || !selected) return;
      setPrompts(prompts.map((prompt) => (prompt.id === selected.id ? mutate(prompt) : prompt)));
      setDirty(true);
    },
    [prompts, selected],
  );

  if (!prompts) {
    return (
      <div style={{ padding: 'var(--space-32)' }}>
        <div className="vw-type-title">Prompt Library</div>
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)' }}>
          Loading…
        </div>
      </div>
    );
  }

  return (
    <div
      style={{
        height: '100%',
        display: 'grid',
        gridTemplateColumns: '280px 1fr',
      }}
    >
      <div
        style={{
          borderRight: '1px solid var(--stroke-card)',
          paddingTop: 40,
          paddingLeft: 12,
          paddingRight: 12,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 'var(--space-12)' }}>
          <div className="vw-type-subtitle">Prompt Library</div>
          <button
            type="button"
            className="vw-button vw-button--ghost vw-iconButton"
            aria-label="Add prompt"
            onClick={() => {
              const prompt = newPrompt();
              setPrompts([...(prompts ?? []), prompt]);
              setSelectedId(prompt.id);
              setDirty(true);
            }}
          >
            +
          </button>
        </div>

        <div style={{ marginTop: 'var(--space-12)', display: 'grid', gap: 'var(--space-8)' }}>
          {prompts.map((prompt) => {
            const isSelected = prompt.id === selectedId;
            const isDefault = effectiveDefaultPromptId === prompt.id;
            return (
              <button
                key={prompt.id}
                type="button"
                onClick={() => setSelectedId(prompt.id)}
                style={{
                  padding: 12,
                  borderRadius: 'var(--radius-card)',
                  border: '1px solid transparent',
                  background: isSelected ? 'rgba(255,255,255,0.18)' : 'transparent',
                  cursor: 'pointer',
                  display: 'grid',
                  gap: 4,
                  textAlign: 'left',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 'var(--space-8)' }}>
                  <div className="vw-type-bodyStrong" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {prompt.title || 'Untitled Prompt'}
                  </div>
                  {isDefault ? (
                    <span className="vw-type-caption" style={{ color: 'var(--color-accent)' }}>
                      Default
                    </span>
                  ) : null}
                </div>
                <div className="vw-type-caption">
                  {prompt.mode === 'Assistant' ? 'Assistant' : 'Enhancer'}
                </div>
                <div
                  className="vw-type-caption"
                  style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                >
                  {prompt.trigger_words.length > 0 ? prompt.trigger_words.join(', ') : 'No trigger words'}
                </div>
              </button>
            );
          })}
        </div>
      </div>

      <div style={{ padding: 'var(--space-32)', overflowY: 'auto' }}>
        <div className="vw-type-title">Prompt</div>

        {error ? (
          <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            {error}
          </div>
        ) : null}

        {!selected ? (
          <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)' }}>
            Select a prompt.
          </div>
        ) : (
          <div style={{ marginTop: 'var(--space-24)', display: 'grid', gap: 'var(--space-16)' }}>
            <Section title="Definition" subtitle="Prompt templates shape cleanup, rewrite, or assistant behavior after transcription.">
              <label style={{ display: 'grid', gap: 6 }}>
                <span className="vw-type-caption">Title</span>
                <input
                  className="vw-input"
                  aria-label="Prompt title"
                  value={selected.title}
                  onChange={(e) => {
                    mutateSelected((prompt) => ({ ...prompt, title: e.target.value }));
                  }}
                />
              </label>

              <label style={{ display: 'grid', gap: 6 }}>
                <span className="vw-type-caption">Mode</span>
                <select
                  className="vw-input"
                  aria-label="Prompt mode"
                  value={selected.mode}
                  onChange={(e) => {
                    mutateSelected((prompt) => ({
                      ...prompt,
                      mode: e.target.value === 'Assistant' ? 'Assistant' : 'Enhancer',
                    }));
                  }}
                >
                  <option value="Enhancer">Enhancer</option>
                  <option value="Assistant">Assistant</option>
                </select>
              </label>

              <label style={{ display: 'grid', gap: 6 }}>
                <span className="vw-type-caption">Trigger words</span>
                <textarea
                  className="vw-input"
                  aria-label="Prompt trigger words"
                  rows={4}
                  value={selected.trigger_words.join('\n')}
                  onChange={(e) => {
                    const triggerWords = e.target.value
                      .split(/\r?\n|,/)
                      .map((value) => value.trim())
                      .filter((value) => value.length > 0);
                    mutateSelected((prompt) => ({ ...prompt, trigger_words: triggerWords }));
                  }}
                />
              </label>

              <label style={{ display: 'grid', gap: 6 }}>
                <span className="vw-type-caption">Instructions</span>
                <textarea
                  className="vw-input"
                  aria-label="Prompt instructions"
                  rows={12}
                  value={selected.prompt_text}
                  onChange={(e) => {
                    mutateSelected((prompt) => ({ ...prompt, prompt_text: e.target.value }));
                  }}
                />
              </label>

              <div style={{ display: 'flex', gap: 'var(--space-12)', flexWrap: 'wrap' }}>
                <button
                  type="button"
                  className="vw-button vw-button--secondary"
                  onClick={() => {
                    const duplicate = {
                      ...selected,
                      id: crypto.randomUUID(),
                      title: selected.title.trim().length > 0 ? `${selected.title} Copy` : 'Prompt Copy',
                    };
                    setPrompts([...(prompts ?? []), duplicate]);
                    setSelectedId(duplicate.id);
                    setDirty(true);
                  }}
                >
                  Duplicate
                </button>

                {effectiveDefaultPromptId === selected.id ? (
                  <button
                    type="button"
                    className="vw-button vw-button--secondary"
                    onClick={() => {
                      setDefaultPromptId(null);
                      setDirty(true);
                    }}
                  >
                    Use Automatic Default
                  </button>
                ) : (
                  <button
                    type="button"
                    className="vw-button vw-button--secondary"
                    onClick={() => {
                      setDefaultPromptId(selected.id);
                      setDirty(true);
                    }}
                  >
                    Set As Default
                  </button>
                )}

                <button
                  type="button"
                  className="vw-button vw-button--secondary"
                  disabled={prompts.length <= 1}
                  onClick={() => {
                    const nextPrompts = prompts.filter((prompt) => prompt.id !== selected.id);
                    setPrompts(nextPrompts);
                    setSelectedId(nextPrompts[0]?.id ?? null);
                    if (defaultPromptId === selected.id) {
                      setDefaultPromptId(null);
                    }
                    setDirty(true);
                  }}
                >
                  Delete
                </button>
              </div>
            </Section>

            <Section
              title="Preview"
              subtitle="Run the selected prompt against the effective LLM config for the current foreground app/profile without inserting text or writing history. The benchmark button repeats the same preview 3 times for rough latency and stability measurement; it does not measure the full recording stop path."
            >
              <label style={{ display: 'grid', gap: 6 }}>
                <span className="vw-type-caption">Preview scope</span>
                <select
                  className="vw-input"
                  aria-label="Preview scope"
                  value={previewScope}
                  onChange={(e) => setPreviewScope(e.target.value as PreviewScope)}
                >
                  <option value="current_app">Current foreground app</option>
                  <option value="defaults">Global defaults</option>
                  {selectableProfiles.map((profile) => (
                    <option key={profile.id} value={`profile:${profile.id}`}>
                      Profile: {profile.name}
                    </option>
                  ))}
                </select>
              </label>

              <label style={{ display: 'grid', gap: 6 }}>
                <span className="vw-type-caption">Sample transcript</span>
                <textarea
                  className="vw-input"
                  aria-label="Sample transcript"
                  rows={5}
                  value={previewText}
                  onChange={(e) => setPreviewText(e.target.value)}
                />
              </label>

              <div style={{ display: 'grid', gap: 'var(--space-12)' }}>
                <div className="vw-type-caption">
                  Sample context overrides the live clipboard/selection/window text for preview only. It does not affect saved config or runtime sessions.
                </div>

                <label style={{ display: 'grid', gap: 6 }}>
                  <span className="vw-type-caption">Sample selected text</span>
                  <textarea
                    className="vw-input"
                    aria-label="Sample selected text"
                    rows={4}
                    value={previewContext.selected_text}
                    onChange={(e) =>
                      setPreviewContext((current) => ({ ...current, selected_text: e.target.value }))
                    }
                  />
                </label>

                <label style={{ display: 'grid', gap: 6 }}>
                  <span className="vw-type-caption">Sample clipboard text</span>
                  <textarea
                    className="vw-input"
                    aria-label="Sample clipboard text"
                    rows={3}
                    value={previewContext.clipboard}
                    onChange={(e) =>
                      setPreviewContext((current) => ({ ...current, clipboard: e.target.value }))
                    }
                  />
                </label>

                <label style={{ display: 'grid', gap: 6 }}>
                  <span className="vw-type-caption">Sample window context</span>
                  <textarea
                    className="vw-input"
                    aria-label="Sample window context"
                    rows={3}
                    value={previewContext.window_context}
                    onChange={(e) =>
                      setPreviewContext((current) => ({ ...current, window_context: e.target.value }))
                    }
                  />
                </label>

                <label style={{ display: 'grid', gap: 6 }}>
                  <span className="vw-type-caption">Sample screenshot image</span>
                  <input
                    className="vw-input"
                    aria-label="Sample screenshot image"
                    type="file"
                    accept="image/*"
                    onChange={async (e) => {
                      const file = e.target.files?.[0] ?? null;
                      if (!file) {
                        setPreviewContext((current) => ({
                          ...current,
                          screenshot_data_url: '',
                          screenshot_name: '',
                        }));
                        return;
                      }
                      try {
                        const dataUrl = await readFileAsDataUrl(file);
                        setPreviewContext((current) => ({
                          ...current,
                          screenshot_data_url: dataUrl,
                          screenshot_name: file.name,
                        }));
                        setError(null);
                      } catch (err) {
                        setError(String(err));
                      } finally {
                        e.target.value = '';
                      }
                    }}
                  />
                </label>

                {previewContext.screenshot_data_url ? (
                  <div style={{ display: 'grid', gap: 6 }}>
                    <div className="vw-type-caption">
                      Loaded screenshot: {previewContext.screenshot_name || 'Image'}
                    </div>
                    <img
                      alt="Sample screenshot preview"
                      src={previewContext.screenshot_data_url}
                      style={{
                        maxWidth: '100%',
                        maxHeight: 160,
                        objectFit: 'contain',
                        borderRadius: 12,
                        border: '1px solid var(--stroke-card)',
                        background: 'var(--surface-elevated)',
                      }}
                    />
                    <div>
                      <button
                        type="button"
                        className="vw-button vw-button--secondary"
                        onClick={() =>
                          setPreviewContext((current) => ({
                            ...current,
                            screenshot_data_url: '',
                            screenshot_name: '',
                          }))
                        }
                      >
                        Clear Screenshot
                      </button>
                    </div>
                  </div>
                ) : null}
              </div>

              <div style={{ display: 'flex', gap: 'var(--space-12)', flexWrap: 'wrap' }}>
                <button
                  type="button"
                  className="vw-button vw-button--primary"
                  disabled={previewing || benchmarkingPreview}
                  onClick={() => {
                    void runPreview();
                  }}
                >
                  {previewing ? 'Running…' : 'Run Preview'}
                </button>

                <button
                  type="button"
                  className="vw-button vw-button--secondary"
                  disabled={previewing || benchmarkingPreview}
                  onClick={() => {
                    void runPreviewBenchmark();
                  }}
                >
                  {benchmarkingPreview ? 'Benchmarking…' : 'Run 3-Round Preview Benchmark'}
                </button>

                {preview ? (
                  <div style={{ display: 'grid', gap: 2, alignSelf: 'center' }}>
                    <div className="vw-type-caption">
                      {preview.provider_kind} • {preview.api_kind} • {preview.model} • {formatPreviewLatency(preview)}
                    </div>
                    <div className="vw-type-caption">{formatPreviewScope(preview)}</div>
                    {formatPreviewVisualRuntime(preview) ? (
                      <div className="vw-type-caption">{formatPreviewVisualRuntime(preview)}</div>
                    ) : null}
                  </div>
                ) : null}
              </div>

              {previewBenchmark ? (
                <div
                  className="vw-type-caption"
                  style={{
                    whiteSpace: 'pre-wrap',
                    wordBreak: 'break-word',
                    padding: 'var(--space-12)',
                    border: '1px solid var(--stroke-card)',
                    borderRadius: 12,
                    background: 'var(--surface-elevated)',
                  }}
                >
                  {[
                    `preview benchmark • ${previewBenchmark.rounds} rounds`,
                    previewBenchmark.provider_kind,
                    previewBenchmark.api_kind,
                    previewBenchmark.model,
                    formatBenchmarkLatency(
                      previewBenchmark.elapsed_min_ms,
                      previewBenchmark.elapsed_avg_ms,
                      previewBenchmark.elapsed_max_ms,
                      'total',
                    ),
                    formatBenchmarkLatency(
                      previewBenchmark.first_token_min_ms,
                      previewBenchmark.first_token_avg_ms,
                      previewBenchmark.first_token_max_ms,
                      'first token',
                    ),
                    previewBenchmark.last_visual_label
                      ? `visual ${previewBenchmark.last_visual_label}`
                      : null,
                    previewBenchmark.visual_variant_count > 1
                      ? `visual variants ${previewBenchmark.visual_variant_count}`
                      : null,
                    formatBenchmarkLatency(
                      previewBenchmark.capture_elapsed_min_ms,
                      previewBenchmark.capture_elapsed_avg_ms,
                      previewBenchmark.capture_elapsed_max_ms,
                      'capture',
                    ),
                    formatBenchmarkLatency(
                      previewBenchmark.ocr_elapsed_min_ms,
                      previewBenchmark.ocr_elapsed_avg_ms,
                      previewBenchmark.ocr_elapsed_max_ms,
                      'ocr',
                    ),
                    formatBenchmarkLatency(
                      previewBenchmark.ocr_first_token_min_ms,
                      previewBenchmark.ocr_first_token_avg_ms,
                      previewBenchmark.ocr_first_token_max_ms,
                      'ocr first token',
                    ),
                    formatBenchmarkCountRange(
                      previewBenchmark.ocr_text_chars_min,
                      previewBenchmark.ocr_text_chars_avg,
                      previewBenchmark.ocr_text_chars_max,
                      'ocr text',
                      'chars',
                    ),
                    formatBenchmarkCountRange(
                      previewBenchmark.input_tokens_min,
                      previewBenchmark.input_tokens_avg,
                      previewBenchmark.input_tokens_max,
                      'input',
                      'tok',
                    ),
                    formatBenchmarkCountRange(
                      previewBenchmark.cached_input_tokens_min,
                      previewBenchmark.cached_input_tokens_avg,
                      previewBenchmark.cached_input_tokens_max,
                      'cache',
                      'tok',
                    ),
                    previewBenchmark.capture_fallback_count > 0
                      ? `capture fallbacks ${previewBenchmark.capture_fallback_count}/${previewBenchmark.rounds}`
                      : null,
                    `warnings ${previewBenchmark.warning_count}/${previewBenchmark.rounds}`,
                    `final variants ${previewBenchmark.final_output_variant_count}`,
                    `raw variants ${previewBenchmark.raw_output_variant_count}`,
                    `Last output: ${previewBenchmark.last_final_output}`,
                  ]
                    .filter((value): value is string => Boolean(value))
                    .join(' • ')}
                  {previewBenchmark.sample_capture_fallback_reason
                    ? `\nSample capture fallback: ${previewBenchmark.sample_capture_fallback_reason}`
                    : ''}
                  {previewBenchmark.sample_warning ? `\nSample warning: ${previewBenchmark.sample_warning}` : ''}
                </div>
              ) : null}

              {previewBenchmarkError ? (
                <div
                  className="vw-type-caption"
                  style={{
                    whiteSpace: 'pre-wrap',
                    wordBreak: 'break-word',
                    padding: 'var(--space-12)',
                    border: '1px solid var(--stroke-card)',
                    borderRadius: 12,
                    background: 'var(--surface-elevated)',
                    color: 'var(--color-danger-fg)',
                  }}
                >
                  {previewBenchmarkError}
                </div>
              ) : null}

              {preview ? (
                <div style={{ display: 'grid', gap: 'var(--space-12)' }}>
                  {preview.warning ? (
                    <div
                      className="vw-type-caption"
                      style={{
                        whiteSpace: 'pre-wrap',
                        wordBreak: 'break-word',
                        padding: 'var(--space-12)',
                        border: '1px solid var(--stroke-card)',
                        borderRadius: 12,
                        background: 'var(--surface-elevated)',
                      }}
                    >
                      Warning: {preview.warning}
                    </div>
                  ) : null}

                  <label style={{ display: 'grid', gap: 6 }}>
                    <span className="vw-type-caption">Final output</span>
                    <textarea className="vw-input" rows={5} value={preview.final_output} readOnly />
                  </label>

                  <label style={{ display: 'grid', gap: 6 }}>
                    <span className="vw-type-caption">Raw model output</span>
                    <textarea className="vw-input" rows={5} value={preview.raw_output} readOnly />
                  </label>

                  <label style={{ display: 'grid', gap: 6 }}>
                    <span className="vw-type-caption">Rendered system message</span>
                    <textarea className="vw-input" rows={8} value={preview.system_message} readOnly />
                  </label>

                  <label style={{ display: 'grid', gap: 6 }}>
                    <span className="vw-type-caption">Rendered user message</span>
                    <textarea className="vw-input" rows={6} value={preview.user_message} readOnly />
                  </label>
                </div>
              ) : null}
            </Section>

            {dirty ? (
              <div style={{ display: 'flex', gap: 'var(--space-12)' }}>
                <button
                  type="button"
                  className="vw-button vw-button--secondary"
                  disabled={saving}
                  onClick={() => {
                    void refresh();
                  }}
                >
                  Cancel
                </button>
                <button type="button" className="vw-button vw-button--primary" disabled={saving} onClick={() => void save()}>
                  Save Changes
                </button>
              </div>
            ) : (
              <div className="vw-type-caption">Starter prompts are seeded automatically; you can freely edit or duplicate them here.</div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
