import { render, screen, waitFor, within } from '@testing-library/react';
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

function baseConfig() {
  return {
    defaults: {
      microphone_device_id: 'mic-2',
      microphone_device: 'USB Mic',
    },
  };
}

function baseDevices() {
  return [
    { id: 'mic-1', name: 'Built-in Microphone', is_default: true, is_selected: false },
    { id: 'mic-2', name: 'USB Mic', is_default: false, is_selected: true },
  ];
}

describe('OverviewPage microphone picker', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    let cfg = baseConfig();

    invokeMock.mockImplementation(async (command: string, args?: unknown) => {
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
      if (command === 'list_microphone_devices') {
        return baseDevices();
      }
      if (command === 'set_config') {
        const next = (args as { cfg: typeof cfg }).cfg;
        cfg = next;
        return null;
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });
  });

  it('shows the selected microphone in the overview and picker', async () => {
    const user = userEvent.setup();
    render(<OverviewPage />);

    expect(await screen.findByText(/Selected microphone:\s*USB Mic/i)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Microphone device' }));

    const selectedButton = await screen.findByRole('button', { name: /USB Mic/i });
    expect(within(selectedButton).getByText('Selected')).toBeInTheDocument();
  });

  it('saves both device id and name when selecting a microphone', async () => {
    const user = userEvent.setup();
    render(<OverviewPage />);

    await user.click(await screen.findByRole('button', { name: 'Microphone device' }));
    await user.click(await screen.findByRole('button', { name: /Built-in Microphone/i }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'set_config',
        expect.objectContaining({
          cfg: expect.objectContaining({
            defaults: expect.objectContaining({
              microphone_device_id: 'mic-1',
              microphone_device: 'Built-in Microphone',
            }),
          }),
        }),
      );
    });
  });
});
