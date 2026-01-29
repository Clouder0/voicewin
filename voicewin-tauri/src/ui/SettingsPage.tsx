import { useCallback, useEffect, useMemo, useState } from 'react';

import type { AppConfig, ProviderStatus } from '../lib/types';

type ModelStatus = {
  bootstrap_ok: boolean;
  bootstrap_path: string;
  preferred_ok: boolean;
  preferred_path: string;
};

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
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        gap: 'var(--space-16)',
        padding: 'var(--space-12)',
      }}
    >
      <div style={{ minWidth: 0 }}>
        <div className="vw-type-bodyStrong">{title}</div>
        {description ? (
          <div className="vw-type-caption" style={{ marginTop: 4 }}>
            {description}
          </div>
        ) : null}
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-8)' }}>{right}</div>
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
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // Inline key feedback so failures aren't "offscreen" at the page header.
  const [elevenKeyNotice, setElevenKeyNotice] = useState<string | null>(null);
  const [elevenKeyError, setElevenKeyError] = useState<string | null>(null);
  const [openaiKeyNotice, setOpenaiKeyNotice] = useState<string | null>(null);
  const [openaiKeyError, setOpenaiKeyError] = useState<string | null>(null);

  const [dirty, setDirty] = useState(false);
  const [draft, setDraft] = useState({
    enable_enhancement: false,
    llm_base_url: '',
    llm_model: '',

    stt_provider: 'local',
    local_stt_model_path: '',
    elevenlabs_stt_model: 'scribe_v2',
  });

  const [openaiApiKeyDraft, setOpenaiApiKeyDraft] = useState('');
  const [elevenApiKeyDraft, setElevenApiKeyDraft] = useState('');

  const refresh = useCallback(async () => {
    try {
      const { isTauri, invoke } = await import('@tauri-apps/api/core');
      if (!isTauri()) return;

      const nextCfg = await invoke<AppConfig>('get_config');
      const nextProviders = await invoke<ProviderStatus>('get_provider_status');
      const nextModelStatus = await invoke<ModelStatus>('get_model_status');

      setCfg(nextCfg);
      setProviders(nextProviders);
      setModelStatus(nextModelStatus);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const saveConfig = useCallback(
    async (nextCfg: AppConfig): Promise<boolean> => {
      setSaving(true);
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
        setSaving(false);
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

    const localDefault = modelStatus?.preferred_ok
      ? modelStatus.preferred_path
      : modelStatus?.bootstrap_path ?? '';

    const isLocal = cfg.defaults.stt_provider === 'local';
    const currentEleven = cfg.defaults.stt_provider === 'elevenlabs' ? cfg.defaults.stt_model : 'scribe_v2';
    const normalizedEleven = currentEleven === 'scribe_v2_realtime' ? 'scribe_v2_realtime' : 'scribe_v2';

    setDraft({
      enable_enhancement: Boolean(cfg.defaults.enable_enhancement),
      llm_base_url: cfg.defaults.llm_base_url ?? '',
      llm_model: cfg.defaults.llm_model ?? '',

      stt_provider: (cfg.defaults.stt_provider === 'elevenlabs' ? 'elevenlabs' : 'local') as 'local' | 'elevenlabs',
      local_stt_model_path: isLocal ? cfg.defaults.stt_model : localDefault,
      elevenlabs_stt_model: normalizedEleven as 'scribe_v2' | 'scribe_v2_realtime',
    });
  }, [cfg, dirty, modelStatus]);

  const openaiKeyStatus = useMemo(() => {
    if (!providers) return 'Unknown';
    if (providers.openai_api_key_error) return 'Unavailable';
    return providers.openai_api_key_present ? 'Set' : 'Not set';
  }, [providers]);

  const openaiKeyStatusError = useMemo(() => {
    return providers?.openai_api_key_error ?? null;
  }, [providers]);

  const elevenKeyStatus = useMemo(() => {
    if (!providers) return 'Unknown';
    if (providers.elevenlabs_api_key_error) return 'Unavailable';
    return providers.elevenlabs_api_key_present ? 'Set' : 'Not set';
  }, [providers]);

  const elevenKeyStatusError = useMemo(() => {
    return providers?.elevenlabs_api_key_error ?? null;
  }, [providers]);

  const baseUrlLooksMissingV1 = useMemo(() => {
    const u = draft.llm_base_url.trim();
    if (!u) return false;
    // Heuristic: OpenAI-compatible endpoints usually require the /v1 prefix.
    // (We keep it as a warning, not a hard validation.)
    return !/\/v1\/?$/.test(u);
  }, [draft.llm_base_url]);

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
            right={<span className="vw-type-caption">Configured</span>}
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
        subtitle="Required only when ElevenLabs STT is selected. The key is stored in the OS keyring (not in config.json)."
      >
        <SettingRow
          title="API key"
          description={`Status: ${elevenKeyStatus}.`}
          right={
            <>
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
                disabled={saving}
                onClick={async () => {
                  try {
                    setSaving(true);
                    setElevenKeyError(null);
                    setElevenKeyNotice(null);
                    const { invoke } = await import('@tauri-apps/api/core');
                    const next = await invoke<ProviderStatus>('set_elevenlabs_api_key', { api_key: elevenApiKeyDraft });
                    setProviders(next);
                    setElevenApiKeyDraft('');

                    setElevenKeyNotice('Saved');
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
            </>
          }
        />

        {elevenKeyStatusError ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            Keyring error: {elevenKeyStatusError}
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
            ElevenLabs is selected but the OS keyring is unavailable. Recording will fail until this is resolved.
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
          description="When enabled, VoiceWin will call your OpenAI-compatible endpoint after transcription."
          right={
            <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                type="checkbox"
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
      </Section>

      <Section
        title="OpenAI-Compatible"
        subtitle="Configure the endpoint used for enhancement (base URL + model) and store your API key in the OS keyring."
      >
        <SettingRow
          title="API key"
          description={`Status: ${openaiKeyStatus}. The key is stored in the OS keyring (not in config.json).`}
          right={
            <>
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
                disabled={saving}
                onClick={async () => {
                  try {
                    setSaving(true);
                    setOpenaiKeyError(null);
                    setOpenaiKeyNotice(null);
                    const { invoke } = await import('@tauri-apps/api/core');
                    const next = await invoke<ProviderStatus>('set_openai_api_key', { api_key: openaiApiKeyDraft });
                    setProviders(next);
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
            </>
          }
        />

        {openaiKeyStatusError ? (
          <div className="vw-type-caption" style={{ padding: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
            Keyring error: {openaiKeyStatusError}
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

        <SettingRow
          title="Base URL"
          description="Example: https://api.openai.com/v1 or http://localhost:11434/v1"
          right={
            <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-8)' }}>
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
                  setDraft((d) => ({ ...d, llm_base_url: 'https://api.openai.com/v1' }));
                }}
              >
                Reset
              </button>
            </div>
          }
        />

        <SettingRow
          title="Model"
          description="Example: gpt-4o-mini"
          right={
            <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-8)' }}>
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
                  setDraft((d) => ({ ...d, llm_model: 'gpt-4o-mini' }));
                }}
              >
                Reset
              </button>
            </div>
          }
        />
      </Section>

      {baseUrlLooksMissingV1 ? (
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
          Warning: your Base URL does not end with <code>/v1</code>. Many OpenAI-compatible servers require it.
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
              setDraft({
                enable_enhancement: Boolean(cfg.defaults.enable_enhancement),
                llm_base_url: cfg.defaults.llm_base_url ?? '',
                llm_model: cfg.defaults.llm_model ?? '',

                stt_provider: (cfg.defaults.stt_provider === 'elevenlabs' ? 'elevenlabs' : 'local') as
                  | 'local'
                  | 'elevenlabs',
                local_stt_model_path:
                  cfg.defaults.stt_provider === 'local'
                    ? cfg.defaults.stt_model
                    : (modelStatus?.preferred_ok
                        ? modelStatus.preferred_path
                        : modelStatus?.bootstrap_path ?? ''),
                elevenlabs_stt_model:
                  cfg.defaults.stt_provider === 'elevenlabs' && cfg.defaults.stt_model === 'scribe_v2_realtime'
                    ? 'scribe_v2_realtime'
                    : 'scribe_v2',
              });
            }}
          >
            Cancel
          </button>

          <button
            type="button"
            className="vw-button vw-button--primary"
            disabled={saving}
            onClick={() => {
              const nextCfg: AppConfig = {
                ...cfg,
                defaults: {
                  ...cfg.defaults,
                  enable_enhancement: Boolean(draft.enable_enhancement),
                  llm_base_url: draft.llm_base_url.trim() || cfg.defaults.llm_base_url,
                  llm_model: draft.llm_model.trim() || cfg.defaults.llm_model,

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
