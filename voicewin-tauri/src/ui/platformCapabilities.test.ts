import {
  contextCapabilityWarnings,
  fallbackPlatformCapabilities,
  foregroundAppCapabilityWarning,
  loadPlatformCapabilities,
} from './platformCapabilities';

describe('platformCapabilities helpers', () => {
  it('returns Linux fallback capabilities conservatively', () => {
    expect(fallbackPlatformCapabilities('Mozilla/5.0 (X11; Linux x86_64)')).toEqual({
      platform: 'linux',
      foreground_app_identity: false,
      clipboard_context: true,
      selected_text_context: false,
      window_context: false,
      screenshot_capture: false,
      foreground_window_capture: false,
      auto_insert: false,
    });
  });

  it('builds Linux context warnings for unsupported features', () => {
    const warnings = contextCapabilityWarnings(fallbackPlatformCapabilities('Mozilla/5.0 (X11; Linux x86_64)'), {
      useSelectedText: true,
      useWindowContext: true,
      visualMode: 'screenshot',
      captureScope: 'foreground_window',
    });

    expect(warnings).toEqual([
      'Selected text capture is not available on Linux yet. VoiceWin will continue without selected text.',
      'Window context capture is not available on Linux yet. VoiceWin will continue without window context.',
      'Visual context capture is not available on Linux yet. Screenshot, Auto, and OCR modes will continue without visual context.',
    ]);
  });

  it('reports macOS foreground-window fallback guidance', () => {
    const warnings = contextCapabilityWarnings(fallbackPlatformCapabilities('Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)'), {
      useSelectedText: false,
      useWindowContext: false,
      visualMode: 'ocr',
      captureScope: 'foreground_window',
    });

    expect(warnings).toEqual([
      'Foreground-window visual capture is not available on macOS yet. VoiceWin currently falls back to full-display capture.',
    ]);
  });

  it('reports missing foreground app identity for Linux', () => {
    expect(
      foregroundAppCapabilityWarning(fallbackPlatformCapabilities('Mozilla/5.0 (X11; Linux x86_64)')),
    ).toBe('Automatic profile matching is not available on Linux yet because foreground app identity is not exposed.');
  });

  it('prefers backend platform capabilities when the command is available', async () => {
    const invoke = vi.fn().mockResolvedValue({
      platform: 'windows',
      foreground_app_identity: true,
      clipboard_context: true,
      selected_text_context: true,
      window_context: true,
      screenshot_capture: true,
      foreground_window_capture: true,
      auto_insert: true,
    });

    await expect(loadPlatformCapabilities(invoke, 'Mozilla/5.0 (X11; Linux x86_64)')).resolves.toEqual({
      platform: 'windows',
      foreground_app_identity: true,
      clipboard_context: true,
      selected_text_context: true,
      window_context: true,
      screenshot_capture: true,
      foreground_window_capture: true,
      auto_insert: true,
    });
    expect(invoke).toHaveBeenCalledWith('get_platform_capabilities');
  });

  it('falls back to navigator inference when the backend command is unavailable', async () => {
    const invoke = vi.fn().mockRejectedValue(new Error('missing command'));

    await expect(loadPlatformCapabilities(invoke, 'Mozilla/5.0 (X11; Linux x86_64)')).resolves.toEqual({
      platform: 'linux',
      foreground_app_identity: false,
      clipboard_context: true,
      selected_text_context: false,
      window_context: false,
      screenshot_capture: false,
      foreground_window_capture: false,
      auto_insert: false,
    });
  });
});
