import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { OverviewPage } from './OverviewPage';

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => false,
  invoke: vi.fn(),
}));

describe('OverviewPage hotkey capture', () => {
  it('uses event code so Alt+KeyS is captured as Alt+S', async () => {
    const user = userEvent.setup();
    render(<OverviewPage />);

    const [openButton] = screen.getAllByRole('button', { name: 'Change hotkey' });
    await user.click(openButton);

    await waitFor(() => {
      expect(screen.getByText('Set Hotkey')).toBeInTheDocument();
    });

    fireEvent.keyDown(window, {
      key: 'ß',
      code: 'KeyS',
      altKey: true,
      bubbles: true,
      cancelable: true,
    });

    const buttons = screen.getAllByRole('button', { name: 'Change hotkey' });
    const editorButton = buttons[buttons.length - 1];

    expect(editorButton).toHaveTextContent('Alt');
    expect(editorButton).toHaveTextContent('S');
  });
});
