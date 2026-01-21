import { useEffect, useMemo, useState } from 'react';
import type { Invoker } from '../lib/invoker';

type Props = {
  invoker: Invoker;
};

type Tab = 'providers' | 'defaults' | 'prompts' | 'power' | 'history';

export function Settings({ invoker }: Props) {
  const [tab, setTab] = useState<Tab>('providers');

  useEffect(() => {
    let unlisten: null | (() => void) = null;

    async function start() {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        unlisten = await listen<string>('voicewin://navigate', (e) => {
          const dest = e.payload;
          if (dest === 'providers' || dest === 'defaults' || dest === 'prompts' || dest === 'power' || dest === 'history') {
            setTab(dest);
          }
        });
      } catch {
        // Not running inside Tauri.
      }
    }

    void start();

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const tabTitle = useMemo(() => {
    switch (tab) {
      case 'providers':
        return 'Providers';
      case 'defaults':
        return 'Defaults';
      case 'prompts':
        return 'Prompts';
      case 'power':
        return 'Power Modes';
      case 'history':
        return 'History';
    }
  }, [tab]);

  return (
    <div className="card">
      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div>
          <div style={{ fontSize: 13, letterSpacing: 0.4 }}>Settings</div>
          <div className="small">Configure providers, prompts, and per-app power modes.</div>
        </div>
        <div className="badge">{tabTitle}</div>
      </div>

      <div className="hr" />

      <div className="row" style={{ gap: 8, flexWrap: 'wrap' }}>
        <button type="button" className="button" onClick={() => setTab('providers')}>
          Providers
        </button>
        <button type="button" className="button" onClick={() => setTab('defaults')}>
          Defaults
        </button>
        <button type="button" className="button" onClick={() => setTab('prompts')}>
          Prompts
        </button>
        <button type="button" className="button" onClick={() => setTab('power')}>
          Power Modes
        </button>
        <button type="button" className="button" onClick={() => setTab('history')}>
          History
        </button>
      </div>

      <div className="hr" />

      {tab === 'providers' ? <Providers invoker={invoker} /> : null}
      {tab === 'defaults' ? <Defaults invoker={invoker} /> : null}
      {tab === 'prompts' ? <Prompts invoker={invoker} /> : null}
      {tab === 'power' ? <PowerModes invoker={invoker} /> : null}
      {tab === 'history' ? <History invoker={invoker} /> : null}
    </div>
  );
}

function Providers({ invoker }: Props) {
  const [status, setStatus] = useState<null | { openai: boolean; eleven: boolean }>(null);
  const [openaiKey, setOpenaiKey] = useState('');
  const [elevenKey, setElevenKey] = useState('');
  const [message, setMessage] = useState<string | null>(null);

  async function refresh() {
    const s = await invoker.getProviderStatus();
    setStatus({
      openai: s.openai_api_key_present,
      eleven: s.elevenlabs_api_key_present,
    });
  }

  return (
    <div className="kv">
      <b>STT</b>
      <div className="small">Local Whisper / ElevenLabs (MVP)</div>

      <b>LLM</b>
      <div className="small">OpenAI-compatible only (MVP)</div>

      <b>Key Status</b>
      <div className="small">
        OpenAI key: {status ? (status.openai ? 'set' : 'missing') : 'unknown'} · ElevenLabs key:{' '}
        {status ? (status.eleven ? 'set' : 'missing') : 'unknown'}
      </div>

      <b>OpenAI API Key</b>
      <div className="row">
        <input
          className="input"
          value={openaiKey}
          onChange={(e) => setOpenaiKey(e.target.value)}
          placeholder="sk-..."
          aria-label="openai-api-key"
        />
        <button
          type="button"
          className="button"
          onClick={async () => {
            await invoker.setOpenAiApiKey(openaiKey);
            setOpenaiKey('');
            await refresh();
            setMessage('Saved OpenAI key to keyring');
          }}
        >
          Save
        </button>
      </div>

      <b>ElevenLabs API Key</b>
      <div className="row">
        <input
          className="input"
          value={elevenKey}
          onChange={(e) => setElevenKey(e.target.value)}
          placeholder="..."
          aria-label="elevenlabs-api-key"
        />
        <button
          type="button"
          className="button"
          onClick={async () => {
            await invoker.setElevenLabsApiKey(elevenKey);
            setElevenKey('');
            await refresh();
            setMessage('Saved ElevenLabs key to keyring');
          }}
        >
          Save
        </button>
      </div>

      <b>Actions</b>
      <div className="row">
        <button type="button" className="button" onClick={refresh}>
          Refresh
        </button>
        <span className="small">{message ?? ''}</span>
      </div>
    </div>
  );
}

function Defaults({ invoker }: Props) {
  const [cfg, setCfg] = useState<null | import('../lib/types').AppConfig>(null);
  const [error, setError] = useState<string | null>(null);

  const [modelStatus, setModelStatus] = useState<null | {
    bootstrap_ok: boolean;
    bootstrap_path: string;
    preferred_ok: boolean;
    preferred_path: string;
  }>(null);
  const [preferredSrcPath, setPreferredSrcPath] = useState('');
  const [modelMessage, setModelMessage] = useState<string | null>(null);

  async function load() {
    try {
      setError(null);
      const c = await invoker.getConfig();
      setCfg(c);
    } catch (e) {
      setError(String(e));
    }
  }

  async function save(next: import('../lib/types').AppConfig) {
    await invoker.setConfig(next);
    setCfg(next);
  }

  async function refreshModelStatus() {
    try {
      setModelMessage(null);
      const s = await invoker.getModelStatus();
      setModelStatus(s);
    } catch (e) {
      setModelMessage(String(e));
    }
  }

  async function installPreferred() {
    setModelMessage(null);
    try {
      const dst = await invoker.installPreferredModel({ src_path: preferredSrcPath });
      setModelMessage(`Installed preferred model to: ${dst}`);
      setPreferredSrcPath('');
      await refreshModelStatus();

      // Best-effort: reload config to reflect auto-switch.
      await load();
    } catch (e) {
      setModelMessage(String(e));
    }
  }

  if (!cfg) {
    return (
      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div className="small">Load defaults from config.</div>
        <button type="button" className="button" onClick={load}>
          Load
        </button>
        {error ? <div className="small">Error: {error}</div> : null}
      </div>
    );
  }

  const d = cfg.defaults;

  return (
    <div>
      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div className="small">Global Defaults</div>
        <button
          type="button"
          className="button"
          onClick={async () => {
            await save(cfg);
          }}
        >
          Save
        </button>
      </div>

      <div className="hr" />

      <div className="kv">
        <b>STT Provider</b>
        <input
          className="input"
          value={d.stt_provider}
          onChange={(e) => {
            const next = {
              ...cfg,
              defaults: { ...d, stt_provider: e.target.value },
            };
            setCfg(next);
          }}
          aria-label="stt-provider"
          placeholder="local or elevenlabs"
        />

        <b>STT Model</b>
        <input
          className="input"
          value={d.stt_model}
          onChange={(e) => {
            const next = {
              ...cfg,
              defaults: { ...d, stt_model: e.target.value },
            };
            setCfg(next);
          }}
          aria-label="stt-model"
          placeholder="For local: filesystem path to GGUF" 
        />

        <b>Local Model</b>
        <div className="card" style={{ padding: 12, gridColumn: '1 / -1' }}>
          <div className="row" style={{ justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
            <div className="small">
              Bootstrap: {modelStatus ? (modelStatus.bootstrap_ok ? 'OK' : 'missing/corrupt') : 'unknown'} · Preferred:{' '}
              {modelStatus ? (modelStatus.preferred_ok ? 'installed' : 'not installed') : 'unknown'}
            </div>
            <div className="row" style={{ gap: 8 }}>
              <button type="button" className="button" onClick={refreshModelStatus}>
                Refresh
              </button>
            </div>
          </div>

          <div className="hr" />

          <div className="kv">
            <b>Bootstrap Path</b>
            <div className="small">{modelStatus ? modelStatus.bootstrap_path : '—'}</div>

            <b>Preferred Path</b>
            <div className="small">{modelStatus ? modelStatus.preferred_path : '—'}</div>

            <b>Install Preferred GGUF</b>
            <div className="row" style={{ gap: 8, flexWrap: 'wrap' }}>
              <input
                className="input"
                value={preferredSrcPath}
                onChange={(e) => setPreferredSrcPath(e.target.value)}
                placeholder="Select whisper-large-v3-turbo-q5_k.gguf"
                aria-label="preferred-model-src"
              />
              <button
                type="button"
                className="button"
                onClick={async () => {
                  const p = await invoker.pickPreferredModelFile();
                  if (p) setPreferredSrcPath(p);
                }}
              >
                Browse…
              </button>
              <button type="button" className="button" onClick={installPreferred} disabled={!preferredSrcPath}>
                Install
              </button>
            </div>

            {modelMessage ? <div className="small">{modelMessage}</div> : null}
            <div className="small">
              Tip: this copies the file into the app models directory and automatically switches Defaults → STT Model.
            </div>
          </div>
        </div>

        <b>Language</b>
        <input
          className="input"
          value={d.language}
          onChange={(e) => {
            const next = {
              ...cfg,
              defaults: { ...d, language: e.target.value },
            };
            setCfg(next);
          }}
          aria-label="language"
          placeholder="en or auto"
        />

        <b>LLM Base URL</b>
        <input
          className="input"
          value={d.llm_base_url}
          onChange={(e) => {
            const next = {
              ...cfg,
              defaults: { ...d, llm_base_url: e.target.value },
            };
            setCfg(next);
          }}
          aria-label="llm-base-url"
          placeholder="https://api.openai.com/v1"
        />

        <b>LLM Model</b>
        <input
          className="input"
          value={d.llm_model}
          onChange={(e) => {
            const next = {
              ...cfg,
              defaults: { ...d, llm_model: e.target.value },
            };
            setCfg(next);
          }}
          aria-label="llm-model"
          placeholder="gpt-4o-mini"
        />

        <b>Enable Enhancement</b>
        <label className="row" style={{ gap: 8 }}>
          <input
            type="checkbox"
            checked={d.enable_enhancement}
            onChange={(e) => {
              const next = {
                ...cfg,
                defaults: { ...d, enable_enhancement: e.target.checked },
              };
              setCfg(next);
            }}
            aria-label="enable-enhancement"
          />
          <span className="small">Use LLM post-processing by default</span>
        </label>

        <b>Insert Mode</b>
        <input
          className="input"
          value={d.insert_mode}
          onChange={(e) => {
            const mode = e.target.value === 'PasteAndEnter' ? 'PasteAndEnter' : 'Paste';
            const next = {
              ...cfg,
              defaults: { ...d, insert_mode: mode },
            };
            setCfg(next);
          }}
          aria-label="insert-mode"
          placeholder="Paste or PasteAndEnter"
        />

        <b>History</b>
        <label className="row" style={{ gap: 8 }}>
          <input
            type="checkbox"
            checked={d.history_enabled}
            onChange={(e) => {
              setCfg({
                ...cfg,
                defaults: { ...d, history_enabled: e.target.checked },
              });
            }}
            aria-label="history-enabled"
          />
          <span className="small">Save transcript history to disk</span>
        </label>

        <b>Context</b>
        <div className="row" style={{ gap: 12, flexWrap: 'wrap' }}>
          <label className="row" style={{ gap: 6 }}>
            <input
              type="checkbox"
              checked={d.context.use_clipboard}
              onChange={(e) => {
                setCfg({
                  ...cfg,
                  defaults: {
                    ...d,
                    context: { ...d.context, use_clipboard: e.target.checked },
                  },
                });
              }}
              aria-label="use-clipboard"
            />
            <span className="small">Clipboard</span>
          </label>
          <label className="row" style={{ gap: 6 }}>
            <input
              type="checkbox"
              checked={d.context.use_selected_text}
              onChange={(e) => {
                setCfg({
                  ...cfg,
                  defaults: {
                    ...d,
                    context: { ...d.context, use_selected_text: e.target.checked },
                  },
                });
              }}
              aria-label="use-selected-text"
            />
            <span className="small">Selected text</span>
          </label>
          <label className="row" style={{ gap: 6 }}>
            <input
              type="checkbox"
              checked={d.context.use_window_context}
              onChange={(e) => {
                setCfg({
                  ...cfg,
                  defaults: {
                    ...d,
                    context: { ...d.context, use_window_context: e.target.checked },
                  },
                });
              }}
              aria-label="use-window-context"
            />
            <span className="small">Window</span>
          </label>
          <label className="row" style={{ gap: 6 }}>
            <input
              type="checkbox"
              checked={d.context.use_custom_vocabulary}
              onChange={(e) => {
                setCfg({
                  ...cfg,
                  defaults: {
                    ...d,
                    context: { ...d.context, use_custom_vocabulary: e.target.checked },
                  },
                });
              }}
              aria-label="use-custom-vocabulary"
            />
            <span className="small">Vocabulary</span>
          </label>
        </div>

        <div className="small">
          Note: local Whisper expects 16kHz mono audio; STT model is a local file path.
        </div>
      </div>
    </div>
  );
}

function Prompts({ invoker }: Props) {
  const [cfg, setCfg] = useState<null | import('../lib/types').AppConfig>(null);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    try {
      setError(null);
      const c = await invoker.getConfig();
      setCfg(c);
    } catch (e) {
      setError(String(e));
    }
  }

  async function save(next: import('../lib/types').AppConfig) {
    await invoker.setConfig(next);
    setCfg(next);
  }

  if (!cfg) {
    return (
      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div className="small">Load prompts from config.</div>
        <button type="button" className="button" onClick={load}>
          Load
        </button>
        {error ? <div className="small">Error: {error}</div> : null}
      </div>
    );
  }

  return (
    <div>
      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div className="small">Prompts ({cfg.prompts.length})</div>
        <button
          type="button"
          className="button"
          onClick={async () => {
            const next = {
              ...cfg,
              prompts: [
                ...cfg.prompts,
                {
                  id: crypto.randomUUID(),
                  title: 'New Prompt',
                  mode: 'Enhancer',
                  prompt_text: 'Fix grammar.',
                  trigger_words: [],
                },
              ],
            };
            await save(next);
          }}
        >
          Add
        </button>
      </div>

      <div className="hr" />

      {cfg.prompts.length === 0 ? (
        <div className="small">No prompts yet.</div>
      ) : (
        <div className="kv">
          {cfg.prompts.map((p, idx) => (
            <div key={p.id} style={{ gridColumn: '1 / -1' }}>
              <div className="card" style={{ padding: 12 }}>
                <div className="row" style={{ justifyContent: 'space-between' }}>
                  <div className="badge">#{idx + 1}</div>
                  <button
                    type="button"
                    className="button"
                    onClick={async () => {
                      const next = {
                        ...cfg,
                        prompts: cfg.prompts.filter((x) => x.id !== p.id),
                      };
                      await save(next);
                    }}
                  >
                    Delete
                  </button>
                </div>

                <div className="hr" />

                <div className="kv">
                  <b>Title</b>
                  <input
                    className="input"
                    value={p.title}
                    onChange={(e) => {
                      const next = {
                        ...cfg,
                        prompts: cfg.prompts.map((x) =>
                          x.id === p.id ? { ...x, title: e.target.value } : x,
                        ),
                      };
                      setCfg(next);
                    }}
                  />

                  <b>Mode</b>
                  <input
                    className="input"
                    value={p.mode}
                    onChange={(e) => {
                      const mode = e.target.value === 'Assistant' ? 'Assistant' : 'Enhancer';
                      const next = {
                        ...cfg,
                        prompts: cfg.prompts.map((x) => (x.id === p.id ? { ...x, mode } : x)),
                      };
                      setCfg(next);
                    }}
                  />

                  <b>Trigger Words</b>
                  <input
                    className="input"
                    value={p.trigger_words.join(', ')}
                    onChange={(e) => {
                      const words = e.target.value
                        .split(',')
                        .map((w) => w.trim())
                        .filter(Boolean);
                      const next = {
                        ...cfg,
                        prompts: cfg.prompts.map((x) =>
                          x.id === p.id ? { ...x, trigger_words: words } : x,
                        ),
                      };
                      setCfg(next);
                    }}
                  />

                  <b>Prompt Text</b>
                  <textarea
                    className="input"
                    value={p.prompt_text}
                    onChange={(e) => {
                      const next = {
                        ...cfg,
                        prompts: cfg.prompts.map((x) =>
                          x.id === p.id ? { ...x, prompt_text: e.target.value } : x,
                        ),
                      };
                      setCfg(next);
                    }}
                    style={{ minHeight: 120, fontFamily: 'var(--mono)' }}
                  />
                </div>

                <div className="hr" />

                <div className="row" style={{ justifyContent: 'flex-end' }}>
                  <button
                    type="button"
                    className="button"
                    onClick={async () => {
                      await save(cfg);
                    }}
                  >
                    Save Changes
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function PowerModes({ invoker }: Props) {
  const [cfg, setCfg] = useState<null | import('../lib/types').AppConfig>(null);
  const [editableProfiles, setEditableProfiles] = useState<null | import('../lib/types').PowerModeProfile[]>(null);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    try {
      setError(null);
      const c = await invoker.getConfig();
      const { decodePowerModeProfile } = await import('../lib/types');
      setCfg(c);
      setEditableProfiles(c.profiles.map(decodePowerModeProfile));
    } catch (e) {
      setError(String(e));
    }
  }

  async function saveProfiles(profiles: import('../lib/types').PowerModeProfile[]) {
    if (!cfg) return;
    const { encodePowerModeProfile } = await import('../lib/types');
    const next = {
      ...cfg,
      profiles: profiles.map(encodePowerModeProfile),
    };
    await invoker.setConfig(next);
    setCfg(next);
    setEditableProfiles(profiles);
  }

  if (!cfg || !editableProfiles) {
    return (
      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div className="small">Load power modes from config.</div>
        <button type="button" className="button" onClick={load}>
          Load
        </button>
        {error ? <div className="small">Error: {error}</div> : null}
      </div>
    );
  }

  return (
    <div>
      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div className="small">Profiles ({editableProfiles.length})</div>
        <button
          type="button"
          className="button"
          onClick={async () => {
            const next = [
              ...editableProfiles,
              {
                id: crypto.randomUUID(),
                name: 'New Profile',
                enabled: true,
                matchers: [{ kind: 'ProcessNameEquals', value: 'slack.exe' }],
                overrides: {},
              },
            ];
            await saveProfiles(next);
          }}
        >
          Add
        </button>
      </div>

      <div className="hr" />

      {editableProfiles.length === 0 ? (
        <div className="small">No profiles yet.</div>
      ) : (
        <div className="kv">
          {editableProfiles.map((p, idx) => (
            <div key={p.id} style={{ gridColumn: '1 / -1' }}>
              <div className="card" style={{ padding: 12 }}>
                <div className="row" style={{ justifyContent: 'space-between' }}>
                  <div className="badge">#{idx + 1}</div>
                  <button
                    type="button"
                    className="button"
                    onClick={async () => {
                      const next = editableProfiles.filter((x) => x.id !== p.id);
                      await saveProfiles(next);
                    }}
                  >
                    Delete
                  </button>
                </div>

                <div className="hr" />

                <div className="kv">
                  <b>Name</b>
                  <input
                    className="input"
                    value={p.name}
                    onChange={(e) => {
                      const next = editableProfiles.map((x) =>
                        x.id === p.id ? { ...x, name: e.target.value } : x,
                      );
                      setEditableProfiles(next);
                    }}
                  />

                  <b>Enabled</b>
                  <label className="row" style={{ gap: 8 }}>
                    <input
                      type="checkbox"
                      checked={p.enabled}
                      onChange={(e) => {
                        const next = editableProfiles.map((x) =>
                          x.id === p.id ? { ...x, enabled: e.target.checked } : x,
                        );
                        setEditableProfiles(next);
                      }}
                    />
                    <span className="small">Apply this profile when matched</span>
                  </label>

                  <b>Matcher</b>
                  <div className="row" style={{ gap: 8, flexWrap: 'wrap' }}>
                    <input
                      className="input"
                      value={p.matchers[0]?.kind ?? 'ProcessNameEquals'}
                      onChange={(e) => {
                        const kind =
                          e.target.value === 'ExePathEquals'
                            ? 'ExePathEquals'
                            : e.target.value === 'WindowTitleContains'
                              ? 'WindowTitleContains'
                              : 'ProcessNameEquals';
                        const currentValue = p.matchers[0]?.value ?? '';
                        const next = editableProfiles.map((x) =>
                          x.id === p.id
                            ? { ...x, matchers: [{ kind, value: currentValue }] }
                            : x,
                        );
                        setEditableProfiles(next);
                      }}
                      aria-label={`profile-${idx}-matcher-kind`}
                      placeholder="ProcessNameEquals"
                    />
                    <input
                      className="input"
                      value={p.matchers[0]?.value ?? ''}
                      onChange={(e) => {
                        const kind = p.matchers[0]?.kind ?? 'ProcessNameEquals';
                        const next = editableProfiles.map((x) =>
                          x.id === p.id
                            ? { ...x, matchers: [{ kind, value: e.target.value }] }
                            : x,
                        );
                        setEditableProfiles(next);
                      }}
                      aria-label={`profile-${idx}-matcher-value`}
                      placeholder="slack.exe"
                    />
                  </div>

                  <b>Overrides</b>
                  <div className="kv">
                    <b>Insert Mode</b>
                    <input
                      className="input"
                      value={p.overrides.insert_mode ?? ''}
                      onChange={(e) => {
                        const v = e.target.value;
                        const insert_mode = v === '' ? null : v === 'PasteAndEnter' ? 'PasteAndEnter' : 'Paste';
                        const next = editableProfiles.map((x) =>
                          x.id === p.id ? { ...x, overrides: { ...x.overrides, insert_mode } } : x,
                        );
                        setEditableProfiles(next);
                      }}
                      placeholder="(inherit) or Paste/PasteAndEnter"
                    />

                    <b>Enable Enhancement</b>
                    <input
                      className="input"
                      value={
                        p.overrides.enable_enhancement === null || p.overrides.enable_enhancement === undefined
                          ? ''
                          : p.overrides.enable_enhancement
                            ? 'true'
                            : 'false'
                      }
                      onChange={(e) => {
                        const raw = e.target.value.trim();
                        const enable_enhancement =
                          raw === '' ? null : raw.toLowerCase() === 'true' ? true : false;
                        const next = editableProfiles.map((x) =>
                          x.id === p.id
                            ? { ...x, overrides: { ...x.overrides, enable_enhancement } }
                            : x,
                        );
                        setEditableProfiles(next);
                      }}
                      placeholder="(inherit) or true/false"
                    />

                    <b>Prompt ID</b>
                    <input
                      className="input"
                      value={p.overrides.prompt_id ?? ''}
                      onChange={(e) => {
                        const prompt_id = e.target.value.trim() === '' ? null : e.target.value.trim();
                        const next = editableProfiles.map((x) =>
                          x.id === p.id ? { ...x, overrides: { ...x.overrides, prompt_id } } : x,
                        );
                        setEditableProfiles(next);
                      }}
                      placeholder="(inherit) or UUID"
                    />

                    <b>STT Provider</b>
                    <input
                      className="input"
                      value={p.overrides.stt_provider ?? ''}
                      onChange={(e) => {
                        const stt_provider = e.target.value.trim() === '' ? null : e.target.value;
                        const next = editableProfiles.map((x) =>
                          x.id === p.id ? { ...x, overrides: { ...x.overrides, stt_provider } } : x,
                        );
                        setEditableProfiles(next);
                      }}
                      placeholder="(inherit) local or elevenlabs"
                    />

                    <b>STT Model</b>
                    <input
                      className="input"
                      value={p.overrides.stt_model ?? ''}
                      onChange={(e) => {
                        const stt_model = e.target.value.trim() === '' ? null : e.target.value;
                        const next = editableProfiles.map((x) =>
                          x.id === p.id ? { ...x, overrides: { ...x.overrides, stt_model } } : x,
                        );
                        setEditableProfiles(next);
                      }}
                      placeholder="(inherit)"
                    />

                    <b>Language</b>
                    <input
                      className="input"
                      value={p.overrides.language ?? ''}
                      onChange={(e) => {
                        const language = e.target.value.trim() === '' ? null : e.target.value;
                        const next = editableProfiles.map((x) =>
                          x.id === p.id ? { ...x, overrides: { ...x.overrides, language } } : x,
                        );
                        setEditableProfiles(next);
                      }}
                      placeholder="(inherit)"
                    />

                    <b>LLM Base URL</b>
                    <input
                      className="input"
                      value={p.overrides.llm_base_url ?? ''}
                      onChange={(e) => {
                        const llm_base_url = e.target.value.trim() === '' ? null : e.target.value;
                        const next = editableProfiles.map((x) =>
                          x.id === p.id ? { ...x, overrides: { ...x.overrides, llm_base_url } } : x,
                        );
                        setEditableProfiles(next);
                      }}
                      placeholder="(inherit)"
                    />

                    <b>LLM Model</b>
                    <input
                      className="input"
                      value={p.overrides.llm_model ?? ''}
                      onChange={(e) => {
                        const llm_model = e.target.value.trim() === '' ? null : e.target.value;
                        const next = editableProfiles.map((x) =>
                          x.id === p.id ? { ...x, overrides: { ...x.overrides, llm_model } } : x,
                        );
                        setEditableProfiles(next);
                      }}
                      placeholder="(inherit)"
                    />
                  </div>
                </div>

                <div className="hr" />

                <div className="row" style={{ justifyContent: 'flex-end' }}>
                  <button
                    type="button"
                    className="button"
                    onClick={async () => {
                      await saveProfiles(editableProfiles);
                    }}
                  >
                    Save Changes
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

type HistoryEntry = import('../lib/types').HistoryEntry;

function History({ invoker }: Props) {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [entries, setEntries] = useState<null | HistoryEntry[]>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function refresh() {
    setBusy(true);
    setMessage(null);
    setError(null);

    try {
      const [cfg, hist] = await Promise.all([invoker.getConfig(), invoker.getHistory()]);
      setEnabled(cfg.defaults.history_enabled);
      setEntries(hist.slice().reverse());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    let cancelled = false;

    async function loadInitial() {
      setBusy(true);
      setMessage(null);
      setError(null);

      try {
        const [cfg, hist] = await Promise.all([invoker.getConfig(), invoker.getHistory()]);
        if (cancelled) return;
        setEnabled(cfg.defaults.history_enabled);
        setEntries(hist.slice().reverse());
      } catch (e) {
        if (cancelled) return;
        setError(String(e));
      } finally {
        if (!cancelled) setBusy(false);
      }
    }

    // Auto-load when the History tab is opened.
    void loadInitial();

    return () => {
      cancelled = true;
    };
  }, [invoker]);

  async function clear() {
    setBusy(true);
    setMessage(null);
    setError(null);

    try {
      await invoker.clearHistory();
      await refresh();
      setMessage('History cleared');
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function copy(text: string) {
    setMessage(null);
    try {
      await navigator.clipboard.writeText(text);
      setMessage('Copied');
    } catch (e) {
      setMessage(`Copy failed: ${String(e)}`);
    }
  }

  const status =
    enabled === null ? 'unknown' : enabled ? 'enabled (saving new transcripts)' : 'disabled (not saving new transcripts)';

  return (
    <div>
      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div>
          <div className="small">Transcript history stored on disk.</div>
          <div className="small">Status: {status}</div>
        </div>
        <div className="row" style={{ gap: 8 }}>
          <button type="button" className="button" onClick={refresh} disabled={busy}>
            Refresh
          </button>
          <button type="button" className="button" onClick={clear} disabled={busy}>
            Clear
          </button>
        </div>
      </div>

      {error ? <div className="small">Error: {error}</div> : null}
      {message ? <div className="small">{message}</div> : null}

      <div className="hr" />

      {entries === null ? (
        <div className="small">Press Refresh to load history.</div>
      ) : entries.length === 0 ? (
        <div className="small">No history entries yet.</div>
      ) : (
        <div className="kv">
          {entries.map((h, idx) => {
            const when = new Date(h.ts_unix_ms).toLocaleString();
            const app = h.app_process_name ?? h.app_window_title ?? h.app_exe_path ?? 'unknown app';
            const preview = h.text.length > 240 ? `${h.text.slice(0, 240)}…` : h.text;

            return (
              <div key={`${h.ts_unix_ms}-${idx}`} style={{ gridColumn: '1 / -1' }}>
                <div className="card" style={{ padding: 12 }}>
                  <div className="row" style={{ justifyContent: 'space-between' }}>
                    <div className="badge">{when}</div>
                    <div className="row" style={{ gap: 8 }}>
                      <div className="badge">{h.stage}</div>
                      <button type="button" className="button" onClick={() => void copy(h.text)}>
                        Copy
                      </button>
                    </div>
                  </div>

                  <div className="hr" />

                  <div className="small">App: {app}</div>
                  <div style={{ fontFamily: 'var(--mono)', fontSize: 12 }}>{preview}</div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
