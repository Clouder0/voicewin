import { render, screen } from '@testing-library/react';
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
