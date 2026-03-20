import { useEffect, useMemo, useState } from 'react';

import type { PlatformCapabilities } from '../lib/types';
import { keydownToHotkey } from './hotkey';
import { fallbackPlatformCapabilities, loadPlatformCapabilities } from './platformCapabilities';

type HotkeyState = {
  hotkey: string;
  error?: string | null;
};

type ModelStatus = {
  bootstrap_ok: boolean;
  bootstrap_path: string;
  preferred_ok: boolean;
  preferred_path: string;
};

type MicrophoneDevice = {
  id: string;
  name: string;
  is_default: boolean;
  is_selected: boolean;
};

type MacosPermissionsStatus = {
  accessibility_trusted: boolean;
};

function splitHotkey(hotkey: string): string[] {
  return hotkey
    .split('+')
    .map((p) => p.trim())
    .filter((p) => p.length > 0);
}

function displayHotkeyPart(part: string, isMac: boolean): string {
  if (!isMac) return part;
  if (part === 'Alt') return 'Option';
  if (part === 'Super') return 'Cmd';
  return part;
}

function HotkeyKbd({ hotkey, isMac, onClick }: { hotkey: string; isMac: boolean; onClick: () => void }) {
  const parts = splitHotkey(hotkey);
  return (
    <button type="button" className="vw-hotkeyButton" onClick={onClick} aria-label="Change hotkey" title="Click to change hotkey">
      {parts.map((p, idx) => {
        const key = `${p}-${idx}`;
        const label = displayHotkeyPart(p, isMac);
        return (
          <span key={key} style={{ display: 'inline-flex', alignItems: 'center' }}>
            <span className="vw-kbd">{label}</span>
            {idx + 1 < parts.length ? <span style={{ padding: '0 var(--space-4)' }}>+</span> : null}
          </span>
        );
      })}
    </button>
  );
}

function StatusCard({ icon, title, subtitle, subtitleColor }: { icon: string; title: string; subtitle?: string; subtitleColor?: string }) {
  return (
    <div className="vw-card" style={{ height: 80, padding: 'var(--space-12)', display: 'grid', gap: 'var(--space-4)' }}>
      <div style={{ fontSize: 16 }}>{icon}</div>
      <div className="vw-type-bodyStrong">{title}</div>
      {subtitle ? (
        <div className="vw-type-caption" style={{ color: subtitleColor ?? 'var(--text-secondary)' }}>
          {subtitle}
        </div>
      ) : (
        <div className="vw-type-caption">&nbsp;</div>
      )}
    </div>
  );
}

function MicHero({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      className="vw-micHero"
      onClick={onClick}
      aria-label="Microphone device"
      title="Click to change device"
    >
      <div className="vw-micHeroIcon">🎤</div>
    </button>
  );
}

export function OverviewPage() {
  const initialPlatformCapabilities = fallbackPlatformCapabilities();
  const initialIsMac = initialPlatformCapabilities.platform === 'macos';
  const initialHotkey = initialIsMac ? 'Alt+Z' : 'Ctrl+Space';

  const [platformCapabilities, setPlatformCapabilities] = useState<PlatformCapabilities>(initialPlatformCapabilities);

  const [toggleHotkey, setToggleHotkey] = useState<string>(initialHotkey);
  const [hotkeyEditorOpen, setHotkeyEditorOpen] = useState(false);
  const [hotkeyDraft, setHotkeyDraft] = useState<string>(initialHotkey);
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);
  const [hotkeySaving, setHotkeySaving] = useState(false);

  const [micPickerOpen, setMicPickerOpen] = useState(false);
  const [micDevices, setMicDevices] = useState<MicrophoneDevice[] | null>(null);
  const [micError, setMicError] = useState<string | null>(null);
  const [selectedMicName, setSelectedMicName] = useState<string | null>(null);
  const [selectedMicId, setSelectedMicId] = useState<string | null>(null);

  // macOS-only: required for auto-paste (synthetic Cmd+V).
  const [accessibilityTrusted, setAccessibilityTrusted] = useState<boolean | null>(null);

  const [modelStatus, setModelStatus] = useState<ModelStatus | null>(null);
  const isMac = platformCapabilities.platform === 'macos';
  const isLinux = platformCapabilities.platform === 'linux';

  useEffect(() => {
    let disposed = false;
    let unlisten: null | (() => void) = null;

    async function start() {
      try {
        const { isTauri, invoke } = await import('@tauri-apps/api/core');
        if (!isTauri()) return;

        const nextPlatformCapabilities = await loadPlatformCapabilities(invoke);
        if (!disposed) {
          setPlatformCapabilities(nextPlatformCapabilities);
        }

        if (nextPlatformCapabilities.platform === 'macos') {
          try {
            const status = await invoke<MacosPermissionsStatus>('get_macos_permissions_status');
            if (!disposed && typeof status?.accessibility_trusted === 'boolean') {
              setAccessibilityTrusted(status.accessibility_trusted);
            }
          } catch {
            // Best-effort: older builds may not expose this command.
          }
        }

        // Hotkey
        try {
          const res = await invoke<HotkeyState>('get_toggle_hotkey');
          if (!disposed && res?.hotkey) {
            setToggleHotkey(res.hotkey);
            setHotkeyDraft(res.hotkey);
          }
        } catch {
          // best-effort
        }

        try {
          const { listen } = await import('@tauri-apps/api/event');
          unlisten = await listen<string>('voicewin://toggle_hotkey_changed', (e) => {
            if (typeof e.payload === 'string' && e.payload.length > 0) {
              setToggleHotkey(e.payload);
              setHotkeyDraft(e.payload);
            }
          });
        } catch {
          // ignore
        }

        // Model status
        try {
          const ms = await invoke<ModelStatus>('get_model_status');
          if (!disposed) {
            setModelStatus(ms);
          }
        } catch {
          // ignore
        }

        try {
          const cfg = await invoke<{
            defaults: { microphone_device?: string | null; microphone_device_id?: string | null };
          }>('get_config');
          const nextName = cfg.defaults.microphone_device?.trim() ?? '';
          const nextId = cfg.defaults.microphone_device_id?.trim() ?? '';
          if (!disposed) {
            setSelectedMicName(nextName.length > 0 ? nextName : null);
            setSelectedMicId(nextId.length > 0 ? nextId : null);
          }
        } catch {
          // ignore
        }
      } catch {
        // not in tauri
      }
    }

    void start();

    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, []);

  useEffect(() => {
    if (!isMac) {
      setAccessibilityTrusted(null);
      return;
    }

    let disposed = false;

    const refreshAccessibilityTrusted = async () => {
      try {
        const { isTauri, invoke } = await import('@tauri-apps/api/core');
        if (!isTauri()) return;

        const status = await invoke<MacosPermissionsStatus>('get_macos_permissions_status');
        if (!disposed && typeof status?.accessibility_trusted === 'boolean') {
          setAccessibilityTrusted(status.accessibility_trusted);
        }
      } catch {
        // Best-effort: older builds may not expose this command.
      }
    };

    const onFocus = () => {
      void refreshAccessibilityTrusted();
    };

    const onVisibility = () => {
      if (typeof document !== 'undefined' && document.visibilityState === 'visible') {
        void refreshAccessibilityTrusted();
      }
    };

    void refreshAccessibilityTrusted();

    window.addEventListener('focus', onFocus);
    document.addEventListener('visibilitychange', onVisibility);

    return () => {
      disposed = true;
      window.removeEventListener('focus', onFocus);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [isMac]);

  useEffect(() => {
    if (!hotkeyEditorOpen) return;

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const next = keydownToHotkey(e);
      if (next.error) {
        setHotkeyError(next.error);
        return;
      }
      if (!next.hotkey) return;

      setHotkeyError(null);
      setHotkeyDraft(next.hotkey);
    };

    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [hotkeyEditorOpen]);

  const providerTitle = 'Local Engine';

  const modelTitle = useMemo(() => {
    if (!modelStatus) return 'Whisper';
    if (modelStatus.preferred_ok) return 'Whisper Base';
    if (modelStatus.bootstrap_ok) return 'Whisper Tiny';
    return 'Whisper';
  }, [modelStatus]);

  const modelSubtitle = useMemo(() => {
    if (!modelStatus) return undefined;
    if (modelStatus.preferred_ok) return 'Loaded';
    if (modelStatus.bootstrap_ok) return 'Loaded';
    return 'Missing';
  }, [modelStatus]);

  const modelSubtitleColor = useMemo(() => {
    if (!modelStatus) return undefined;
    if (modelStatus.preferred_ok || modelStatus.bootstrap_ok) return 'var(--color-success-fg)';
    return 'var(--color-danger-fg)';
  }, [modelStatus]);

  const insertTitle = useMemo(() => {
    if (isMac && accessibilityTrusted === false) return 'Clipboard Only';
    return platformCapabilities.auto_insert ? 'Auto Insert' : 'Clipboard Only';
  }, [accessibilityTrusted, isMac, platformCapabilities.auto_insert]);

  const insertSubtitle = useMemo(() => {
    if (isMac) {
      if (accessibilityTrusted == null) return 'Checking access';
      return accessibilityTrusted ? 'Ready' : 'Enable Accessibility';
    }
    return platformCapabilities.auto_insert ? 'Ready' : 'Manual paste';
  }, [accessibilityTrusted, isMac, platformCapabilities.auto_insert]);

  const insertSubtitleColor = useMemo(() => {
    if (!isMac) return undefined;
    if (accessibilityTrusted == null) return undefined;
    return accessibilityTrusted ? 'var(--color-success-fg)' : 'var(--color-danger-fg)';
  }, [accessibilityTrusted, isMac]);

  return (
    <div style={{ maxWidth: 600, margin: '0 auto', paddingTop: 64 }}>
      <div className="vw-type-display">Ready to Dictate</div>
      <div className="vw-type-body" style={{ marginTop: 'var(--space-8)' }}>
        Press <HotkeyKbd hotkey={toggleHotkey} isMac={isMac} onClick={() => setHotkeyEditorOpen(true)} /> to start.
      </div>

      <div style={{ marginTop: 'var(--space-24)', display: 'flex', justifyContent: 'center' }}>
        <MicHero
          onClick={async () => {
            const nextOpen = !micPickerOpen;
            setMicPickerOpen(nextOpen);
            if (!nextOpen) return;

            try {
              const { invoke } = await import('@tauri-apps/api/core');
              const cfg = await invoke<{
                defaults: { microphone_device?: string | null; microphone_device_id?: string | null };
              }>('get_config');
              const nextName = cfg.defaults.microphone_device?.trim() ?? '';
              const nextId = cfg.defaults.microphone_device_id?.trim() ?? '';
              setSelectedMicName(nextName.length > 0 ? nextName : null);
              setSelectedMicId(nextId.length > 0 ? nextId : null);

              const devices = await invoke<MicrophoneDevice[]>('list_microphone_devices');
              setMicDevices(devices);
              setMicError(null);
            } catch (e) {
              setMicError(String(e));
              setMicDevices([]);
            }
          }}
        />
      </div>

      <div className="vw-type-caption" style={{ marginTop: 'var(--space-8)', textAlign: 'center' }}>
        Selected microphone: {selectedMicName ?? 'System Default'}
      </div>

      {isMac ? (
        <div className="vw-card" style={{ marginTop: 'var(--space-12)', padding: 'var(--space-12)' }}>
          <div className="vw-type-bodyStrong">Auto-Paste (Accessibility)</div>
          <div
            className="vw-type-caption"
            style={{
              marginTop: 'var(--space-8)',
              color:
                accessibilityTrusted === null
                  ? 'var(--text-secondary)'
                  : accessibilityTrusted
                    ? 'var(--color-success-fg)'
                    : 'var(--color-danger-fg)',
            }}
          >
            {accessibilityTrusted === null
              ? 'Checking…'
              : accessibilityTrusted
                ? 'Enabled'
                : 'Not enabled (auto-paste disabled)'}
          </div>

          {accessibilityTrusted === false ? (
            <>
              <div className="vw-type-caption" style={{ marginTop: 'var(--space-8)' }}>
                VoiceWin can still copy your transcript to the clipboard; press Cmd+V to paste.
              </div>

              <div style={{ display: 'flex', gap: 'var(--space-12)', marginTop: 'var(--space-12)', flexWrap: 'wrap' }}>
                <button
                  type="button"
                  className="vw-button vw-button--primary"
                  aria-label="Enable Accessibility"
                  onClick={async () => {
                    try {
                      const { invoke } = await import('@tauri-apps/api/core');
                      // Best-effort: triggers the macOS system prompt.
                      await invoke('prompt_macos_accessibility_permission');
                    } catch {
                      // ignore
                    }

                    try {
                      const { invoke } = await import('@tauri-apps/api/core');
                      await invoke('open_macos_accessibility_settings');
                    } catch {
                      // ignore
                    }

                    // Refresh status after the user potentially toggled the setting.
                    try {
                      const { invoke } = await import('@tauri-apps/api/core');
                      const status = await invoke<MacosPermissionsStatus>('get_macos_permissions_status');
                      if (typeof status?.accessibility_trusted === 'boolean') {
                        setAccessibilityTrusted(status.accessibility_trusted);
                      }
                    } catch {
                      // ignore
                    }
                  }}
                >
                  Enable Accessibility…
                </button>

                <button
                  type="button"
                  className="vw-button vw-button--secondary"
                  onClick={async () => {
                    try {
                      const { invoke } = await import('@tauri-apps/api/core');
                      const status = await invoke<MacosPermissionsStatus>('get_macos_permissions_status');
                      if (typeof status?.accessibility_trusted === 'boolean') {
                        setAccessibilityTrusted(status.accessibility_trusted);
                      }
                    } catch {
                      // ignore
                    }
                  }}
                >
                  Re-check
                </button>
              </div>
            </>
          ) : null}
        </div>
      ) : null}

      {isLinux ? (
        <div className="vw-card" style={{ marginTop: 'var(--space-12)', padding: 'var(--space-12)' }}>
          <div className="vw-type-bodyStrong">Linux Clipboard Insert</div>
          <div className="vw-type-caption" style={{ marginTop: 'var(--space-8)' }}>
            VoiceWin currently copies the final transcript to the clipboard on Linux. Press Ctrl+V to paste.
          </div>
          <div className="vw-type-caption" style={{ marginTop: 'var(--space-8)' }}>
            Automatic profile matching, selected-text capture, window context capture, and visual context capture are not available on Linux yet.
          </div>
        </div>
      ) : null}

      {micPickerOpen ? (
        <div className="vw-card" style={{ marginTop: 'var(--space-12)', padding: 'var(--space-12)' }}>
          <div className="vw-type-bodyStrong">Microphone</div>
          <div className="vw-type-caption" style={{ marginTop: 'var(--space-8)' }}>
            Click a device to select it.
          </div>

          <div className="vw-type-caption" style={{ marginTop: 'var(--space-8)' }}>
            Current: {selectedMicName ?? 'System Default'}
          </div>

          {micError ? (
            <div className="vw-type-caption" style={{ marginTop: 'var(--space-8)', color: 'var(--color-danger-fg)' }}>
              {micError}
            </div>
          ) : null}

          <div style={{ marginTop: 'var(--space-12)', display: 'grid', gap: 'var(--space-8)' }}>
            <button
              type="button"
              className={`vw-button ${selectedMicId === null && selectedMicName === null ? 'vw-button--primary' : 'vw-button--secondary'}`}
              onClick={async () => {
                try {
                  const { invoke } = await import('@tauri-apps/api/core');
                  const cfg = await invoke<{
                    defaults: { microphone_device: string | null; microphone_device_id?: string | null };
                  }>('get_config');
                  cfg.defaults.microphone_device = null;
                  cfg.defaults.microphone_device_id = null;
                  await invoke('set_config', { cfg });
                  setSelectedMicName(null);
                  setSelectedMicId(null);
                  setMicPickerOpen(false);
                } catch (e) {
                  setMicError(String(e));
                }
              }}
            >
              <span>System Default</span>
              {selectedMicId === null && selectedMicName === null ? (
                <span style={{ marginInlineStart: 'var(--space-8)' }} className="vw-type-caption">
                  Selected
                </span>
              ) : null}
            </button>

            {(micDevices ?? []).map((device) => (
              <button
                key={device.id}
                type="button"
                className={`vw-button ${device.is_selected ? 'vw-button--primary' : 'vw-button--secondary'}`}
                onClick={async () => {
                  try {
                    const { invoke } = await import('@tauri-apps/api/core');
                    const cfg = await invoke<{
                      defaults: { microphone_device: string | null; microphone_device_id?: string | null };
                    }>('get_config');
                    cfg.defaults.microphone_device = device.name;
                    cfg.defaults.microphone_device_id = device.id;
                    await invoke('set_config', { cfg });
                    setSelectedMicName(device.name);
                    setSelectedMicId(device.id);
                    setMicPickerOpen(false);
                  } catch (e) {
                    setMicError(String(e));
                  }
                }}
              >
                <span>{device.name}</span>
                {device.is_selected ? (
                  <span style={{ marginInlineStart: 'var(--space-8)' }} className="vw-type-caption">
                    Selected
                  </span>
                ) : null}
                {device.is_default ? (
                  <span style={{ marginInlineStart: 'var(--space-8)' }} className="vw-type-caption">
                    Default
                  </span>
                ) : null}
              </button>
            ))}
          </div>
        </div>
      ) : null}

      {hotkeyEditorOpen ? (
        <div className="vw-card" style={{ marginTop: 'var(--space-12)', padding: 'var(--space-12)' }}>
          <div className="vw-type-bodyStrong">Set Hotkey</div>
          <div className="vw-type-caption" style={{ marginTop: 'var(--space-8)' }}>
            Press the new key combination now (example: Ctrl+Shift+Space).
          </div>

          <div style={{ marginTop: 'var(--space-12)' }}>
            <HotkeyKbd hotkey={hotkeyDraft} isMac={isMac} onClick={() => {}} />
          </div>

          {hotkeyError ? (
            <div className="vw-type-caption" style={{ marginTop: 'var(--space-8)', color: 'var(--color-danger-fg)' }}>
              {hotkeyError}
            </div>
          ) : null}

          <div style={{ display: 'flex', gap: 'var(--space-12)', marginTop: 'var(--space-12)' }}>
            <button
              type="button"
              className="vw-button vw-button--secondary"
              onClick={() => {
                setHotkeyEditorOpen(false);
                setHotkeyDraft(toggleHotkey);
                setHotkeyError(null);
              }}
              disabled={hotkeySaving}
            >
              Cancel
            </button>

            <button
              type="button"
              className="vw-button vw-button--primary"
              onClick={async () => {
                setHotkeySaving(true);
                try {
                  const { invoke } = await import('@tauri-apps/api/core');
                  const res = await invoke<HotkeyState>('set_toggle_hotkey', { hotkey: hotkeyDraft });
                  if (res?.error) {
                    setHotkeyError(res.error);
                  } else {
                    setToggleHotkey(res.hotkey ?? hotkeyDraft);
                    setHotkeyEditorOpen(false);
                    setHotkeyError(null);
                  }
                } catch (e) {
                  setHotkeyError(String(e));
                } finally {
                  setHotkeySaving(false);
                }
              }}
              disabled={hotkeySaving}
            >
              {hotkeySaving ? 'Saving…' : 'Save'}
            </button>
          </div>
        </div>
      ) : null}

      <div
        style={{
          marginTop: 'var(--space-24)',
          display: 'grid',
          gridTemplateColumns: 'repeat(3, 1fr)',
          gap: 'var(--space-12)',
        }}
      >
        <StatusCard icon="💾" title={modelTitle} subtitle={modelSubtitle} subtitleColor={modelSubtitleColor} />
        <StatusCard icon="☁" title={providerTitle} />
        <StatusCard icon="⌨" title={insertTitle} subtitle={insertSubtitle} subtitleColor={insertSubtitleColor} />
      </div>

      <div className="vw-type-caption" style={{ marginTop: 'var(--space-12)' }}>
        Tip: Click the hotkey to customize it.
      </div>
    </div>
  );
}
