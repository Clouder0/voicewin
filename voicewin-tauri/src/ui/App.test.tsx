import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from './App';

it('runs a session via injected invoker', async () => {
  const user = userEvent.setup();

  render(
    <App
      invoker={{
        async runSession({ transcript }) {
          return {
            stage: 'done',
            final_text: `OK:${transcript}`,
          };
        },
        async toggleRecording() {
          return { stage: 'recording' };
        },
        async getConfig() {
          throw new Error('not implemented');
        },
        async setConfig() {
          throw new Error('not implemented');
        },
        async getProviderStatus() {
          throw new Error('not implemented');
        },
        async setOpenAiApiKey() {
          throw new Error('not implemented');
        },
        async setElevenLabsApiKey() {
          throw new Error('not implemented');
        },
        async getHistory() {
          return [];
        },
        async clearHistory() {},
        async getModelStatus() {
          throw new Error('not implemented');
        },
        async installPreferredModel() {
          throw new Error('not implemented');
        },
        async cancelRecording() {
          throw new Error('not implemented');
        },
        async pickPreferredModelFile() {
          throw new Error('not implemented');
        },
      }}
    />,
  );

  const input = screen.getByLabelText('transcript');
  await user.clear(input);
  await user.type(input, 'hello');

  await user.click(screen.getByRole('button', { name: 'Run' }));

  expect(await screen.findByText('OK:hello')).toBeInTheDocument();
});
