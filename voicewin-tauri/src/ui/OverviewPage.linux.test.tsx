import { render, screen, waitFor } from '@testing-library/react';

import { OverviewPage } from './OverviewPage';

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: async () => () => {},
}));

function forceLinuxUserAgent() {
  Object.defineProperty(navigator, 'userAgent', {
    value: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko)',
    configurable: true,
  });
}

function baseConfig() {
  return {
    defaults: {
      microphone_device_id: null as string | null,
      microphone_device: null as string | null,
    },
  };
}

function baseModelStatus() {
  return {
    bootstrap_ok: true,
    bootstrap_path: 'bootstrap.bin',
    preferred_ok: true,
    preferred_path: 'preferred.bin',
  };
}

describe('OverviewPage Linux capability guidance', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    forceLinuxUserAgent();
  });

  it('shows explicit clipboard-only and capability guidance when the backend reports Linux', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_platform_capabilities') {
        return {
          platform: 'linux',
          foreground_app_identity: false,
          clipboard_context: true,
          selected_text_context: false,
          window_context: false,
          screenshot_capture: false,
          foreground_window_capture: false,
          auto_insert: false,
        };
      }
      if (command === 'get_toggle_hotkey') {
        return { hotkey: 'Ctrl+Space', error: null };
      }
      if (command === 'get_model_status') {
        return baseModelStatus();
      }
      if (command === 'get_config') {
        return baseConfig();
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<OverviewPage />);

    expect(await screen.findByText(/VoiceWin currently copies the final transcript to the clipboard on Linux/i)).toBeInTheDocument();
    expect(screen.getByText(/Press Ctrl\+V to paste\./i)).toBeInTheDocument();
    expect(
      screen.getByText(/Automatic profile matching, selected-text capture, window context capture, and visual context capture are not available on Linux yet\./i),
    ).toBeInTheDocument();
    expect(screen.getByText('Clipboard Only')).toBeInTheDocument();
  });

  it('trusts backend capabilities over a Linux user agent fallback', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_platform_capabilities') {
        return {
          platform: 'windows',
          foreground_app_identity: true,
          clipboard_context: true,
          selected_text_context: true,
          window_context: true,
          screenshot_capture: true,
          foreground_window_capture: true,
          auto_insert: true,
        };
      }
      if (command === 'get_toggle_hotkey') {
        return { hotkey: 'Ctrl+Space', error: null };
      }
      if (command === 'get_model_status') {
        return baseModelStatus();
      }
      if (command === 'get_config') {
        return baseConfig();
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<OverviewPage />);

    await waitFor(() => {
      expect(screen.queryByText('Linux Clipboard Insert')).not.toBeInTheDocument();
    });
    expect(screen.getByText('Auto Insert')).toBeInTheDocument();
    expect(screen.getByText('Ready')).toBeInTheDocument();
  });
});
