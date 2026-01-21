import { useEffect, useMemo, useState } from 'react';

type SessionStage =
  | 'idle'
  | 'recording'
  | 'transcribing'
  | 'enhancing'
  | 'inserting'
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

function formatElapsed(ms: number | null | undefined): string {
  const total = Math.max(0, Math.floor((ms ?? 0) / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, '0')}`;
}

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

        const u1 = await listen<SessionStatusPayload>('voicewin://session_status', (e) => {
          setStatus(e.payload);
        });
        const u2 = await listen<MicLevelPayload>('voicewin://mic_level', (e) => {
          setLevels(e.payload);
        });

        unlistenStatus = u1;
        unlistenLevel = u2;
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

  useEffect(() => {
    let stop = false;

    async function tick() {
      while (!stop) {
        try {
          const { invoke } = await import('@tauri-apps/api/core');
          const next = await invoke<SessionStatusPayload>('get_session_status');
          setStatus(next);
          // eslint-disable-next-line no-empty
        } catch {}

        await new Promise((r) => setTimeout(r, 250));
      }
    }

    void tick();

    return () => {
      stop = true;
    };
  }, []);

  const title = useMemo(() => {
    if (status.stage === 'recording') return 'Recording';
    if (status.stage === 'transcribing') return 'Transcribing';
    if (status.stage === 'enhancing') return 'Enhancing';
    if (status.stage === 'inserting') return 'Inserting';
    if (status.stage === 'error') return 'Error';
    return 'Idle';
  }, [status.stage]);

  const meter = useMemo(() => {
    const v = status.stage === 'recording' ? Math.max(levels.rms, levels.peak) : 0;
    return meterBars(v, 12);
  }, [levels.peak, levels.rms, status.stage]);

  const handlePointerDown = async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('overlay_drag_begin');

      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().startDragging();
    } catch {
      // Not running inside Tauri.
    }
  };

  const handlePointerUp = async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('overlay_drag_end');

      const { emit } = await import('@tauri-apps/api/event');
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const pos = await getCurrentWindow().outerPosition();
      await emit('voicewin://overlay_moved', { x: pos.x, y: pos.y });
    } catch {
      // Not running inside Tauri.
    }
  };

  return (
    <div className="overlayRoot" data-stage={status.stage}>
      <div className="overlayFrame" onPointerDown={handlePointerDown} onPointerUp={handlePointerUp}>
        <div className="overlayLeft">
          <div className={status.stage === 'recording' ? 'overlayDot on' : 'overlayDot'} />
          <div className="overlayTitle">{title}</div>
        </div>

        <div className="overlayCenter">
          {meter.map((on, i) => {
            const id = i + 1;
            return <div key={`bar-${id}`} className={on ? 'bar on' : 'bar'} />;
          })}
        </div>

        <div className="overlayRight">
          <div className="overlayTime">{formatElapsed(status.elapsed_ms)}</div>
        </div>
      </div>

      {status.error ? <div className="overlayError">{status.error}</div> : null}

      {status.stage !== 'recording' && status.last_text_available && status.last_text_preview ? (
        <div className="overlayPreview">{status.last_text_preview}</div>
      ) : null}
    </div>
  );
}
