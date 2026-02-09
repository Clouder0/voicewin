import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { ProfilesPage } from './ProfilesPage';

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}));

function baseConfig() {
  return {
    defaults: {
      enable_enhancement: false,
      prompt_id: null,
      insert_mode: 'Paste',
      stt_provider: 'local',
      stt_model: 'ggml-base.bin',
      language: 'auto',
      llm_base_url: 'https://api.openai.com/v1',
      llm_model: 'gpt-4o-mini',
      microphone_device: null,
      history_enabled: true,
      context: {
        use_clipboard: false,
        use_selected_text: false,
        use_window_context: false,
        use_custom_vocabulary: false,
        use_ocr: false,
      },
    },
    profiles: [
      {
        id: 'p1',
        name: 'Profile One',
        enabled: true,
        matchers: [{ ProcessNameEquals: 'code.exe' }],
        overrides: {},
      },
      {
        id: 'p2',
        name: 'Profile Two',
        enabled: true,
        matchers: [{ ProcessNameEquals: 'slack.exe' }],
        overrides: {},
      },
    ],
    prompts: [],
    llm_api_key_present: false,
  };
}

describe('ProfilesPage refresh behavior', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'set_config') return;
      if (command === 'capture_foreground_app') {
        return {
          process_name: 'code.exe',
          exe_path: 'C:/code.exe',
          window_title: 'Code',
        };
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });
  });

  it('does not refetch config when only selecting another profile', async () => {
    const user = userEvent.setup();
    render(<ProfilesPage />);

    await screen.findByText('Profile One');
    await user.click(screen.getByText('Profile Two'));

    await vi.waitFor(() => {
      const getConfigCalls = invokeMock.mock.calls.filter(([command]) => command === 'get_config').length;
      expect(getConfigCalls).toBe(1);
    });
  });
});
