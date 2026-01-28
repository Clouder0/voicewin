import { useCallback, useEffect, useMemo, useState } from 'react';

import type { AppConfig, ProviderStatus } from '../lib/types';

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
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const [dirty, setDirty] = useState(false);
  const [draft, setDraft] = useState({
    enable_enhancement: false,
    llm_base_url: '',
    llm_model: '',
  });

  const [apiKeyDraft, setApiKeyDraft] = useState('');

  const refresh = useCallback(async () => {
    try {
      const { isTauri, invoke } = await import('@tauri-apps/api/core');
      if (!isTauri()) return;

      const nextCfg = await invoke<AppConfig>('get_config');
      const nextProviders = await invoke<ProviderStatus>('get_provider_status');

      setCfg(nextCfg);
      setProviders(nextProviders);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const saveConfig = useCallback(
    async (nextCfg: AppConfig) => {
      setSaving(true);
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('set_config', { cfg: nextCfg });
        setCfg(nextCfg);
        setError(null);
        // Refresh so we pick up key-present and any backend normalization.
        await refresh();
      } catch (e) {
        setError(String(e));
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
    setDraft({
      enable_enhancement: Boolean(cfg.defaults.enable_enhancement),
      llm_base_url: cfg.defaults.llm_base_url ?? '',
      llm_model: cfg.defaults.llm_model ?? '',
    });
  }, [cfg, dirty]);

  const openaiKeyStatus = useMemo(() => {
    if (!providers) return 'Unknown';
    return providers.openai_api_key_present ? 'Set' : 'Not set';
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
      <div style={{ maxWidth: 720, margin: '0 auto', paddingTop: 64, paddingInline: 'var(--space-24)' }}>
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
    <div style={{ maxWidth: 720, margin: '0 auto', paddingTop: 64, paddingInline: 'var(--space-24)' }}>
      <div className="vw-type-title">Settings</div>

      {error ? (
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
          {error}
        </div>
      ) : null}

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
                value={apiKeyDraft}
                onChange={(e) => setApiKeyDraft(e.target.value)}
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
                    const { invoke } = await import('@tauri-apps/api/core');
                    const next = await invoke<ProviderStatus>('set_openai_api_key', { apiKey: apiKeyDraft });
                    setProviders(next);
                    setApiKeyDraft('');
                    await refresh();
                  } catch (e) {
                    setError(String(e));
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
                    const { invoke } = await import('@tauri-apps/api/core');
                    const next = await invoke<ProviderStatus>('clear_openai_api_key');
                    setProviders(next);
                    setApiKeyDraft('');
                    await refresh();
                  } catch (e) {
                    setError(String(e));
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
        <div style={{ display: 'flex', gap: 'var(--space-12)', marginTop: 'var(--space-16)' }}>
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
                },
              };
              void saveConfig(nextCfg).then(() => setDirty(false));
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
