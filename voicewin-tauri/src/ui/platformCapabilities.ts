import type {
  PlatformCapabilities,
  PlatformName,
  VisualCaptureScope,
  VisualContextMode,
} from '../lib/types';

type InvokeLike = <T>(command: string) => Promise<T>;

export function platformLabel(platform: PlatformName): string {
  switch (platform) {
    case 'windows':
      return 'Windows';
    case 'macos':
      return 'macOS';
    case 'linux':
      return 'Linux';
    default:
      return 'this platform';
  }
}

export function detectPlatformFromNavigator(userAgent?: string): PlatformName {
  const ua = userAgent ?? (typeof navigator !== 'undefined' ? navigator.userAgent : '');
  if (/Mac/i.test(ua)) return 'macos';
  if (/Windows/i.test(ua)) return 'windows';
  if (/Linux/i.test(ua) && !/Android/i.test(ua)) return 'linux';
  return 'unknown';
}

export function fallbackPlatformCapabilities(userAgent?: string): PlatformCapabilities {
  const platform = detectPlatformFromNavigator(userAgent);
  switch (platform) {
    case 'windows':
      return {
        platform,
        foreground_app_identity: true,
        clipboard_context: true,
        selected_text_context: true,
        window_context: true,
        screenshot_capture: true,
        foreground_window_capture: true,
        auto_insert: true,
      };
    case 'macos':
      return {
        platform,
        foreground_app_identity: true,
        clipboard_context: true,
        selected_text_context: true,
        window_context: true,
        screenshot_capture: true,
        foreground_window_capture: false,
        auto_insert: true,
      };
    case 'linux':
      return {
        platform,
        foreground_app_identity: false,
        clipboard_context: true,
        selected_text_context: false,
        window_context: false,
        screenshot_capture: false,
        foreground_window_capture: false,
        auto_insert: false,
      };
    default:
      return {
        platform,
        foreground_app_identity: false,
        clipboard_context: false,
        selected_text_context: false,
        window_context: false,
        screenshot_capture: false,
        foreground_window_capture: false,
        auto_insert: false,
      };
  }
}

export async function loadPlatformCapabilities(
  invoke: InvokeLike,
  userAgent?: string,
): Promise<PlatformCapabilities> {
  try {
    return await invoke<PlatformCapabilities>('get_platform_capabilities');
  } catch {
    return fallbackPlatformCapabilities(userAgent);
  }
}

export function contextCapabilityWarnings(
  capabilities: PlatformCapabilities,
  options: {
    useSelectedText: boolean;
    useWindowContext: boolean;
    visualMode: VisualContextMode;
    captureScope: VisualCaptureScope;
  },
): string[] {
  const warnings: string[] = [];
  const platform = platformLabel(capabilities.platform);

  if (options.useSelectedText && !capabilities.selected_text_context) {
    warnings.push(`Selected text capture is not available on ${platform} yet. VoiceWin will continue without selected text.`);
  }

  if (options.useWindowContext && !capabilities.window_context) {
    warnings.push(`Window context capture is not available on ${platform} yet. VoiceWin will continue without window context.`);
  }

  if (options.visualMode !== 'off' && !capabilities.screenshot_capture) {
    warnings.push(`Visual context capture is not available on ${platform} yet. Screenshot, Auto, and OCR modes will continue without visual context.`);
  } else if (
    options.visualMode !== 'off' &&
    options.captureScope === 'foreground_window' &&
    !capabilities.foreground_window_capture
  ) {
    if (capabilities.platform === 'macos') {
      warnings.push('Foreground-window visual capture is not available on macOS yet. VoiceWin currently falls back to full-display capture.');
    } else {
      warnings.push(`Foreground-window visual capture is not available on ${platform} yet.`);
    }
  }

  return warnings;
}

export function foregroundAppCapabilityWarning(capabilities: PlatformCapabilities): string | null {
  if (capabilities.foreground_app_identity) return null;
  return `Automatic profile matching is not available on ${platformLabel(capabilities.platform)} yet because foreground app identity is not exposed.`;
}
