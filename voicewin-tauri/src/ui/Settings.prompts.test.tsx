import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Settings } from './Settings';

it('loads config and allows adding a prompt', async () => {
  const user = userEvent.setup();

  let cfg = {
    defaults: {
      enable_enhancement: true,
      prompt_id: null,
      insert_mode: 'Paste',
      stt_provider: 'local',
      stt_model: './models/whisper.bin',
      language: 'auto',
      llm_base_url: 'https://api.openai.com/v1',
      llm_model: 'gpt-4o-mini',
      history_enabled: true,
      context: {
        use_clipboard: true,
        use_selected_text: false,
        use_window_context: true,
        use_custom_vocabulary: true,
        use_ocr: false,
      },
    },
    profiles: [],
    prompts: [],
    llm_api_key_present: false,
  };

  render(
    <Settings
      invoker={{
        async runSession() {
          throw new Error('not needed');
        },
        async toggleRecording() {
          throw new Error('not needed');
        },
        async getProviderStatus() {

          return { openai_api_key_present: false, elevenlabs_api_key_present: false };
        },
        async setOpenAiApiKey() {},
        async setElevenLabsApiKey() {},
        async getConfig() {
          return cfg;
        },
        async setConfig(next) {
          cfg = next;
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

  await user.click(screen.getByRole('button', { name: 'Prompts' }));
  await user.click(screen.getByRole('button', { name: 'Load' }));

  expect(await screen.findByText(/Prompts \(0\)/)).toBeInTheDocument();

  await user.click(screen.getByRole('button', { name: 'Add' }));
  expect(await screen.findByText(/Prompts \(1\)/)).toBeInTheDocument();
});
