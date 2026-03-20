import { useCallback, useEffect, useMemo, useState } from 'react';

import type {
  HistoryEntry,
  PromptPreviewResponse,
  VisualCaptureScope,
  VisualContextMode,
  VisualContextRuntime,
} from '../lib/types';
import { formatBenchmarkCountRange, formatBenchmarkLatency, summarizeNumbers } from './llmBenchmark';

type HistoryReplayBenchmark = {
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

function buildReplayBenchmark(rounds: PromptPreviewResponse[]): HistoryReplayBenchmark {
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

function formatTime(tsUnixMs: number): string {
  const d = new Date(tsUnixMs);
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  return `${hh}:${mm}`;
}

function formatModel(provider?: string | null, model?: string | null): string | null {
  const parts = [provider, model].filter((value): value is string => Boolean(value && value.trim()));
  if (parts.length === 0) return null;
  return parts.join(' / ');
}

function formatLatency(entry: HistoryEntry): string | null {
  const parts: string[] = [];

  if (entry.transcription_ms != null) {
    parts.push(`STT ${entry.transcription_ms} ms`);
  }
  if (entry.enhancement_ms != null) {
    parts.push(`LLM ${entry.enhancement_ms} ms`);
  }
  if (entry.enhancement_first_token_ms != null) {
    parts.push(`LLM first token ${entry.enhancement_first_token_ms} ms`);
  }
  if (entry.enhancement_input_tokens != null) {
    parts.push(`LLM input ${entry.enhancement_input_tokens} tok`);
  }
  if (entry.enhancement_cached_input_tokens != null) {
    parts.push(`LLM cache ${entry.enhancement_cached_input_tokens} tok`);
  }
  if (entry.visual_context_runtime?.screenshot_capture_elapsed_ms != null) {
    parts.push(`Capture ${entry.visual_context_runtime.screenshot_capture_elapsed_ms} ms`);
  }
  if (entry.visual_context_runtime?.screen_ocr_source != null) {
    parts.push(`OCR ${entry.visual_context_runtime.screen_ocr_source}`);
  }
  if (entry.visual_context_runtime?.screen_ocr_elapsed_ms != null) {
    parts.push(`OCR ${entry.visual_context_runtime.screen_ocr_elapsed_ms} ms`);
  }
  if (entry.visual_context_runtime?.screen_ocr_first_token_ms != null) {
    parts.push(`OCR first token ${entry.visual_context_runtime.screen_ocr_first_token_ms} ms`);
  }
  if (entry.visual_context_runtime?.screen_ocr_text_chars != null) {
    parts.push(`OCR text ${entry.visual_context_runtime.screen_ocr_text_chars} chars`);
  }

  if (parts.length === 0) return null;
  return parts.join(' • ');
}

function normalizeLegacyVisualContext(
  flags?: HistoryEntry['context_flags'] | null,
): { mode: VisualContextMode; scope: VisualCaptureScope } | null {
  if (!flags) return null;

  const legacyFlags = flags as typeof flags & { use_ocr?: boolean };
  return {
    mode: flags.visual_context_mode ?? (legacyFlags.use_ocr ? 'screenshot' : 'off'),
    scope: flags.visual_capture_scope ?? 'display',
  };
}

function formatRuntimeVisualLabel(
  runtime?: VisualContextRuntime | null,
  configured?: { mode: VisualContextMode; scope: VisualCaptureScope } | null,
): string | null {
  const mode = configured?.mode ?? runtime?.mode ?? null;
  const scope = configured?.scope ?? runtime?.capture_scope ?? null;

  if (!mode || mode === 'off') return null;

  const parts: string[] = [];
  const dispatch = runtime?.dispatch ?? null;
  const visualLabel =
    dispatch == null || dispatch === mode ? `visual:${mode}` : `visual:${mode}->${dispatch}`;
  parts.push(visualLabel);
  if (scope) {
    parts.push(`capture:${formatCaptureScope(scope)}`);
  }
  if (runtime?.capture_actual_scope && runtime.capture_actual_scope !== scope) {
    parts.push(`captured:${formatCaptureScope(runtime.capture_actual_scope)}`);
  }
  if (runtime?.capture_fallback_reason) {
    parts.push(`fallback:${runtime.capture_fallback_reason}`);
  }
  if (runtime?.dispatch === 'ocr' && runtime.screen_ocr_source) {
    parts.push(`ocr:${runtime.screen_ocr_source}`);
  }

  return parts.join(', ');
}

function formatContextFlags(entry: HistoryEntry): string | null {
  const flags = entry.context_flags;
  const configuredVisual = normalizeLegacyVisualContext(flags);
  if (!flags && !entry.visual_context_runtime) return null;

  const enabled: string[] = [];
  if (flags?.use_clipboard) enabled.push('clipboard');
  if (flags?.use_selected_text) enabled.push('selection');
  if (flags?.use_window_context) enabled.push('window');
  if (flags?.use_custom_vocabulary) enabled.push('vocabulary');

  const visualLabel = formatRuntimeVisualLabel(entry.visual_context_runtime, configuredVisual);
  if (visualLabel) {
    enabled.push(visualLabel);
  } else if (!flags && entry.visual_context_runtime?.dispatch && entry.visual_context_runtime.dispatch !== 'off') {
    enabled.push(`visual:${entry.visual_context_runtime.dispatch}`);
  }

  if (enabled.length === 0) return null;
  return enabled.join(', ');
}

function formatReplayScope(preview: PromptPreviewResponse): string | null {
  const parts: string[] = [];

  if (preview.app_process_name) {
    parts.push(preview.app_process_name);
  }
  if (preview.matched_profile_name) {
    parts.push(`Profile ${preview.matched_profile_name}`);
  } else {
    parts.push('Using defaults');
  }

  if (parts.length === 0) return null;
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

function buildMetadata(entry: HistoryEntry): string[] {
  const details = [`Stage: ${entry.stage}`];

  if (entry.matched_profile_name) details.push(`Profile: ${entry.matched_profile_name}`);
  if (entry.prompt_title) details.push(`Prompt: ${entry.prompt_title}`);
  if (entry.detected_trigger_word) details.push(`Trigger: ${entry.detected_trigger_word}`);

  const stt = formatModel(entry.stt_provider, entry.stt_model);
  if (stt) details.push(`STT: ${stt}`);

  const llm = formatModel(entry.llm_provider, entry.llm_model);
  if (llm) details.push(`LLM: ${llm}`);

  const latency = formatLatency(entry);
  if (latency) details.push(`Latency: ${latency}`);

  const context = formatContextFlags(entry);
  if (context) details.push(`Context: ${context}`);

  return details;
}

export function HistoryPage() {
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [replayingId, setReplayingId] = useState<string | null>(null);
  const [benchmarkingId, setBenchmarkingId] = useState<string | null>(null);
  const [replayPreviewById, setReplayPreviewById] = useState<Record<string, PromptPreviewResponse>>({});
  const [replayBenchmarkById, setReplayBenchmarkById] = useState<Record<string, HistoryReplayBenchmark>>({});
  const [replayErrorById, setReplayErrorById] = useState<Record<string, string>>({});
  const [replayBenchmarkErrorById, setReplayBenchmarkErrorById] = useState<Record<string, string>>({});

  const refresh = useCallback(async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const list = await invoke<HistoryEntry[]>('get_history');
      setEntries(list.slice().reverse());
      setError(null);
    } catch (e) {
      setError(String(e));
      setEntries([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const rows = useMemo(() => entries ?? [], [entries]);
  const previewHistoryEntry = useCallback(async (id: string) => {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<PromptPreviewResponse>('preview_history_entry', { id });
  }, []);

  const clearReplayError = useCallback((id: string) => {
    setReplayErrorById((prev) => {
      const next = { ...prev };
      delete next[id];
      return next;
    });
  }, []);

  const clearReplayBenchmark = useCallback((id: string) => {
    setReplayBenchmarkById((prev) => {
      const next = { ...prev };
      delete next[id];
      return next;
    });
  }, []);

  const clearReplayBenchmarkError = useCallback((id: string) => {
    setReplayBenchmarkErrorById((prev) => {
      const next = { ...prev };
      delete next[id];
      return next;
    });
  }, []);

  const runReplay = useCallback(
    async (id: string) => {
      try {
        setReplayingId(id);
        clearReplayError(id);
        clearReplayBenchmarkError(id);
        const preview = await previewHistoryEntry(id);
        setReplayPreviewById((prev) => ({ ...prev, [id]: preview }));
      } catch (e) {
        setReplayErrorById((prev) => ({ ...prev, [id]: String(e) }));
      } finally {
        setReplayingId((current) => (current === id ? null : current));
      }
    },
    [clearReplayBenchmarkError, clearReplayError, previewHistoryEntry],
  );

  const runReplayBenchmark = useCallback(
    async (id: string) => {
      const rounds = 3;
      const collected: PromptPreviewResponse[] = [];
      let failedRound = 0;

      try {
        setBenchmarkingId(id);
        clearReplayError(id);
        clearReplayBenchmark(id);
        clearReplayBenchmarkError(id);

        for (let round = 0; round < rounds; round += 1) {
          failedRound = round + 1;
          const preview = await previewHistoryEntry(id);
          collected.push(preview);
          setReplayPreviewById((prev) => ({ ...prev, [id]: preview }));
        }

        setReplayBenchmarkById((prev) => ({ ...prev, [id]: buildReplayBenchmark(collected) }));
      } catch (e) {
        setReplayBenchmarkErrorById((prev) => ({
          ...prev,
          [id]: `Replay benchmark failed on round ${failedRound}/${rounds}: ${String(e)}`,
        }));
      } finally {
        setBenchmarkingId((current) => (current === id ? null : current));
      }
    },
    [
      clearReplayBenchmark,
      clearReplayBenchmarkError,
      clearReplayError,
      previewHistoryEntry,
    ],
  );

  return (
    <div style={{ padding: 'var(--space-32)' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <div className="vw-type-title">History</div>
        <button
          type="button"
          className="vw-button vw-button--secondary"
          onClick={async () => {
            try {
              const { invoke } = await import('@tauri-apps/api/core');
              await invoke('clear_history');
              await refresh();
            } catch (e) {
              setError(String(e));
            }
          }}
        >
          Clear All
        </button>
      </div>

      {error ? (
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
          {error}
        </div>
      ) : null}

      <div style={{ marginTop: 'var(--space-16)' }}>
        <div className="vw-type-caption" style={{ marginBottom: 'var(--space-12)' }}>
          Recover final inserts, raw transcripts, prompt usage, provider metadata, and latency from recent sessions.
        </div>

        {rows.map((r, idx) => {
          const app = r.app_process_name ?? '—';
          const text = r.text && r.text.trim().length > 0 ? r.text : (r.error ?? '');
          const rowId = r.id && r.id.trim().length > 0 ? r.id : null;
          const metadata = buildMetadata(r);
          const rawTranscript =
            r.raw_transcript && r.raw_transcript.trim().length > 0 ? r.raw_transcript : null;
          const enhancedText =
            r.enhanced_text && r.enhanced_text.trim().length > 0 ? r.enhanced_text : null;
          const showRawTranscript = rawTranscript && rawTranscript !== text;
          const showEnhancedText = enhancedText && enhancedText !== text;
          const replayPreview = rowId ? replayPreviewById[rowId] ?? null : null;
          const replayBenchmark = rowId ? replayBenchmarkById[rowId] ?? null : null;
          const replayError = rowId ? replayErrorById[rowId] ?? null : null;
          const replayBenchmarkError = rowId ? replayBenchmarkErrorById[rowId] ?? null : null;
          const isReplaying = rowId != null && replayingId === rowId;
          const isBenchmarking = rowId != null && benchmarkingId === rowId;
          const replayScope = replayPreview ? formatReplayScope(replayPreview) : null;
          const replayVisualRuntime = replayPreview ? formatPreviewVisualRuntime(replayPreview) : null;

          return (
            <div
              key={rowId ?? `${r.ts_unix_ms}-${r.text}-${idx}`}
              className="vw-historyRow"
              style={{
                padding: 'var(--space-16) var(--space-12)',
                borderBottom: '1px solid var(--stroke-card)',
                display: 'grid',
                gap: 'var(--space-10)',
              }}
            >
              <div
                style={{
                  display: 'flex',
                  gap: 'var(--space-12)',
                  justifyContent: 'space-between',
                  alignItems: 'flex-start',
                }}
              >
                <div style={{ display: 'grid', gap: 4 }}>
                  <div className="vw-type-caption" style={{ color: 'var(--text-secondary)' }}>
                    {formatTime(r.ts_unix_ms)} • {app}
                  </div>
                  <div className="vw-type-bodyStrong" style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                    {text || 'No recoverable text'}
                  </div>
                </div>

                <div className="vw-historyActions" style={{ display: 'flex', gap: 'var(--space-8)', justifyContent: 'flex-end' }}>
                  <button
                    type="button"
                    className="vw-button vw-button--ghost"
                    aria-label="Enhance Again"
                    disabled={!rowId || isReplaying || isBenchmarking}
                    onClick={async () => {
                      if (!rowId) return;
                      await runReplay(rowId);
                    }}
                  >
                    {isReplaying ? 'Replaying…' : 'Enhance Again'}
                  </button>

                  <button
                    type="button"
                    className="vw-button vw-button--ghost"
                    aria-label="Benchmark Again"
                    disabled={!rowId || isReplaying || isBenchmarking}
                    onClick={async () => {
                      if (!rowId) return;
                      await runReplayBenchmark(rowId);
                    }}
                  >
                    {isBenchmarking ? 'Benchmarking…' : 'Benchmark Again'}
                  </button>

                  <button
                    type="button"
                    className="vw-button vw-button--ghost vw-iconButton"
                    aria-label="Copy"
                    onClick={async () => {
                      try {
                        await navigator.clipboard.writeText(text);
                      } catch {
                        // ignore
                      }
                    }}
                  >
                    ⧉
                  </button>

                  <button
                    type="button"
                    className="vw-button vw-button--ghost vw-iconButton"
                    aria-label="Delete"
                    onClick={async () => {
                      try {
                        const { invoke } = await import('@tauri-apps/api/core');
                        if (rowId) {
                          await invoke('delete_history_entry_by_id', { id: rowId });
                        } else {
                          await invoke('delete_history_entry', { tsUnixMs: r.ts_unix_ms, text: r.text });
                        }
                        await refresh();
                      } catch (e) {
                        setError(String(e));
                      }
                    }}
                  >
                    🗑
                  </button>
                </div>
              </div>

              {metadata.length > 0 ? (
                <div className="vw-type-caption" style={{ color: 'var(--text-secondary)', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                  {metadata.join(' • ')}
                </div>
              ) : null}

              {showRawTranscript ? (
                <div className="vw-type-caption" style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                  Transcript: {rawTranscript}
                </div>
              ) : null}

              {showEnhancedText ? (
                <div className="vw-type-caption" style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                  Enhanced Output: {enhancedText}
                </div>
              ) : null}

              {r.warning ? (
                <div className="vw-type-caption" style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                  Warning: {r.warning}
                </div>
              ) : null}

              {replayPreview ? (
                <div
                  className="vw-type-caption"
                  style={{
                    whiteSpace: 'pre-wrap',
                    wordBreak: 'break-word',
                    display: 'grid',
                    gap: 4,
                    padding: 'var(--space-12)',
                    border: '1px solid var(--stroke-card)',
                    borderRadius: 12,
                    background: 'var(--surface-elevated)',
                  }}
                >
                  <div className="vw-type-bodyStrong">Replay Preview: {replayPreview.final_output}</div>
                  <div style={{ color: 'var(--text-secondary)' }}>
                    {formatModel(replayPreview.provider_kind, replayPreview.model) ?? replayPreview.model} • {replayPreview.elapsed_ms} ms total
                    {replayPreview.first_token_ms != null ? ` • ${replayPreview.first_token_ms} ms first token` : ''}
                    {replayPreview.input_tokens != null ? ` • ${replayPreview.input_tokens} input tok` : ''}
                    {replayPreview.cached_input_tokens != null ? ` • ${replayPreview.cached_input_tokens} cached tok` : ''}
                  </div>
                  {replayScope ? <div style={{ color: 'var(--text-secondary)' }}>{replayScope}</div> : null}
                  {replayVisualRuntime ? <div style={{ color: 'var(--text-secondary)' }}>{replayVisualRuntime}</div> : null}
                  {replayPreview.warning ? <div>Warning: {replayPreview.warning}</div> : null}
                  <div style={{ color: 'var(--text-secondary)' }}>
                    Uses the saved transcript and prompt when available. Clipboard and selection context are not replayed from history.
                  </div>
                </div>
              ) : null}

              {replayBenchmark ? (
                <div
                  className="vw-type-caption"
                  style={{
                    whiteSpace: 'pre-wrap',
                    wordBreak: 'break-word',
                    display: 'grid',
                    gap: 4,
                    padding: 'var(--space-12)',
                    border: '1px solid var(--stroke-card)',
                    borderRadius: 12,
                    background: 'var(--surface-elevated)',
                  }}
                >
                  <div>
                    {[
                      `Replay benchmark • ${replayBenchmark.rounds} rounds`,
                      replayBenchmark.provider_kind,
                      replayBenchmark.api_kind,
                      replayBenchmark.model,
                      formatBenchmarkLatency(
                        replayBenchmark.elapsed_min_ms,
                        replayBenchmark.elapsed_avg_ms,
                        replayBenchmark.elapsed_max_ms,
                        'total',
                      ),
                      formatBenchmarkLatency(
                        replayBenchmark.first_token_min_ms,
                        replayBenchmark.first_token_avg_ms,
                        replayBenchmark.first_token_max_ms,
                        'first token',
                      ),
                      replayBenchmark.last_visual_label
                        ? `visual ${replayBenchmark.last_visual_label}`
                        : null,
                      replayBenchmark.visual_variant_count > 1
                        ? `visual variants ${replayBenchmark.visual_variant_count}`
                        : null,
                      formatBenchmarkLatency(
                        replayBenchmark.capture_elapsed_min_ms,
                        replayBenchmark.capture_elapsed_avg_ms,
                        replayBenchmark.capture_elapsed_max_ms,
                        'capture',
                      ),
                      formatBenchmarkLatency(
                        replayBenchmark.ocr_elapsed_min_ms,
                        replayBenchmark.ocr_elapsed_avg_ms,
                        replayBenchmark.ocr_elapsed_max_ms,
                        'ocr',
                      ),
                      formatBenchmarkLatency(
                        replayBenchmark.ocr_first_token_min_ms,
                        replayBenchmark.ocr_first_token_avg_ms,
                        replayBenchmark.ocr_first_token_max_ms,
                        'ocr first token',
                      ),
                      formatBenchmarkCountRange(
                        replayBenchmark.ocr_text_chars_min,
                        replayBenchmark.ocr_text_chars_avg,
                        replayBenchmark.ocr_text_chars_max,
                        'ocr text',
                        'chars',
                      ),
                      formatBenchmarkCountRange(
                        replayBenchmark.input_tokens_min,
                        replayBenchmark.input_tokens_avg,
                        replayBenchmark.input_tokens_max,
                        'input',
                        'tok',
                      ),
                      formatBenchmarkCountRange(
                        replayBenchmark.cached_input_tokens_min,
                        replayBenchmark.cached_input_tokens_avg,
                        replayBenchmark.cached_input_tokens_max,
                        'cache',
                        'tok',
                      ),
                      replayBenchmark.capture_fallback_count > 0
                        ? `capture fallbacks ${replayBenchmark.capture_fallback_count}/${replayBenchmark.rounds}`
                        : null,
                      `warnings ${replayBenchmark.warning_count}/${replayBenchmark.rounds}`,
                      `final variants ${replayBenchmark.final_output_variant_count}`,
                      `raw variants ${replayBenchmark.raw_output_variant_count}`,
                      `Last output: ${replayBenchmark.last_final_output}`,
                    ]
                      .filter((value): value is string => Boolean(value))
                      .join(' • ')}
                  </div>
                  {replayBenchmark.sample_capture_fallback_reason ? (
                    <div>Sample capture fallback: {replayBenchmark.sample_capture_fallback_reason}</div>
                  ) : null}
                  {replayBenchmark.sample_warning ? <div>Sample warning: {replayBenchmark.sample_warning}</div> : null}
                </div>
              ) : null}

              {replayError ? (
                <div className="vw-type-caption" style={{ color: 'var(--color-danger-fg)', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                  Replay Error: {replayError}
                </div>
              ) : null}

              {replayBenchmarkError ? (
                <div className="vw-type-caption" style={{ color: 'var(--color-danger-fg)', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                  {replayBenchmarkError}
                </div>
              ) : null}

              {r.error ? (
                <div className="vw-type-caption" style={{ color: 'var(--color-danger-fg)', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                  Error: {r.error}
                </div>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
