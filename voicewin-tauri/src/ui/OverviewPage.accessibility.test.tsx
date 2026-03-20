import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

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

function forceMacUserAgent() {
  // JSDOM's UA is not macOS by default; our UI logic uses UA sniffing.
  Object.defineProperty(navigator, 'userAgent', {
    value: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 (KHTML, like Gecko)',
    configurable: true,
  });
}

describe('OverviewPage macOS Accessibility permission', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    forceMacUserAgent();

    const cfg = {
      defaults: {
        microphone_device_id: null as string | null,
        microphone_device: null as string | null,
      },
    };

    invokeMock.mockImplementation(async (command: string, args?: unknown) => {
      if (command === 'get_platform_capabilities') {
        return {
          platform: 'macos',
          foreground_app_identity: true,
          clipboard_context: true,
          selected_text_context: true,
          window_context: true,
          screenshot_capture: true,
          foreground_window_capture: false,
          auto_insert: true,
        };
      }
      if (command === 'get_toggle_hotkey') {
        return { hotkey: 'Alt+Z', error: null };
      }
      if (command === 'get_model_status') {
        return {
          bootstrap_ok: true,
          bootstrap_path: 'bootstrap.bin',
          preferred_ok: true,
          preferred_path: 'preferred.bin',
        };
      }
      if (command === 'get_config') {
        return cfg;
      }
      if (command === 'get_macos_permissions_status') {
        return { accessibility_trusted: false };
      }
      if (command === 'prompt_macos_accessibility_permission') {
        return { accessibility_trusted: false };
      }
      if (command === 'open_macos_accessibility_settings') {
        return null;
      }
      if (command === 'set_config') {
        // Not used in this test, but OverviewPage may call it in other flows.
        return (args as { cfg: unknown })?.cfg ?? null;
      }

      throw new Error(`Unexpected invoke command: ${command}`);
    });
  });

  it('shows an Enable Accessibility CTA when permission is missing', async () => {
    const user = userEvent.setup();
    render(<OverviewPage />);

    // The page should surface a persistent indicator (not only after insertion errors).
    const enable = await screen.findByRole('button', { name: /enable accessibility/i });
    expect(enable).toBeInTheDocument();

    await user.click(enable);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('prompt_macos_accessibility_permission', undefined);
      expect(invokeMock).toHaveBeenCalledWith('open_macos_accessibility_settings', undefined);
    });
  });
});
