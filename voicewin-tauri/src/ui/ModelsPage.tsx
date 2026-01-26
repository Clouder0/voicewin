import { useCallback, useEffect, useMemo, useState } from 'react';

type ModelCatalogEntry = {
  id: string;
  title: string;
  recommended: boolean;
  filename: string;
  size_bytes?: number | null;
  speed_label?: string | null;
  accuracy_label?: string | null;
  installed: boolean;
  active: boolean;
  downloading: boolean;
};

type DownloadProgress = {
  model_id: string;
  downloaded_bytes: number;
  total_bytes?: number | null;
};

function formatBytes(n: number | null | undefined): string {
  if (!n || n <= 0) return '';
  const gb = 1024 * 1024 * 1024;
  const mb = 1024 * 1024;
  if (n >= gb) return `${(n / gb).toFixed(1)} GB`;
  if (n >= mb) return `${(n / mb).toFixed(0)} MB`;
  return `${n} B`;
}

export function ModelsPage() {
  const [models, setModels] = useState<ModelCatalogEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});

  const refresh = useCallback(async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const list = await invoke<ModelCatalogEntry[]>('list_models');
      setModels(list);
      setError(null);
    } catch (e) {
      setError(String(e));
      setModels([]);
    }
  }, []);

  useEffect(() => {
    let unlisten1: null | (() => void) = null;
    let unlisten2: null | (() => void) = null;
    let stop = false;

    async function start() {
      await refresh();

      try {
        const { listen } = await import('@tauri-apps/api/event');

        unlisten1 = await listen<DownloadProgress>('voicewin://model_download_progress', (e) => {
          const p = e.payload;
          setProgress((prev) => ({ ...prev, [p.model_id]: p }));
        });

        unlisten2 = await listen<string>('voicewin://model_download_done', (e) => {
          const id = e.payload;
          setProgress((prev) => {
            const next = { ...prev };
            delete next[id];
            return next;
          });
          void refresh();
        });
      } catch {
        // Not running in Tauri.
      }

      // Best-effort refresh in case events are missed.
      while (!stop) {
        await new Promise((r) => setTimeout(r, 3000));
        if (stop) break;
        await refresh();
      }
    }

    void start();

    return () => {
      stop = true;
      if (unlisten1) unlisten1();
      if (unlisten2) unlisten2();
    };
  }, [refresh]);

  const cards = useMemo(() => models ?? [], [models]);

  return (
    <div style={{ padding: 'var(--space-32)' }}>
      <div className="vw-type-title">Model Library</div>

      {error ? (
        <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)', color: 'var(--color-danger-fg)' }}>
          {error}
        </div>
      ) : null}

      <div
        style={{
          marginTop: 'var(--space-16)',
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))',
          gap: 'var(--space-16)',
        }}
      >
        {cards.map((m) => {
          const p = progress[m.id];
          const frac = p?.total_bytes ? Math.max(0, Math.min(1, p.downloaded_bytes / p.total_bytes)) : null;

          return (
            <div key={m.id} className="vw-card" style={{ height: 140, padding: 'var(--space-16)', display: 'grid', gap: 'var(--space-8)' }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 'var(--space-12)' }}>
                <div className="vw-type-subtitle">{m.title}</div>
                {m.recommended ? (
                  <div
                    style={{
                      background: 'var(--color-accent)',
                      color: 'var(--color-accent-text)',
                      borderRadius: 2,
                      padding: '2px 6px',
                      fontSize: 10,
                      fontWeight: 600,
                    }}
                  >
                    Recommend
                  </div>
                ) : null}
              </div>

              <div className="vw-type-caption">
                {[formatBytes(m.size_bytes ?? null), m.speed_label ?? '', m.accuracy_label ?? '']
                  .filter((x) => x && x.length > 0)
                  .join(' • ')}
              </div>

              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 'var(--space-12)' }}>
                {m.active ? (
                  <button type="button" className="vw-button vw-button--secondary" disabled>
                    Active
                  </button>
                ) : m.installed ? (
                  <button
                    type="button"
                    className="vw-button vw-button--secondary"
                    onClick={async () => {
                      try {
                        const { invoke } = await import('@tauri-apps/api/core');
                        await invoke('set_active_model', { modelId: m.id });
                        await refresh();
                      } catch (e) {
                        setError(String(e));
                      }
                    }}
                  >
                    Set Active
                  </button>
                ) : p ? (
                  <button type="button" className="vw-button vw-button--primary" disabled>
                    Downloading…
                  </button>
                ) : (
                  <button
                    type="button"
                    className="vw-button vw-button--primary"
                    onClick={async () => {
                      try {
                        const { invoke } = await import('@tauri-apps/api/core');
                        setError(null);
                        await invoke('download_model', { modelId: m.id });
                      } catch (e) {
                        setError(String(e));
                      }
                    }}
                  >
                    Download
                  </button>
                )}

                {p ? (
                  <div className="vw-type-caption">
                    {p.total_bytes ? `${Math.round(frac ? frac * 100 : 0)}%` : `${Math.round(p.downloaded_bytes / (1024 * 1024))} MB`}
                  </div>
                ) : null}
              </div>

                {p ? (
                  <div style={{ height: 2, width: '100%', background: 'var(--stroke-card)', alignSelf: 'end' }}>
                    <div style={{ height: 2, width: `${Math.round((frac ?? 0) * 100)}%`, background: 'var(--color-accent)' }} />
                  </div>
                ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
