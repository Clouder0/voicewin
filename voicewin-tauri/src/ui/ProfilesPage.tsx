import { useCallback, useEffect, useMemo, useState } from 'react';

import type {
  AppConfig,
  AppMatcher,
  ContextToggles,
  PlatformCapabilities,
  PowerModeProfile,
  VisualCaptureScope,
  VisualContextMode,
} from '../lib/types';
import { decodePowerModeProfile, encodePowerModeProfile } from '../lib/types';
import {
  defaultBaseUrlForProvider,
  defaultModelForProvider,
  llmSupportsAttachedImages,
  normalizeLlmApiKind,
  normalizeLlmPreflightMode,
  normalizeLlmProviderKind,
  normalizeLlmReasoningEffort,
  normalizeVisualCaptureScope,
  normalizeVisualContextMode,
  resolveVisualContextDispatch,
  screenshotContextWarning,
  shouldRecommendResponsesForCc2OpenAiChatCompletions,
} from './llmConfig';
import {
  contextCapabilityWarnings,
  fallbackPlatformCapabilities,
  foregroundAppCapabilityWarning,
  loadPlatformCapabilities,
} from './platformCapabilities';

type ForegroundAppInfo = {
  process_name?: string | null;
  exe_path?: string | null;
  window_title?: string | null;
};

type ContextOverrideChoice = 'inherit' | 'on' | 'off';

const CONTEXT_OVERRIDE_FIELDS = [
  {
    key: 'use_clipboard',
    title: 'Clipboard context',
    description: 'Include the current clipboard text when enhancement runs.',
  },
  {
    key: 'use_selected_text',
    title: 'Selected text',
    description: 'Best-effort. Use the active selection when the platform can capture it.',
  },
  {
    key: 'use_window_context',
    title: 'Window context',
    description: 'Include the active window title or text snapshot captured at recording start.',
  },
  {
    key: 'use_custom_vocabulary',
    title: 'Custom vocabulary',
    description: 'Include terms from custom_vocabulary.txt in the VoiceWin app data folder when present.',
  },
] as const satisfies ReadonlyArray<{
  key: keyof ContextToggles;
  title: string;
  description: string;
}>;

type ContextOverrideKey = (typeof CONTEXT_OVERRIDE_FIELDS)[number]['key'];
type VisualContextOverrideChoice = 'inherit' | VisualContextMode;
type VisualCaptureScopeOverrideChoice = 'inherit' | VisualCaptureScope;

type MatcherKind = AppMatcher['kind'];

const MATCHER_KIND_OPTIONS: Array<{ kind: MatcherKind; label: string; placeholder: string }> = [
  { kind: 'ProcessNameEquals', label: 'Process name', placeholder: 'code.exe' },
  { kind: 'ExePathEquals', label: 'Executable path', placeholder: 'C:/Program Files/App/app.exe' },
  { kind: 'WindowTitleContains', label: 'Window title contains', placeholder: 'Inbox' },
];

function newProfile(): PowerModeProfile {
  const id = crypto.randomUUID();
  return {
    id,
    name: 'New Profile',
    enabled: true,
    matchers: [blankMatcher('ProcessNameEquals')],
    overrides: {},
  };
}

function blankMatcher(kind: MatcherKind, value = ''): AppMatcher {
  return { kind, value };
}

function matcherLabel(kind: MatcherKind): string {
  return MATCHER_KIND_OPTIONS.find((option) => option.kind === kind)?.label ?? kind;
}

function matcherPlaceholder(kind: MatcherKind): string {
  return MATCHER_KIND_OPTIONS.find((option) => option.kind === kind)?.placeholder ?? '';
}

function summarizeMatchers(matchers: AppMatcher[]): string {
  const filled = matchers
    .map((matcher) => `${matcherLabel(matcher.kind)}: ${matcher.value.trim()}`)
    .filter((value) => !value.endsWith(': '));

  if (filled.length === 0) return 'No target app';
  if (filled.length === 1) return filled[0];
  return `${filled[0]} +${filled.length - 1} more`;
}

function foregroundValueForMatcher(info: ForegroundAppInfo, kind: MatcherKind): string {
  switch (kind) {
    case 'ExePathEquals':
      return info.exe_path?.trim() ?? '';
    case 'WindowTitleContains':
      return info.window_title?.trim() ?? '';
    case 'ProcessNameEquals':
      return info.process_name?.trim() ?? '';
  }
}

function SectionCard({
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
      <div style={{ display: 'grid', gap: 'var(--space-16)' }}>{children}</div>
    </div>
  );
}

function FieldRow({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
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
      <div className="vw-settingRowRight">{children}</div>
    </div>
  );
}

function formatPromptLabel(cfg: AppConfig, promptId: string | null | undefined): string {
  if (!promptId) {
    return cfg.prompts[0] ? `Automatic (${cfg.prompts[0].title})` : 'No prompts loaded';
  }
  return cfg.prompts.find((prompt) => prompt.id === promptId)?.title ?? 'Missing prompt';
}

function formatApiModeLabel(value: string): string {
  if (value === 'stream_generate_content_sse') return 'streamGenerateContent (HTTP SSE)';
  if (value === 'responses_sse') return 'OpenAI Responses (HTTP SSE)';
  return 'Chat Completions (Legacy)';
}

function formatPreflightLabel(value: string): string {
  return value === 'http_connect' ? 'HTTP Connect' : 'Off';
}

function formatReasoningLabel(value: string | null | undefined): string {
  switch (value) {
    case 'minimal':
      return 'Minimal';
    case 'low':
      return 'Low';
    case 'medium':
      return 'Medium';
    case 'high':
      return 'High';
    default:
      return 'Disabled';
  }
}

function formatProviderLabel(value: string): string {
  return normalizeLlmProviderKind(value) === 'gemini' ? 'Google Gemini' : 'OpenAI-Compatible';
}

function contextOverrideChoice(
  context: Partial<ContextToggles> | null | undefined,
  key: ContextOverrideKey,
): ContextOverrideChoice {
  const value = context?.[key];
  if (value === undefined) return 'inherit';
  return value ? 'on' : 'off';
}

function setContextOverrideChoice(
  context: Partial<ContextToggles> | null | undefined,
  key: ContextOverrideKey,
  nextValue: ContextOverrideChoice,
): Partial<ContextToggles> | null {
  const next: Partial<ContextToggles> = { ...(context ?? {}) };
  if (nextValue === 'inherit') {
    delete next[key];
  } else {
    next[key] = nextValue === 'on';
  }
  return Object.keys(next).length > 0 ? next : null;
}

function visualContextOverrideChoice(
  context: Partial<ContextToggles> | null | undefined,
): VisualContextOverrideChoice {
  if (context?.visual_context_mode == null) return 'inherit';
  return normalizeVisualContextMode(context.visual_context_mode);
}

function setVisualContextOverrideChoice(
  context: Partial<ContextToggles> | null | undefined,
  nextValue: VisualContextOverrideChoice,
): Partial<ContextToggles> | null {
  const next: Partial<ContextToggles> = { ...(context ?? {}) };
  if (nextValue === 'inherit') {
    delete next.visual_context_mode;
  } else {
    next.visual_context_mode = nextValue;
  }
  return Object.keys(next).length > 0 ? next : null;
}

function visualCaptureScopeOverrideChoice(
  context: Partial<ContextToggles> | null | undefined,
): VisualCaptureScopeOverrideChoice {
  if (context?.visual_capture_scope == null) return 'inherit';
  return normalizeVisualCaptureScope(context.visual_capture_scope);
}

function setVisualCaptureScopeOverrideChoice(
  context: Partial<ContextToggles> | null | undefined,
  nextValue: VisualCaptureScopeOverrideChoice,
): Partial<ContextToggles> | null {
  const next: Partial<ContextToggles> = { ...(context ?? {}) };
  if (nextValue === 'inherit') {
    delete next.visual_capture_scope;
  } else {
    next.visual_capture_scope = nextValue;
  }
  return Object.keys(next).length > 0 ? next : null;
}

function formatVisualContextModeLabel(value: VisualContextMode): string {
  switch (value) {
    case 'auto':
      return 'Auto';
    case 'screenshot':
      return 'Screenshot Only';
    case 'ocr':
      return 'OCR Only';
    default:
      return 'Off';
  }
}

function formatVisualCaptureScopeLabel(value: VisualCaptureScope): string {
  return value === 'foreground_window' ? 'Foreground Window' : 'Display';
}

export function ProfilesPage() {
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [profiles, setProfiles] = useState<PowerModeProfile[] | null>(null);
  const [platformCapabilities, setPlatformCapabilities] = useState<PlatformCapabilities>(
    fallbackPlatformCapabilities(),
  );
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const c = await invoke<AppConfig>('get_config');
      setCfg(c);
      setPlatformCapabilities(await loadPlatformCapabilities(invoke));
      const decoded = c.profiles.map(decodePowerModeProfile);
      setProfiles(decoded);
      setSelectedId((current) => {
        if (decoded.length === 0) return null;
        if (current && decoded.some((p) => p.id === current)) return current;
        return decoded[0].id;
      });
      setError(null);
    } catch (e) {
      setError(String(e));
      setCfg(null);
      setProfiles([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const selected = useMemo(() => {
    if (!profiles || !selectedId) return null;
    return profiles.find((p) => p.id === selectedId) ?? null;
  }, [profiles, selectedId]);

  const save = useCallback(
    async (nextProfiles: PowerModeProfile[]) => {
      if (!cfg) return;
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const nextCfg: AppConfig = {
          ...cfg,
          profiles: nextProfiles.map(encodePowerModeProfile),
        };
        await invoke('set_config', { cfg: nextCfg });
        setCfg(nextCfg);
        setProfiles(nextProfiles);
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    },
    [cfg],
  );

  const mutateSelected = useCallback(
    (mutate: (profile: PowerModeProfile) => PowerModeProfile) => {
      if (!profiles || !selected) return null;
      const next = profiles.map((profile) => (profile.id === selected.id ? mutate(profile) : profile));
      setProfiles(next);
      return next;
    },
    [profiles, selected],
  );

  const saveSelectedMutation = useCallback(
    async (mutate: (profile: PowerModeProfile) => PowerModeProfile) => {
      const next = mutateSelected(mutate);
      if (next) {
        await save(next);
      }
    },
    [mutateSelected, save],
  );

  const saveCurrentDraft = useCallback(async () => {
    if (profiles) {
      await save(profiles);
    }
  }, [profiles, save]);

  const effectiveProviderKind = useMemo(() => {
    if (!cfg || !selected) return 'openai_compatible';
    return normalizeLlmProviderKind(selected.overrides.llm_provider_kind ?? cfg.defaults.llm_provider_kind);
  }, [cfg, selected]);

  const defaultProviderKind = useMemo(() => {
    if (!cfg) return 'openai_compatible';
    return normalizeLlmProviderKind(cfg.defaults.llm_provider_kind);
  }, [cfg]);

  const canInheritApiKind = useMemo(() => {
    if (!selected) return true;
    return selected.overrides.llm_provider_kind == null || effectiveProviderKind === defaultProviderKind;
  }, [defaultProviderKind, effectiveProviderKind, selected]);

  const inheritedPromptLabel = useMemo(() => {
    if (!cfg) return 'No prompts loaded';
    return formatPromptLabel(cfg, cfg.defaults.prompt_id ?? null);
  }, [cfg]);

  const effectiveApiKind = useMemo(() => {
    if (!cfg || !selected) return 'responses_sse';
    return normalizeLlmApiKind(selected.overrides.llm_api_kind ?? cfg.defaults.llm_api_kind, effectiveProviderKind);
  }, [cfg, effectiveProviderKind, selected]);

  const effectiveBaseUrl = useMemo(() => {
    if (!cfg || !selected) return '';
    const overrideValue = selected.overrides.llm_base_url?.trim();
    return overrideValue && overrideValue.length > 0 ? overrideValue : cfg.defaults.llm_base_url;
  }, [cfg, selected]);

  const effectiveModel = useMemo(() => {
    if (!cfg || !selected) return '';
    const overrideValue = selected.overrides.llm_model?.trim();
    return overrideValue && overrideValue.length > 0 ? overrideValue : cfg.defaults.llm_model;
  }, [cfg, selected]);

  const effectivePreflightMode = useMemo(() => {
    if (!cfg || !selected) return 'off';
    return normalizeLlmPreflightMode(selected.overrides.llm_preflight_mode ?? cfg.defaults.llm_preflight_mode);
  }, [cfg, selected]);

  const effectiveReasoningEffort = useMemo(() => {
    if (!cfg || !selected) return '';
    return normalizeLlmReasoningEffort(selected.overrides.llm_reasoning_effort ?? cfg.defaults.llm_reasoning_effort);
  }, [cfg, selected]);

  const showOpenAiRecommendedProfileCallout = useMemo(() => {
    if (effectiveProviderKind !== 'openai_compatible') return false;
    if (effectiveBaseUrl !== 'https://api.openai.com/v1') return false;

    return (
      effectiveApiKind !== 'responses_sse' ||
      effectiveModel !== 'gpt-5.4' ||
      effectivePreflightMode !== 'off' ||
      effectiveReasoningEffort !== ''
    );
  }, [effectiveApiKind, effectiveBaseUrl, effectiveModel, effectivePreflightMode, effectiveProviderKind, effectiveReasoningEffort]);

  const showCc2ResponsesProfileRecommendation = useMemo(() => {
    return shouldRecommendResponsesForCc2OpenAiChatCompletions(
      effectiveProviderKind,
      effectiveApiKind,
      effectiveBaseUrl,
    );
  }, [effectiveApiKind, effectiveBaseUrl, effectiveProviderKind]);

  const effectiveVisualContextMode = useMemo(() => {
    if (!cfg || !selected) return 'off';
    return normalizeVisualContextMode(
      selected.overrides.context?.visual_context_mode ?? cfg.defaults.context.visual_context_mode,
    );
  }, [cfg, selected]);

  const effectiveVisualCaptureScope = useMemo(() => {
    if (!cfg || !selected) return 'display';
    return normalizeVisualCaptureScope(
      selected.overrides.context?.visual_capture_scope ?? cfg.defaults.context.visual_capture_scope,
    );
  }, [cfg, selected]);

  const effectiveSelectedTextContext = useMemo(() => {
    if (!cfg || !selected) return false;
    return selected.overrides.context?.use_selected_text ?? cfg.defaults.context.use_selected_text;
  }, [cfg, selected]);

  const effectiveWindowContext = useMemo(() => {
    if (!cfg || !selected) return false;
    return selected.overrides.context?.use_window_context ?? cfg.defaults.context.use_window_context;
  }, [cfg, selected]);

  const effectiveVisualDispatch = useMemo(() => {
    return resolveVisualContextDispatch(effectiveVisualContextMode, effectiveProviderKind, effectiveApiKind);
  }, [effectiveApiKind, effectiveProviderKind, effectiveVisualContextMode]);

  const platformContextWarnings = useMemo(() => {
    return contextCapabilityWarnings(platformCapabilities, {
      useSelectedText: effectiveSelectedTextContext,
      useWindowContext: effectiveWindowContext,
      visualMode: effectiveVisualContextMode,
      captureScope: effectiveVisualCaptureScope,
    });
  }, [
    effectiveSelectedTextContext,
    effectiveVisualCaptureScope,
    effectiveVisualContextMode,
    effectiveWindowContext,
    platformCapabilities,
  ]);

  const platformForegroundAppWarning = useMemo(() => {
    return foregroundAppCapabilityWarning(platformCapabilities);
  }, [platformCapabilities]);

  const effectiveScreenshotContextWarning = useMemo(() => {
    if (effectiveVisualContextMode !== 'screenshot' || effectiveVisualDispatch !== 'off') return null;
    if (llmSupportsAttachedImages(effectiveProviderKind, effectiveApiKind)) return null;
    return `${screenshotContextWarning(effectiveProviderKind, effectiveApiKind)} Use OpenAI Responses or Gemini native SSE, or switch this profile to Auto/OCR mode.`;
  }, [effectiveApiKind, effectiveProviderKind, effectiveVisualContextMode, effectiveVisualDispatch]);

  if (!profiles) {
    return (
      <div style={{ padding: 'var(--space-32)' }}>
        <div className="vw-type-title">Profiles</div>
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)' }}>
          Loading…
        </div>
      </div>
    );
  }

  if (!cfg) {
    return (
      <div style={{ padding: 'var(--space-32)' }}>
        <div className="vw-type-title">Profiles</div>
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
          {error ?? 'Failed to load config.'}
        </div>
      </div>
    );
  }

  return (
    <div
      style={{
        height: '100%',
        display: 'grid',
        gridTemplateColumns: '260px 1fr',
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
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
          <div className="vw-type-subtitle">Profiles</div>
          <button
            type="button"
            className="vw-button vw-button--ghost vw-iconButton"
            aria-label="Add profile"
            onClick={async () => {
              const profile = newProfile();
              const next = [...profiles, profile];
              setSelectedId(profile.id);
              await save(next);
            }}
          >
            +
          </button>
        </div>

        <div style={{ marginTop: 'var(--space-12)', display: 'grid', gap: 'var(--space-8)' }}>
          {profiles.map((profile) => {
            const isSelected = profile.id === selectedId;
            return (
              <button
                key={profile.id}
                type="button"
                onClick={() => setSelectedId(profile.id)}
                style={{
                  height: 72,
                  padding: 12,
                  borderRadius: 'var(--radius-card)',
                  border: '1px solid transparent',
                  background: isSelected ? 'rgba(255,255,255,0.18)' : 'transparent',
                  cursor: 'pointer',
                  display: 'grid',
                  gridTemplateColumns: 'auto 1fr',
                  gap: 'var(--space-12)',
                  textAlign: 'left',
                }}
              >
                <div
                  style={{
                    width: 32,
                    height: 32,
                    borderRadius: 8,
                    display: 'grid',
                    placeItems: 'center',
                    background: 'rgba(0,0,0,0.06)',
                  }}
                  aria-hidden="true"
                >
                  🪟
                </div>
                <div style={{ overflow: 'hidden' }}>
                  <div className="vw-type-bodyStrong" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {profile.name}
                  </div>
                  <div className="vw-type-caption" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {summarizeMatchers(profile.matchers)}
                  </div>
                  {!profile.enabled ? (
                    <div className="vw-type-caption" style={{ color: 'var(--color-danger-fg)' }}>
                      Disabled
                    </div>
                  ) : null}
                </div>
              </button>
            );
          })}
        </div>
      </div>

      <div style={{ padding: 'var(--space-32)', overflowY: 'auto' }}>
        <div className="vw-type-title">Profile</div>

        {error ? (
          <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            {error}
          </div>
        ) : null}

        {!selected ? (
          <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)' }}>
            Select a profile.
          </div>
        ) : (
          <div style={{ marginTop: 'var(--space-24)', display: 'grid', gap: 'var(--space-16)' }}>
            <SectionCard title="General" subtitle="Match one or more apps, then override only the settings that should differ from global defaults.">
              {platformForegroundAppWarning ? (
                <div className="vw-type-caption" style={{ color: 'var(--color-danger-fg)' }}>
                  {platformForegroundAppWarning}
                </div>
              ) : null}

              <FieldRow title="Name">
                <input
                  className="vw-input"
                  value={selected.name}
                  onChange={(e) => {
                    mutateSelected((profile) => ({ ...profile, name: e.target.value }));
                  }}
                  onBlur={() => {
                    void saveCurrentDraft();
                  }}
                />
              </FieldRow>

              <FieldRow title="Enabled" description="Disabled profiles stay stored but never match the foreground app.">
                <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <input
                    type="checkbox"
                    aria-label="Enable profile"
                    checked={selected.enabled}
                    onChange={(e) => {
                      void saveSelectedMutation((profile) => ({ ...profile, enabled: e.target.checked }));
                    }}
                  />
                  <span className="vw-type-caption">{selected.enabled ? 'On' : 'Off'}</span>
                </label>
              </FieldRow>

              <FieldRow
                title="Matchers"
                description="Profiles match when any matcher matches the foreground app. Use multiple rows to cover multiple applications or window-title variants."
              >
                <div style={{ display: 'grid', gap: 'var(--space-8)', width: '100%' }}>
                  {selected.matchers.map((matcher, index) => (
                    <div key={`${selected.id}-matcher-${index}`} className="vw-settingControls">
                      <select
                        className="vw-input"
                        aria-label={`Profile matcher type ${index + 1}`}
                        value={matcher.kind}
                        onChange={(e) => {
                          const nextKind = (e.target.value as MatcherKind) ?? 'ProcessNameEquals';
                          void saveSelectedMutation((profile) => ({
                            ...profile,
                            matchers: profile.matchers.map((entry, entryIndex) =>
                              entryIndex === index ? blankMatcher(nextKind, entry.value) : entry,
                            ),
                          }));
                        }}
                      >
                        {MATCHER_KIND_OPTIONS.map((option) => (
                          <option key={option.kind} value={option.kind}>
                            {option.label}
                          </option>
                        ))}
                      </select>

                      <input
                        className="vw-input"
                        aria-label={`Profile matcher value ${index + 1}`}
                        placeholder={matcherPlaceholder(matcher.kind)}
                        value={matcher.value}
                        onChange={(e) => {
                          const value = e.target.value;
                          mutateSelected((profile) => ({
                            ...profile,
                            matchers: profile.matchers.map((entry, entryIndex) =>
                              entryIndex === index ? { ...entry, value } : entry,
                            ),
                          }));
                        }}
                        onBlur={() => {
                          void saveCurrentDraft();
                        }}
                        style={{ width: 280 }}
                      />

                      <button
                        type="button"
                        className="vw-button vw-button--secondary"
                        aria-label={`Use foreground for matcher ${index + 1}`}
                        disabled={!platformCapabilities.foreground_app_identity}
                        title={
                          !platformCapabilities.foreground_app_identity
                            ? platformForegroundAppWarning ?? undefined
                            : undefined
                        }
                        onClick={async () => {
                          try {
                            const { invoke } = await import('@tauri-apps/api/core');
                            const info = await invoke<ForegroundAppInfo>('capture_foreground_app');
                            const value = foregroundValueForMatcher(info, matcher.kind);
                            if (!value) {
                              setError(`Foreground app does not expose a ${matcherLabel(matcher.kind).toLowerCase()} value right now.`);
                              return;
                            }

                            await saveSelectedMutation((profile) => ({
                              ...profile,
                              matchers: profile.matchers.map((entry, entryIndex) =>
                                entryIndex === index ? { ...entry, value } : entry,
                              ),
                            }));
                          } catch (e) {
                            setError(String(e));
                          }
                        }}
                      >
                        Use Foreground
                      </button>

                      <button
                        type="button"
                        className="vw-button vw-button--secondary"
                        aria-label={`Remove matcher ${index + 1}`}
                        disabled={selected.matchers.length <= 1}
                        onClick={() => {
                          void saveSelectedMutation((profile) => ({
                            ...profile,
                            matchers:
                              profile.matchers.length <= 1
                                ? [blankMatcher('ProcessNameEquals')]
                                : profile.matchers.filter((_, entryIndex) => entryIndex !== index),
                          }));
                        }}
                      >
                        Remove
                      </button>
                    </div>
                  ))}

                  <div className="vw-settingControls">
                    <button
                      type="button"
                      className="vw-button vw-button--secondary"
                      onClick={() => {
                        void saveSelectedMutation((profile) => ({
                          ...profile,
                          matchers: [...profile.matchers, blankMatcher('ProcessNameEquals')],
                        }));
                      }}
                    >
                      + Process
                    </button>

                    <button
                      type="button"
                      className="vw-button vw-button--secondary"
                      onClick={() => {
                        void saveSelectedMutation((profile) => ({
                          ...profile,
                          matchers: [...profile.matchers, blankMatcher('WindowTitleContains')],
                        }));
                      }}
                    >
                      + Window Title
                    </button>

                    <button
                      type="button"
                      className="vw-button vw-button--secondary"
                      onClick={() => {
                        void saveSelectedMutation((profile) => ({
                          ...profile,
                          matchers: [...profile.matchers, blankMatcher('ExePathEquals')],
                        }));
                      }}
                    >
                      + Executable
                    </button>

                  </div>
                </div>
              </FieldRow>
            </SectionCard>

            <SectionCard
              title="Enhancement Overrides"
              subtitle={`Inherited defaults: prompt ${inheritedPromptLabel}, provider ${formatProviderLabel(
                cfg.defaults.llm_provider_kind,
              )}, API ${formatApiModeLabel(normalizeLlmApiKind(cfg.defaults.llm_api_kind, defaultProviderKind))}.`}
            >
              {showOpenAiRecommendedProfileCallout ? (
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
                    This profile currently resolves to the legacy OpenAI stack. Recommended: <strong>Responses</strong> +{' '}
                    <strong>gpt-5.4</strong> + <strong>Preflight Off</strong>.
                  </span>
                  <button
                    type="button"
                    className="vw-button vw-button--secondary"
                    onClick={() => {
                      void saveSelectedMutation((profile) => ({
                        ...profile,
                        overrides: {
                          ...profile.overrides,
                          llm_api_kind: 'responses_sse',
                          llm_base_url: 'https://api.openai.com/v1',
                          llm_model: 'gpt-5.4',
                          llm_preflight_mode: 'off',
                          llm_reasoning_effort: null,
                        },
                      }));
                    }}
                  >
                    Apply Recommended Override
                  </button>
                </div>
              ) : null}

              {showCc2ResponsesProfileRecommendation ? (
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
                    On <strong>cc2.caaa.tech</strong>, this profile currently resolves to OpenAI-compatible Chat
                    {' '}Completions, which has been failing live validation. Responses + <strong>gpt-5.4</strong> is
                    {' '}the safer default on this gateway.
                  </span>
                  <button
                    type="button"
                    className="vw-button vw-button--secondary"
                    onClick={() => {
                      void saveSelectedMutation((profile) => ({
                        ...profile,
                        overrides: {
                          ...profile.overrides,
                          llm_provider_kind: 'openai_compatible',
                          llm_api_kind: 'responses_sse',
                          llm_base_url: effectiveBaseUrl,
                          llm_model: 'gpt-5.4',
                          llm_preflight_mode: 'off',
                          llm_reasoning_effort: null,
                        },
                      }));
                    }}
                  >
                    Use Responses Override
                  </button>
                </div>
              ) : null}

              <FieldRow title="Enhancement" description="Override whether this app should run LLM post-processing after transcription.">
                <select
                  className="vw-input"
                  aria-label="Profile enhancement override"
                  value={
                    selected.overrides.enable_enhancement == null
                      ? 'inherit'
                      : selected.overrides.enable_enhancement
                        ? 'on'
                        : 'off'
                  }
                  onChange={(e) => {
                    const value = e.target.value;
                    void saveSelectedMutation((profile) => ({
                      ...profile,
                      overrides: {
                        ...profile.overrides,
                        enable_enhancement: value === 'inherit' ? null : value === 'on',
                      },
                    }));
                  }}
                >
                  <option value="inherit">Inherit ({cfg.defaults.enable_enhancement ? 'On' : 'Off'})</option>
                  <option value="on">On</option>
                  <option value="off">Off</option>
                </select>
              </FieldRow>

              <FieldRow title="Prompt" description="Choose a different prompt for this app without changing the global default.">
                <select
                  className="vw-input"
                  aria-label="Profile prompt override"
                  value={selected.overrides.prompt_id ?? 'inherit'}
                  disabled={cfg.prompts.length === 0}
                  onChange={(e) => {
                    const value = e.target.value;
                    void saveSelectedMutation((profile) => ({
                      ...profile,
                      overrides: {
                        ...profile.overrides,
                        prompt_id: value === 'inherit' ? null : value,
                      },
                    }));
                  }}
                >
                  <option value="inherit">Inherit ({inheritedPromptLabel})</option>
                  {cfg.prompts.map((prompt) => (
                    <option key={prompt.id} value={prompt.id}>
                      {prompt.title}
                    </option>
                  ))}
                </select>
              </FieldRow>

              <FieldRow title="Provider" description="Pick the LLM provider family for this app. Changing provider resets model/base URL to sane defaults for that provider.">
                <select
                  className="vw-input"
                  aria-label="Profile provider override"
                  value={selected.overrides.llm_provider_kind ?? 'inherit'}
                  onChange={(e) => {
                    const value = e.target.value;
                    if (value === 'inherit') {
                      void saveSelectedMutation((profile) => ({
                        ...profile,
                        overrides: {
                          ...profile.overrides,
                          llm_provider_kind: null,
                          llm_api_kind: null,
                          llm_base_url: null,
                          llm_model: null,
                        },
                      }));
                      return;
                    }

                    const providerKind = value === 'gemini' ? 'gemini' : 'openai_compatible';
                    const nextApiKind =
                      providerKind === 'gemini'
                        ? 'stream_generate_content_sse'
                        : normalizeLlmApiKind(profileApiKindValue(selected, cfg), 'openai_compatible');

                    void saveSelectedMutation((profile) => ({
                      ...profile,
                      overrides: {
                        ...profile.overrides,
                        llm_provider_kind: providerKind,
                        llm_api_kind: nextApiKind,
                        llm_base_url: defaultBaseUrlForProvider(providerKind),
                        llm_model: defaultModelForProvider(providerKind),
                      },
                    }));
                  }}
                >
                  <option value="inherit">Inherit ({formatProviderLabel(cfg.defaults.llm_provider_kind)})</option>
                  <option value="openai_compatible">OpenAI-Compatible</option>
                  <option value="gemini">Google Gemini</option>
                </select>
              </FieldRow>

              <FieldRow title="API mode" description="Use inherit when the profile keeps the same provider family as defaults.">
                <select
                  className="vw-input"
                  aria-label="Profile API mode override"
                  value={selected.overrides.llm_api_kind ?? 'inherit'}
                  onChange={(e) => {
                    const value = e.target.value;
                    void saveSelectedMutation((profile) => ({
                      ...profile,
                      overrides: {
                        ...profile.overrides,
                        llm_api_kind: value === 'inherit' ? null : value,
                      },
                    }));
                  }}
                >
                  {canInheritApiKind ? (
                    <option value="inherit">
                      Inherit ({formatApiModeLabel(normalizeLlmApiKind(cfg.defaults.llm_api_kind, effectiveProviderKind))})
                    </option>
                  ) : null}
                  {effectiveProviderKind === 'gemini' ? (
                    <option value="stream_generate_content_sse">streamGenerateContent (HTTP SSE)</option>
                  ) : (
                    <>
                      <option value="chat_completions">Chat Completions (Legacy)</option>
                      <option value="responses_sse">OpenAI Responses (HTTP SSE)</option>
                    </>
                  )}
                </select>
              </FieldRow>

              <FieldRow title="Preflight" description="Best-effort connection warmup on recording start. Keep Off unless this path is measurably faster in your environment.">
                <select
                  className="vw-input"
                  aria-label="Profile preflight override"
                  value={selected.overrides.llm_preflight_mode ?? 'inherit'}
                  onChange={(e) => {
                    const value = e.target.value;
                    void saveSelectedMutation((profile) => ({
                      ...profile,
                      overrides: {
                        ...profile.overrides,
                        llm_preflight_mode: value === 'inherit' ? null : normalizeLlmPreflightMode(value),
                      },
                    }));
                  }}
                >
                  <option value="inherit">Inherit ({formatPreflightLabel(cfg.defaults.llm_preflight_mode)})</option>
                  <option value="off">Off</option>
                  <option value="http_connect">HTTP Connect</option>
                </select>
              </FieldRow>

              <FieldRow title="Reasoning effort" description="Optional. Profiles can enable a reasoning level, but the current config model still inherits when left unset.">
                <select
                  className="vw-input"
                  aria-label="Profile reasoning override"
                  value={selected.overrides.llm_reasoning_effort ?? 'inherit'}
                  onChange={(e) => {
                    const value = e.target.value;
                    void saveSelectedMutation((profile) => ({
                      ...profile,
                      overrides: {
                        ...profile.overrides,
                        llm_reasoning_effort:
                          value === 'inherit' ? null : normalizeLlmReasoningEffort(value),
                      },
                    }));
                  }}
                >
                  <option value="inherit">Inherit ({formatReasoningLabel(cfg.defaults.llm_reasoning_effort)})</option>
                  <option value="minimal">Minimal</option>
                  <option value="low">Low</option>
                  <option value="medium">Medium</option>
                  <option value="high">High</option>
                </select>
              </FieldRow>

              <FieldRow title="Base URL" description="Leave blank to inherit. Switching provider above resets this field to the provider's standard endpoint.">
                <div className="vw-settingControls">
                  <input
                    className="vw-input"
                    type="text"
                    value={selected.overrides.llm_base_url ?? ''}
                    placeholder={cfg.defaults.llm_base_url}
                    onChange={(e) => {
                      const value = e.target.value;
                      mutateSelected((profile) => ({
                        ...profile,
                        overrides: {
                          ...profile.overrides,
                          llm_base_url: value,
                        },
                      }));
                    }}
                    onBlur={() => {
                      void saveSelectedMutation((profile) => ({
                        ...profile,
                        overrides: {
                          ...profile.overrides,
                          llm_base_url:
                            profile.overrides.llm_base_url?.trim() === '' ? null : profile.overrides.llm_base_url ?? null,
                        },
                      }));
                    }}
                    style={{ width: 360 }}
                  />
                  <button
                    type="button"
                    className="vw-button vw-button--secondary"
                    onClick={() => {
                      void saveSelectedMutation((profile) => ({
                        ...profile,
                        overrides: {
                          ...profile.overrides,
                          llm_base_url: null,
                        },
                      }));
                    }}
                  >
                    Use Default
                  </button>
                </div>
              </FieldRow>

              <FieldRow title="Model" description="Leave blank to inherit. Use this to choose a different model for a specific app profile.">
                <div className="vw-settingControls">
                  <input
                    className="vw-input"
                    type="text"
                    value={selected.overrides.llm_model ?? ''}
                    placeholder={cfg.defaults.llm_model}
                    onChange={(e) => {
                      const value = e.target.value;
                      mutateSelected((profile) => ({
                        ...profile,
                        overrides: {
                          ...profile.overrides,
                          llm_model: value,
                        },
                      }));
                    }}
                    onBlur={() => {
                      void saveSelectedMutation((profile) => ({
                        ...profile,
                        overrides: {
                          ...profile.overrides,
                          llm_model: profile.overrides.llm_model?.trim() === '' ? null : profile.overrides.llm_model ?? null,
                        },
                      }));
                    }}
                    style={{ width: 260 }}
                  />
                  <button
                    type="button"
                    className="vw-button vw-button--secondary"
                    onClick={() => {
                      void saveSelectedMutation((profile) => ({
                        ...profile,
                        overrides: {
                          ...profile.overrides,
                          llm_model: null,
                        },
                      }));
                    }}
                  >
                    Use Default
                  </button>
                </div>
              </FieldRow>
            </SectionCard>

            <SectionCard
              title="Context Overrides"
              subtitle="These settings override the global context policy only for this profile, including visual mode and capture target."
            >
              {platformContextWarnings.map((warning) => (
                <div key={warning} className="vw-type-caption" style={{ color: 'var(--color-danger-fg)' }}>
                  {warning}
                </div>
              ))}

              {effectiveScreenshotContextWarning ? (
                <div className="vw-type-caption" style={{ color: 'var(--color-danger-fg)' }}>
                  {effectiveScreenshotContextWarning}
                </div>
              ) : null}

              <FieldRow
                title="Visual mode"
                description="Auto prefers direct screenshot input on multimodal APIs and falls back to OCR on text-only APIs."
              >
                <select
                  className="vw-input"
                  aria-label="Profile Visual mode"
                  value={visualContextOverrideChoice(selected.overrides.context)}
                  onChange={(e) => {
                    const value = e.target.value as VisualContextOverrideChoice;
                    void saveSelectedMutation((profile) => ({
                      ...profile,
                      overrides: {
                        ...profile.overrides,
                        context: setVisualContextOverrideChoice(profile.overrides.context, value),
                      },
                    }));
                  }}
                >
                  <option value="inherit">
                    Inherit ({formatVisualContextModeLabel(cfg.defaults.context.visual_context_mode)})
                  </option>
                  <option value="off">Off</option>
                  <option value="auto">Auto</option>
                  <option value="screenshot">Screenshot Only</option>
                  <option value="ocr">OCR Only</option>
                </select>
              </FieldRow>

              <FieldRow
                title="Capture target"
                description="Display preserves the old behavior. Foreground window is more private and more relevant when the platform supports it."
              >
                <select
                  className="vw-input"
                  aria-label="Profile Visual capture target"
                  value={visualCaptureScopeOverrideChoice(selected.overrides.context)}
                  onChange={(e) => {
                    const value = e.target.value as VisualCaptureScopeOverrideChoice;
                    void saveSelectedMutation((profile) => ({
                      ...profile,
                      overrides: {
                        ...profile.overrides,
                        context: setVisualCaptureScopeOverrideChoice(profile.overrides.context, value),
                      },
                    }));
                  }}
                >
                  <option value="inherit">
                    Inherit ({formatVisualCaptureScopeLabel(cfg.defaults.context.visual_capture_scope)})
                  </option>
                  <option value="display">Display</option>
                  <option value="foreground_window">Foreground Window</option>
                </select>
              </FieldRow>

              <div className="vw-type-caption" style={{ color: 'var(--text-secondary)' }}>
                Effective visual path: {effectiveVisualDispatch} via {formatVisualCaptureScopeLabel(effectiveVisualCaptureScope)}.
              </div>

              {CONTEXT_OVERRIDE_FIELDS.map((field) => (
                <FieldRow key={field.key} title={field.title} description={field.description}>
                  <select
                    className="vw-input"
                    aria-label={`Profile ${field.title}`}
                    value={contextOverrideChoice(selected.overrides.context, field.key)}
                    onChange={(e) => {
                      const value = e.target.value as ContextOverrideChoice;
                      void saveSelectedMutation((profile) => ({
                        ...profile,
                        overrides: {
                          ...profile.overrides,
                          context: setContextOverrideChoice(profile.overrides.context, field.key, value),
                        },
                      }));
                    }}
                  >
                    <option value="inherit">
                      Inherit ({cfg.defaults.context[field.key] ? 'On' : 'Off'})
                    </option>
                    <option value="on">On</option>
                    <option value="off">Off</option>
                  </select>
                </FieldRow>
              ))}
            </SectionCard>

            <div style={{ display: 'flex', gap: 'var(--space-12)' }}>
              <button
                type="button"
                className="vw-button vw-button--secondary"
                onClick={async () => {
                  const duplicate: PowerModeProfile = {
                    ...selected,
                    id: crypto.randomUUID(),
                    name: `${selected.name} Copy`,
                  };
                  const next = [...profiles, duplicate];
                  setProfiles(next);
                  setSelectedId(duplicate.id);
                  await save(next);
                }}
              >
                Duplicate
              </button>

              <button
                type="button"
                className="vw-button vw-button--secondary"
                onClick={async () => {
                  const next = profiles.filter((profile) => profile.id !== selected.id);
                  setProfiles(next);
                  setSelectedId(next[0]?.id ?? null);
                  await save(next);
                }}
              >
                Delete
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function profileApiKindValue(profile: PowerModeProfile, cfg: AppConfig): string {
  return profile.overrides.llm_api_kind ?? cfg.defaults.llm_api_kind;
}
