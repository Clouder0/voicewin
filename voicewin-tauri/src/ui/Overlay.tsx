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

type BridgeState = {
  isTauri: boolean;
  listenOk: boolean;
  invokeOk: boolean;
  overlayReadyOk: boolean;
  lastError: string | null;
};

// Mic levels are emitted on Windows and macOS (best-effort).

function clamp01(v: number): number {
  if (Number.isNaN(v)) return 0;
  if (v < 0) return 0;
  if (v > 1) return 1;
  return v;
}

function clipText(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, max) + '…';
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

  // If we never receive a status update (e.g. event bridge fails), show a minimal
  // fallback pill so the overlay window is never a blank "stuck" rectangle.
  const [idleFallback, setIdleFallback] = useState(false);
  useEffect(() => {
    const t = window.setTimeout(() => setIdleFallback(true), 300);
    return () => window.clearTimeout(t);
  }, []);

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

  const [bridge, setBridge] = useState<BridgeState>({
    isTauri: true,
    listenOk: false,
    invokeOk: false,
    overlayReadyOk: false,
    lastError: null,
  });

  useEffect(() => {
    let unlistenStatus: null | (() => void) = null;
    let unlistenLevel: null | (() => void) = null;

    async function start() {
      try {
        const core = await import('@tauri-apps/api/core');
        if (!core.isTauri()) {
          setBridge((b) => ({ ...b, isTauri: false, lastError: 'not running inside Tauri' }));
          return;
        }
        setBridge((b) => ({ ...b, isTauri: true }));

        try {
          const { listen } = await import('@tauri-apps/api/event');
          unlistenStatus = await listen<SessionStatusPayload>('voicewin://session_status', (e) => {
            setStatus(e.payload);
          });

          // Optional: only emitted when backend supports mic levels.
          unlistenLevel = await listen<MicLevelPayload>('voicewin://mic_level', (e) => {
            setLevels(e.payload);
          });

          setBridge((b) => ({ ...b, listenOk: true }));
        } catch (e) {
          setBridge((b) => ({ ...b, listenOk: false, lastError: String(e) }));
        }

        // Tell the backend we're ready so it can re-emit the current status.
        // Do this *after* listeners are attached.
        try {
          await core.invoke('overlay_ready');
          setBridge((b) => ({ ...b, overlayReadyOk: true }));
        } catch (e) {
          setBridge((b) => ({ ...b, overlayReadyOk: false, lastError: String(e) }));
        }

        // Best-effort: fetch current status in case we missed the first emit.
        try {
          const current = await core.invoke<SessionStatusPayload>('get_session_status');
          setStatus(current);
          setBridge((b) => ({ ...b, invokeOk: true }));
        } catch (e) {
          setBridge((b) => ({ ...b, invokeOk: false, lastError: String(e) }));
        }
      } catch (e) {
        setBridge((b) => ({ ...b, isTauri: false, lastError: String(e) }));
      }
    }

    void start();

    return () => {
      if (unlistenStatus) unlistenStatus();
      if (unlistenLevel) unlistenLevel();
    };
  }, []);

  // Best-effort: poll session status briefly on mount.
  // This helps recover if the overlay window is shown before listeners attach.
  useEffect(() => {
    let stop = false;

    async function poll() {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        for (let i = 0; i < 10; i++) {
          if (stop) return;
          try {
            const current = await invoke<SessionStatusPayload>('get_session_status');
            setStatus(current);
            if (current.stage !== 'idle') {
              return;
            }
          } catch {
            // Ignore.
          }
          await new Promise((r) => setTimeout(r, 250));
        }
      } catch {
        // Not running inside Tauri.
      }
    }

    void poll();
    return () => {
      stop = true;
    };
  }, []);

  // If the overlay missed the initial status event (common when the window is created hidden),
  // keep polling while we're in fallback so we can recover to "Listening" quickly.
  useEffect(() => {
    if (!idleFallback) return;

    let cancelled = false;
    let inFlight = false;

    async function tick() {
      if (cancelled) return;
      if (inFlight) return;

      inFlight = true;
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const current = await invoke<SessionStatusPayload>('get_session_status');
        setStatus(current);
      } catch {
        // Ignore: if IPC isn't ready yet, we will try again.
      } finally {
        inFlight = false;
      }
    }

    // Poll at a modest cadence; stop once we receive a non-idle stage (idleFallback will clear).
    void tick();
    const id = window.setInterval(() => void tick(), 250);

    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [idleFallback]);

  useEffect(() => {
    if (status.stage !== 'idle' && idleFallback) {
      setIdleFallback(false);
    }
  }, [idleFallback, status.stage]);

  const isVisible =
    (status.stage !== 'idle' && status.stage !== 'done') || idleFallback;

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
    const visible = isVisible && status.stage !== 'done';
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
  }, [isVisible, status.stage]);

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
    if (status.stage === 'idle') return 'Connecting…';
    if (status.stage === 'recording') return 'Listening...';
    if (status.stage === 'enhancing') return 'Enhancing...';
    if (status.stage === 'transcribing' || status.stage === 'inserting') return 'Thinking...';
    if (status.stage === 'success') return 'Inserted';
    if (status.stage === 'cancelled') return 'Cancelled';
    if (status.stage === 'error') return status.error ? status.error : 'Error';
    return '';
  })();

  const subtitle = useMemo(() => {
    // `status.error` is a transient status message (not always a hard error).
    if (status.stage !== 'error' && status.error) {
      return status.error;
    }

    // If the overlay is stuck in the fallback state, surface IPC diagnostics.
    if (idleFallback && status.stage === 'idle') {
      const err = bridge.lastError ? clipText(bridge.lastError, 140) : null;
      if (!bridge.isTauri) return err ?? 'IPC unavailable';
      if (!bridge.listenOk && !bridge.invokeOk) return err ?? 'IPC blocked (check capabilities)';
      if (!bridge.listenOk) return err ?? 'Events unavailable';
      if (!bridge.invokeOk) return err ?? 'Status unavailable';
      if (!bridge.overlayReadyOk) return err ?? 'Sync unavailable';
    }

    return null;
  }, [bridge.isTauri, bridge.lastError, bridge.listenOk, bridge.invokeOk, bridge.overlayReadyOk, idleFallback, status.error, status.stage]);

  const leftKind = (() => {
    if (status.stage === 'idle') return 'spinner';
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

              {subtitle ? (
                <div className="vw-type-caption" style={{ marginTop: 2, color: 'var(--text-secondary)' }}>
                  {subtitle}
                </div>
              ) : null}
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
              </div>
            ) : null}

            {/* Always provide a dismiss button so a broken overlay can't trap the user. */}
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
