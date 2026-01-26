import { useEffect, useMemo, useState } from 'react';

  type SessionStage =
    | 'idle'
    | 'recording'
    | 'transcribing'
    | 'enhancing'
    | 'inserting'
    | 'success'
    | 'done'
    | 'error'
    | 'cancelled'
    | 'busy';

type SessionStatusPayload = {
  stage: SessionStage;
  stage_label: string;
  is_recording: boolean;
  elapsed_ms?: number | null;
  error?: string | null;
  last_text_preview?: string | null;
  last_text_available: boolean;
};

type MicLevelPayload = {
  rms: number;
  peak: number;
};

// Mic levels are emitted on Windows and macOS (best-effort).

function clamp01(v: number): number {
  if (Number.isNaN(v)) return 0;
  if (v < 0) return 0;
  if (v > 1) return 1;
  return v;
}

function meterBars(level: number, bars: number): boolean[] {
  const out: boolean[] = [];
  const v = clamp01(level);
  for (let i = 0; i < bars; i++) {
    out.push(v >= (i + 1) / bars);
  }
  return out;
}

export function Overlay() {
  const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.userAgent);

  const [status, setStatus] = useState<SessionStatusPayload>({
    stage: 'idle',
    stage_label: 'idle',
    is_recording: false,
    elapsed_ms: null,
    error: null,
    last_text_preview: null,
    last_text_available: false,
  });

  const [levels, setLevels] = useState<MicLevelPayload>({ rms: 0, peak: 0 });

  useEffect(() => {
    let unlistenStatus: null | (() => void) = null;
    let unlistenLevel: null | (() => void) = null;

    async function start() {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const { invoke } = await import('@tauri-apps/api/core');

        unlistenStatus = await listen<SessionStatusPayload>('voicewin://session_status', (e) => {
          setStatus(e.payload);
        });

        // Optional: only emitted when backend supports mic levels.
        unlistenLevel = await listen<MicLevelPayload>('voicewin://mic_level', (e) => {
          setLevels(e.payload);
        });

        // Best-effort: fetch current status in case we missed the first emit
        // (e.g. overlay window is shown before listeners attach).
        try {
          const current = await invoke<SessionStatusPayload>('get_session_status');
          setStatus(current);
        } catch {
          // Ignore.
        }
      } catch {
        // Not running inside Tauri.
      }
    }

    void start();

    return () => {
      if (unlistenStatus) unlistenStatus();
      if (unlistenLevel) unlistenLevel();
    };
  }, []);

  const isVisible =
    status.stage !== 'idle' && status.stage !== 'cancelled' && status.stage !== 'done';

  const [isExiting, setIsExiting] = useState(false);


  // Auto-dismiss success after spec delay + exit animation.
  useEffect(() => {
    if (status.stage !== 'success') {
      // Reset exit state when leaving success.
      if (isExiting) setIsExiting(false);
      return;
    }

    // Only schedule once per success.
    if (isExiting) return;

    let cancelled = false;

    async function schedule() {
      // 1500ms hold (spec), then start exit animation.
      await new Promise((r) => setTimeout(r, 1500));
      if (cancelled) return;
      setIsExiting(true);

      // Allow the CSS exit animation to run before hiding the window.
      await new Promise((r) => setTimeout(r, 150));
      if (cancelled) return;

      try {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('overlay_dismiss');
      } catch {
        // Ignore.
      }
    }

    void schedule();

    return () => {
      cancelled = true;
    };
  }, [isExiting, status.stage]);

  // Fit-content sizing: measure pill and ask backend to resize the overlay window.
  // We trigger this when the stage changes (content width changes across states).
  useEffect(() => {
    const visible = status.stage !== 'idle' && status.stage !== 'cancelled' && status.stage !== 'done';
    if (!visible) return;

    let raf = 0;
    let stop = false;

    async function resizeOnce() {
      try {
        const pill = document.querySelector<HTMLElement>('[data-vw-overlay-pill]');
        if (!pill) return;

        const rect = pill.getBoundingClientRect();

        // Spec: min 160 max 600.
        const minW = 160;
        const maxW = 600;

        // Add a small safety margin for box-shadow.
        const shadowPad = 24;

        const width = Math.max(minW, Math.min(maxW, Math.ceil(rect.width) + shadowPad));
        const height = 48 + shadowPad;

        // Rust expects f64 values for logical sizing.
        const widthF = Number(width);
        const heightF = Number(height);

        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('overlay_set_size', { width: widthF, height: heightF });
      } catch {
        // Ignore.
      }
    }

    // Defer to next frame so layout is stable.
    raf = window.requestAnimationFrame(() => {
      if (stop) return;
      void resizeOnce();
    });

    return () => {
      stop = true;
      if (raf) window.cancelAnimationFrame(raf);
    };
  }, [status.stage]);

  const meter = useMemo(() => {
    // Spec: 5 bars, height 4px..24px during recording.
    const base = status.stage === 'recording' ? Math.max(levels.rms, levels.peak) : 0;
    return meterBars(base, 5);
  }, [levels.peak, levels.rms, status.stage]);

  const handlePointerDown = async (e: React.PointerEvent) => {
    // Only begin dragging when the pointer originates from the non-interactive region.
    // This prevents button clicks from turning into drags.
    const target = e.target as HTMLElement | null;
    if (target?.closest('button')) {
      return;
    }

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const { getCurrentWindow } = await import('@tauri-apps/api/window');

      // Mark drag begin (backend may use this for persistence).
      await invoke('overlay_drag_begin');

      // This resolves when the native drag operation ends.
      await getCurrentWindow().startDragging();

      // Persist final position.
      await invoke('overlay_drag_end');
    } catch {
      // Not running inside Tauri.
    }
  };

  const pillText = (() => {
    if (status.stage === 'recording') return 'Listening...';
    if (status.stage === 'enhancing') return 'Enhancing...';
    if (status.stage === 'transcribing' || status.stage === 'inserting') return 'Thinking...';
    if (status.stage === 'success') return 'Inserted';
    if (status.stage === 'error') return status.error ? 'Could not insert. Saved to History.' : 'Error';
    return '';
  })();

  const leftKind = (() => {
    if (status.stage === 'recording') return 'mic';
    if (status.stage === 'transcribing' || status.stage === 'enhancing' || status.stage === 'inserting') return 'spinner';
    if (status.stage === 'success') return 'check';
    if (status.stage === 'error') return 'error';
    return 'none';
  })();

  const showStop = status.stage === 'recording';
  const showCancel = status.stage === 'transcribing' || status.stage === 'enhancing';

  const needsAccessibility =
    isMac && typeof status.error === 'string' && status.error.toLowerCase().includes('accessibility');
  const needsMicrophone =
    isMac && typeof status.error === 'string' && status.error.toLowerCase().includes('microphone');

  return (
    <div className="vw-overlayRoot">
      {isVisible ? (
        <div
          className="vw-hud"
          data-stage={status.stage}
          data-vw-overlay-pill
          data-exiting={isExiting ? 'true' : 'false'}
          onPointerDown={handlePointerDown}
        >
          <div className="vw-hudLeft">
            {leftKind === 'mic' ? <div className="vw-hudMic" aria-hidden="true" /> : null}
            {leftKind === 'spinner' ? <div className="vw-hudSpinner" aria-hidden="true" /> : null}
            {leftKind === 'check' ? <div className="vw-hudSuccessIcon" aria-hidden="true" /> : null}
            {leftKind === 'error' ? <div className="vw-hudErrorIcon" aria-hidden="true" /> : null}
          </div>

            <div className="vw-hudCenter">
              {status.stage === 'recording' ? (
                <div className="vw-hudVisualizer" aria-hidden="true">
                  {meter.map((on, i) => {
                    const id = i + 1;
                    return <div key={`bar-${id}`} className={on ? 'vw-bar on' : 'vw-bar'} />;
                  })}
                </div>
              ) : null}

              <div
                className={
                  status.stage === 'recording' || status.stage === 'success'
                    ? 'vw-type-bodyStrong'
                    : 'vw-type-body'
                }
              >
                {pillText}
              </div>
            </div>

          <div className="vw-hudRight">
            {showStop ? (
              <button
                type="button"
                className="vw-button vw-button--ghost vw-iconButton"
                aria-label="Stop"
                onClick={async () => {
                  try {
                    const { invoke } = await import('@tauri-apps/api/core');
                    await invoke('toggle_recording');
                  } catch {
                    // Ignore.
                  }
                }}
              >
                ■
              </button>
            ) : null}

            {showCancel ? (
              <button
                type="button"
                className="vw-button vw-button--ghost vw-iconButton"
                aria-label="Cancel"
                onClick={async () => {
                  try {
                    const { invoke } = await import('@tauri-apps/api/core');
                    await invoke('cancel_recording');
                  } catch {
                    // Ignore.
                  }
                }}
              >
                ✕
              </button>
            ) : null}

            {status.stage === 'error' ? (
              <div style={{ display: 'flex', gap: 'var(--space-8)' }}>
                {needsAccessibility ? (
                  <button
                    type="button"
                    className="vw-button vw-button--ghost"
                    style={{ height: 'var(--hud-button-size)', padding: '0 var(--space-12)' }}
                    aria-label="Open Accessibility Settings"
                    onClick={async () => {
                      try {
                        const { invoke } = await import('@tauri-apps/api/core');
                        await invoke('open_macos_accessibility_settings');
                      } catch {
                        // Ignore.
                      }
                    }}
                  >
                    Accessibility
                  </button>
                ) : null}

                {needsMicrophone ? (
                  <button
                    type="button"
                    className="vw-button vw-button--ghost"
                    style={{ height: 'var(--hud-button-size)', padding: '0 var(--space-12)' }}
                    aria-label="Open Microphone Settings"
                    onClick={async () => {
                      try {
                        const { invoke } = await import('@tauri-apps/api/core');
                        await invoke('open_macos_microphone_settings');
                      } catch {
                        // Ignore.
                      }
                    }}
                  >
                    Microphone
                  </button>
                ) : null}

                <button
                  type="button"
                  className="vw-button vw-button--ghost"
                  style={{ height: 'var(--hud-button-size)', padding: '0 var(--space-12)' }}
                  aria-label="Open History"
                  onClick={async () => {
                    try {
                      const { invoke } = await import('@tauri-apps/api/core');
                      const { emit } = await import('@tauri-apps/api/event');

                      // Bring main window forward and switch to History.
                      await emit('voicewin://navigate', 'history');
                      await invoke('show_main_window');

                      // Dismiss overlay.
                      await invoke('overlay_dismiss');
                    } catch {
                      // Ignore.
                    }
                  }}
                >
                  History
                </button>
                <button
                  type="button"
                  className="vw-button vw-button--ghost vw-iconButton"
                  aria-label="Dismiss"
                  onClick={async () => {
                    try {
                      const { invoke } = await import('@tauri-apps/api/core');
                      await invoke('overlay_dismiss');
                    } catch {
                      // Ignore.
                    }
                  }}
                >
                  ✕
                </button>
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      {status.stage === 'error' && status.error ? (
        <div className="vw-hudErrorText" style={{ display: 'none' }}>
          {status.error}
        </div>
      ) : null}

    </div>
  );
}
