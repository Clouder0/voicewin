import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { ModelsPage } from './ModelsPage';

type EventHandler = (event: { payload: unknown }) => void;

const { invokeMock, listeners } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listeners: new Map<string, EventHandler>(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: async (eventName: string, handler: EventHandler) => {
    listeners.set(eventName, handler);
    return () => listeners.delete(eventName);
  },
}));

function bundledModel() {
  return {
    id: 'whisper-tiny-bundled',
    title: 'Tiny (Bundled)',
    recommended: false,
    filename: 'tiny.bin',
    size_bytes: 10 * 1024 * 1024,
    speed_label: 'Fast',
    accuracy_label: 'Starter',
    installed: true,
    active: false,
    downloading: false,
  };
}

function downloadableModel() {
  return {
    id: 'whisper-base',
    title: 'Base',
    recommended: true,
    filename: 'base.bin',
    size_bytes: 150 * 1024 * 1024,
    speed_label: 'Balanced',
    accuracy_label: 'Better',
    installed: false,
    active: false,
    downloading: false,
  };
}

describe('ModelsPage', () => {
  beforeEach(() => {
    listeners.clear();
    invokeMock.mockReset();
  });

  it('shows cloud-provider warning and confirms before switching to local', async () => {
    const user = userEvent.setup();
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);

    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'list_models') return [bundledModel()];
      if (command === 'get_config') return { defaults: { stt_provider: 'elevenlabs' } };
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<ModelsPage />);

    expect(await screen.findByText('Cloud STT Active')).toBeInTheDocument();

    await user.click(await screen.findByRole('button', { name: 'Switch to Local' }));

    expect(confirmSpy).toHaveBeenCalledWith('Cloud STT is active. Switch to Local and use this model?');
    expect(invokeMock).not.toHaveBeenCalledWith('set_active_model', { modelId: 'whisper-tiny-bundled' });

    confirmSpy.mockRestore();
  });

  it('updates visible download progress from Tauri events', async () => {
    const user = userEvent.setup();

    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'list_models') return [downloadableModel()];
      if (command === 'get_config') return { defaults: { stt_provider: 'local' } };
      if (command === 'download_model') return null;
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<ModelsPage />);

    await user.click(await screen.findByRole('button', { name: 'Download' }));

    await act(async () => {
      listeners.get('voicewin://model_download_progress')?.({
        payload: { model_id: 'whisper-base', downloaded_bytes: 50, total_bytes: 100 },
      });
    });

    expect(await screen.findByText('50%')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Downloading…' })).toBeDisabled();
  });

  it('surfaces download failures and clears optimistic progress state', async () => {
    const user = userEvent.setup();

    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'list_models') return [downloadableModel()];
      if (command === 'get_config') return { defaults: { stt_provider: 'local' } };
      if (command === 'download_model') throw new Error('network broke');
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<ModelsPage />);

    await user.click(await screen.findByRole('button', { name: 'Download' }));

    await waitFor(() => {
      expect(screen.getByText('Error: network broke')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Download' })).toBeInTheDocument();
    });
  });
});
