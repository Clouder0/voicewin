import { useMemo, useState } from 'react';

// In real app, we will use @tauri-apps/api/core.invoke to call Rust commands.
// For unit tests and non-tauri browser runs, we keep this component pure.

import { createMockInvoker, type Invoker } from '../lib/invoker';
import { Settings } from './Settings';

type Props = {
  invoker?: Invoker;
};

export function App({ invoker }: Props) {
  const api = invoker ?? createMockInvoker();

  const [status, setStatus] = useState({ stage: 'idle' } as { stage: string; final_text?: string; error?: string });
  const [transcript, setTranscript] = useState('rewrite hello team rewrite');

  const summary = useMemo(() => (status.final_text ? 'Ready' : 'Idle'), [status.final_text]);

  return (
    <div className="container">
      <div className="header">
        <div>
          <div className="title">VoiceWin</div>
          <div className="subtitle">Outline UI · Tauri v2 · Windows target</div>
        </div>
        <div className="badge">stage={status.stage}</div>
      </div>

      <div className="grid">
        <div className="card">
          <div className="row" style={{ justifyContent: 'space-between' }}>
            <div>
              <div style={{ fontSize: 13, letterSpacing: 0.4 }}>Session</div>
              <div className="small">Run a mock session from the UI.</div>
            </div>
              <div className="row" style={{ gap: 8 }}>
                 <button
                   type="button"
                   className="button"
                   onClick={async () => {
                     setStatus({ stage: 'running' });
                     try {
                       const res = await api.toggleRecording();
                       setStatus(res);
                     } catch (e) {
                       setStatus({ stage: 'error', error: String(e) });
                     }
                   }}
                 >
                   Toggle Recording
                 </button>

                 <button
                   type="button"
                   className="button"
                   onClick={async () => {
                     setStatus({ stage: 'running' });
                     try {
                       const res = await api.cancelRecording();
                       setStatus(res);
                     } catch (e) {
                       setStatus({ stage: 'error', error: String(e) });
                     }
                   }}
                 >
                   Cancel Recording
                 </button>

                <button
                  type="button"
                  className="button"
                  onClick={async () => {
                    setStatus({ stage: 'running' });
                    try {
                      const res = await api.runSession({ transcript });
                      setStatus(res);
                    } catch (e) {
                      setStatus({ stage: 'error', error: String(e) });
                    }
                  }}
                >
                  Run
                </button>
              </div>

          </div>

          <div className="hr" />

          <div className="kv">
            <b>Transcript</b>
            <input
              className="input"
              value={transcript}
              onChange={(e) => setTranscript(e.target.value)}
              aria-label="transcript"
            />

            <b>Result</b>
            <div style={{ fontFamily: 'var(--mono)', fontSize: 12 }}>
              {status.error ? `Error: ${status.error}` : status.final_text ?? '—'}
            </div>

            <b>Summary</b>
            <div>{summary}</div>
          </div>
        </div>

        <Settings invoker={api} />
      </div>
    </div>
  );
}
