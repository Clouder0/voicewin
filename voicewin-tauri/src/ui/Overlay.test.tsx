import { act, render, screen } from '@testing-library/react';

import { Overlay } from './Overlay';

type SessionStage =
  | 'idle'
  | 'recording'
  | 'finalizing'
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

const { invokeMock, statusRef, listeners } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  statusRef: {
    current: {
      stage: 'idle',
      stage_label: 'idle',
      is_recording: false,
      elapsed_ms: null,
      error: null,
      last_text_preview: null,
      last_text_available: false,
    } as SessionStatusPayload,
  },
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: async (event: string, cb: (event: { payload: unknown }) => void) => {
    listeners.set(event, cb);
    return () => listeners.delete(event);
  },
  emit: async () => {},
}));

describe('Overlay actions', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listeners.clear();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'overlay_ready') return;
      if (command === 'get_session_status') return statusRef.current;
      if (command === 'overlay_set_size') return;
      if (command === 'overlay_dismiss') return;
      if (command === 'cancel_recording') return;
      if (command === 'toggle_recording') return;
      throw new Error(`Unexpected invoke command: ${command}`);
    });
  });

  it('shows cancel while inserting', async () => {
    statusRef.current = {
      stage: 'inserting',
      stage_label: 'inserting',
      is_recording: false,
      elapsed_ms: null,
      error: null,
      last_text_preview: null,
      last_text_available: false,
    };

    render(<Overlay />);

    expect(await screen.findByRole('button', { name: 'Cancel' })).toBeInTheDocument();
  });

  it('does not show dismiss when cancel is visible', async () => {
    statusRef.current = {
      stage: 'finalizing',
      stage_label: 'finalizing',
      is_recording: false,
      elapsed_ms: null,
      error: null,
      last_text_preview: null,
      last_text_available: false,
    };

    render(<Overlay />);

    expect(await screen.findByRole('button', { name: 'Cancel' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Dismiss' })).not.toBeInTheDocument();
  });

  it('re-sizes overlay when preview text changes within same stage', async () => {
    statusRef.current = {
      stage: 'recording',
      stage_label: 'recording',
      is_recording: true,
      elapsed_ms: 100,
      error: null,
      last_text_preview: null,
      last_text_available: false,
    };

    render(<Overlay />);

    await screen.findByRole('button', { name: 'Stop' });

    const before = invokeMock.mock.calls.filter(([command]) => command === 'overlay_set_size').length;
    const statusListener = listeners.get('voicewin://session_status');
    expect(statusListener).toBeDefined();

    await act(async () => {
      statusListener?.({
        payload: {
          ...statusRef.current,
          last_text_preview: 'this is a much longer preview that should trigger a fresh fit-content resize call',
        },
      });
    });

    await vi.waitFor(() => {
      const after = invokeMock.mock.calls.filter(([command]) => command === 'overlay_set_size').length;
      expect(after).toBeGreaterThan(before);
    });
  });
});
