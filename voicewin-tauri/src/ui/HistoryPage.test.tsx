import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { HistoryPage } from './HistoryPage';

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}));

describe('HistoryPage delete behavior', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_history') {
        return [
          {
            id: 'entry-1',
            ts_unix_ms: 1,
            app_process_name: 'Code',
            app_exe_path: null,
            app_window_title: null,
            text: '',
            stage: 'error',
            error: 'microphone denied',
          },
        ];
      }
      if (command === 'delete_history_entry_by_id') {
        return true;
      }
      if (command === 'clear_history') {
        return;
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });
  });

  it('deletes rows by stable id when available', async () => {
    const user = userEvent.setup();
    render(<HistoryPage />);

    const row = await screen.findByText('microphone denied');
    const container = row.closest('.vw-historyRow');
    expect(container).not.toBeNull();

    const deleteButton = within(container as HTMLElement).getByRole('button', { name: 'Delete' });
    await user.click(deleteButton);

    expect(invokeMock).toHaveBeenCalledWith('delete_history_entry_by_id', { id: 'entry-1' });
  });
});
