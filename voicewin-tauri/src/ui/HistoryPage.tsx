import { useCallback, useEffect, useMemo, useState } from 'react';

import type { HistoryEntry } from '../lib/types';

function formatTime(tsUnixMs: number): string {
  const d = new Date(tsUnixMs);
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  return `${hh}:${mm}`;
}

export function HistoryPage() {
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

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
        <div
          style={{
            height: 32,
            borderBottom: '1px solid var(--stroke-card)',
            display: 'grid',
            gridTemplateColumns: '100px 150px 1fr 80px',
            alignItems: 'center',
            padding: '0 var(--space-12)',
          }}
        >
          <div className="vw-type-caption" style={{ color: 'var(--text-secondary)' }}>
            Time
          </div>
          <div className="vw-type-caption" style={{ color: 'var(--text-secondary)' }}>
            App
          </div>
          <div className="vw-type-caption" style={{ color: 'var(--text-secondary)' }}>
            Transcript
          </div>
          <div className="vw-type-caption" style={{ color: 'var(--text-secondary)' }}>
            Actions
          </div>
        </div>

        {rows.map((r, idx) => {
          const app = r.app_process_name ?? '—';
          const text = r.text && r.text.trim().length > 0 ? r.text : (r.error ?? '');
          const rowId = r.id && r.id.trim().length > 0 ? r.id : null;

          return (
            <div
              key={rowId ?? `${r.ts_unix_ms}-${r.text}-${idx}`}
              className="vw-historyRow"
              style={{
                height: 56,
                borderBottom: '1px solid var(--stroke-card)',
                display: 'grid',
                gridTemplateColumns: '100px 150px 1fr 80px',
                alignItems: 'center',
                padding: '0 var(--space-12)',
              }}
            >
              <div className="vw-type-caption">{formatTime(r.ts_unix_ms)}</div>
              <div className="vw-type-caption" title={app} style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {app}
              </div>
              <div
                className="vw-type-body"
                title={text}
                style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
              >
                {text}
              </div>
              <div className="vw-historyActions" style={{ display: 'flex', gap: 'var(--space-8)', justifyContent: 'flex-end' }}>
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
          );
        })}
      </div>
    </div>
  );
}
