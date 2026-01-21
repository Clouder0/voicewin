import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Settings } from './Settings';

it('saves keys via invoker and refreshes status', async () => {
  const user = userEvent.setup();

  let openaiSet = false;
  let elevenSet = false;

  render(
    <Settings
      invoker={{
        async runSession() {
          throw new Error('not needed');
        },
        async toggleRecording() {
          throw new Error('not needed');
        },
        async getConfig() {
          throw new Error('not needed');
        },
        async setConfig() {
          throw new Error('not needed');
        },
        async getProviderStatus() {
          return {
            openai_api_key_present: openaiSet,
            elevenlabs_api_key_present: elevenSet,
          };
        },
        async setOpenAiApiKey(apiKey) {
          openaiSet = apiKey.length > 0;
        },
        async setElevenLabsApiKey(apiKey) {
          elevenSet = apiKey.length > 0;
        },
        async getHistory() {
          return [];
        },
        async clearHistory() {},
        async getModelStatus() {
          throw new Error('not needed');
        },
        async installPreferredModel() {
          throw new Error('not needed');
        },
        async cancelRecording() {
          throw new Error('not needed');
        },
        async pickPreferredModelFile() {
          return null;
        },
      }}
    />,
  );

  await user.click(screen.getByRole('button', { name: 'Providers' }));

  await user.type(screen.getByLabelText('openai-api-key'), 'x');
  await user.click(screen.getAllByRole('button', { name: 'Save' })[0]);

  await user.click(screen.getByRole('button', { name: 'Refresh' }));
  expect(await screen.findByText(/OpenAI key: set/)).toBeInTheDocument();

  await user.type(screen.getByLabelText('elevenlabs-api-key'), 'y');
  await user.click(screen.getAllByRole('button', { name: 'Save' })[1]);

  await user.click(screen.getByRole('button', { name: 'Refresh' }));
  expect(await screen.findByText(/ElevenLabs key: set/)).toBeInTheDocument();
});
