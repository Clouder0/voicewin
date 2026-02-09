import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from './App';

it('renders the spec shell and default hotkey text', async () => {
  render(<App />);

  expect(screen.getByText('Ready to Dictate')).toBeInTheDocument();
  expect(screen.getByText('Ctrl')).toBeInTheDocument();
  expect(screen.getByText('Space')).toBeInTheDocument();

  // Navigation rail exists.
  expect(screen.getByRole('button', { name: 'Overview' })).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Profiles' })).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Models' })).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'History' })).toBeInTheDocument();
});

it('exposes active navigation with aria-current', async () => {
  const user = userEvent.setup();
  render(<App />);

  const overview = screen.getByRole('button', { name: 'Overview' });
  const history = screen.getByRole('button', { name: 'History' });

  expect(overview).toHaveAttribute('aria-current', 'page');
  expect(history).not.toHaveAttribute('aria-current');

  await user.click(history);
  expect(history).toHaveAttribute('aria-current', 'page');
  expect(overview).not.toHaveAttribute('aria-current');
});
