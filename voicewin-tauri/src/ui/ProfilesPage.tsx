import { useCallback, useEffect, useMemo, useState } from 'react';

import type { AppConfig, PowerModeProfile } from '../lib/types';
import { decodePowerModeProfile, encodePowerModeProfile } from '../lib/types';

type ForegroundAppInfo = {
  process_name?: string | null;
  exe_path?: string | null;
  window_title?: string | null;
};

function newProfile(): PowerModeProfile {
  const id = crypto.randomUUID();
  return {
    id,
    name: 'New Profile',
    enabled: true,
    matchers: [{ kind: 'ProcessNameEquals', value: '' }],
    overrides: {},
  };
}

export function ProfilesPage() {
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [profiles, setProfiles] = useState<PowerModeProfile[] | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const c = await invoke<AppConfig>('get_config');
      setCfg(c);
      const decoded = c.profiles.map(decodePowerModeProfile);
      setProfiles(decoded);
      if (decoded.length > 0 && !selectedId) {
        setSelectedId(decoded[0].id);
      }
      setError(null);
    } catch (e) {
      setError(String(e));
      setCfg(null);
      setProfiles([]);
    }
  }, [selectedId]);

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
              const p = newProfile();
              const next = [...profiles, p];
              setSelectedId(p.id);
              await save(next);
            }}
          >
            +
          </button>
        </div>

        <div style={{ marginTop: 'var(--space-12)', display: 'grid', gap: 'var(--space-8)' }}>
          {profiles.map((p) => {
            const selected = p.id === selectedId;
            return (
              <button
                key={p.id}
                type="button"
                onClick={() => setSelectedId(p.id)}
                style={{
                  height: 64,
                  padding: 12,
                  borderRadius: 'var(--radius-card)',
                  border: '1px solid transparent',
                  background: selected ? 'rgba(255,255,255,0.18)' : 'transparent',
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
                    {p.name}
                  </div>
                  <div className="vw-type-caption" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {p.matchers.find((m) => m.kind === 'ProcessNameEquals')?.value || '—'}
                  </div>
                </div>
              </button>
            );
          })}
        </div>
      </div>

      <div style={{ padding: 'var(--space-32)' }}>
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
            <input
              className="vw-input"
              value={selected.name}
              onChange={(e) => {
                const next = profiles.map((p) => (p.id === selected.id ? { ...p, name: e.target.value } : p));
                setProfiles(next);
              }}
              onBlur={async () => {
                if (!profiles) return;
                await save(profiles);
              }}
            />

            <div>
              <div className="vw-type-bodyStrong">Target Application</div>
              <div style={{ marginTop: 'var(--space-8)', display: 'grid', gridTemplateColumns: '1fr auto', gap: 'var(--space-12)' }}>
                <input
                  className="vw-input"
                  placeholder="code.exe"
                  value={selected.matchers.find((m) => m.kind === 'ProcessNameEquals')?.value ?? ''}
                  onChange={(e) => {
                    const value = e.target.value;
                    const next = profiles.map((p) => {
                      if (p.id !== selected.id) return p;
                      const others = p.matchers.filter((m) => m.kind !== 'ProcessNameEquals');
                      return { ...p, matchers: [...others, { kind: 'ProcessNameEquals', value }] };
                    });
                    setProfiles(next);
                  }}
                  onBlur={async () => {
                    if (!profiles) return;
                    await save(profiles);
                  }}
                />

                <button
                  type="button"
                  className="vw-button vw-button--secondary"
                  onClick={async () => {
                    try {
                      const { invoke } = await import('@tauri-apps/api/core');
                      const info = await invoke<ForegroundAppInfo>('capture_foreground_app');
                      const proc = info.process_name ?? '';

                      const next = profiles.map((p) => {
                        if (p.id !== selected.id) return p;
                        const others = p.matchers.filter((m) => m.kind !== 'ProcessNameEquals');
                        return { ...p, matchers: [...others, { kind: 'ProcessNameEquals', value: proc }] };
                      });

                      setProfiles(next);
                      await save(next);
                    } catch (e) {
                      setError(String(e));
                    }
                  }}
                >
                  Pick Window
                </button>
              </div>
            </div>

            <div>
              <div className="vw-type-bodyStrong">Overrides</div>
              <div className="vw-type-caption" style={{ marginTop: 'var(--space-8)' }}>
                Override UI is stubbed for now; core matching + target app capture is implemented.
              </div>
            </div>

            <div style={{ display: 'flex', gap: 'var(--space-12)' }}>
              <button
                type="button"
                className="vw-button vw-button--secondary"
                onClick={async () => {
                  const next = profiles.filter((p) => p.id !== selected.id);
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
