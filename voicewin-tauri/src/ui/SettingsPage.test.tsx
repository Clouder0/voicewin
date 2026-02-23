import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { SettingsPage } from './SettingsPage';

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
      stt_provider: 'elevenlabs',
      stt_model: 'scribe_v2',
      language: 'auto',
      llm_base_url: 'https://api.openai.com/v1',
      llm_model: 'gpt-4o-mini',
      microphone_device: null,
      microphone_device_id: null,
      history_enabled: true,
      context: {
        use_clipboard: false,
        use_selected_text: false,
        use_window_context: false,
        use_custom_vocabulary: false,
        use_ocr: false,
      },
    },
    profiles: [],
    prompts: [],
    llm_api_key_present: false,
  };
}

function baseProviderStatus() {
  return {
    openai_api_key_present: false,
    openai_api_key_error: null,
    elevenlabs_api_key_present: false,
    elevenlabs_api_key_error: null,
  };
}

function baseModelStatus() {
  return {
    bootstrap_ok: true,
    bootstrap_path: 'C:/voicewin/models/bootstrap.bin',
    preferred_ok: false,
    preferred_path: 'C:/voicewin/models/ggml-base.bin',
  };
}

describe('SettingsPage ElevenLabs key save', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') return baseProviderStatus();
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'set_elevenlabs_api_key') {
        // Simulate backend failing to confirm key presence after write.
        return {
          ...baseProviderStatus(),
          elevenlabs_api_key_present: false,
        };
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });
  });

  it('shows an error instead of Saved when key is not confirmed present', async () => {
    const user = userEvent.setup();
    render(<SettingsPage />);

    const keyInput = await screen.findByPlaceholderText('Paste xi-api-key…');
    await user.type(keyInput, 'xi_test_123');

    const controls = keyInput.parentElement;
    expect(controls).not.toBeNull();
    const saveButton = within(controls as HTMLElement).getByRole('button', { name: 'Save' });
    await user.click(saveButton);

    expect(await screen.findByText('Saved key but it is still not present in secret storage.')).toBeInTheDocument();
    expect(screen.queryByText(/^Saved$/)).not.toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith('set_elevenlabs_api_key', { apiKey: 'xi_test_123' });
  });
});

describe('SettingsPage loading and local model status', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('keeps settings usable when model status call fails', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') return baseProviderStatus();
      if (command === 'get_model_status') throw new Error('model status unavailable');
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    expect(await screen.findByDisplayValue('ElevenLabs')).toBeInTheDocument();
  });

  it('shows missing local model status when no local model is valid', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') return baseProviderStatus();
      if (command === 'get_model_status') {
        return {
          bootstrap_ok: false,
          bootstrap_path: 'C:/voicewin/models/bootstrap.bin',
          preferred_ok: false,
          preferred_path: 'C:/voicewin/models/ggml-base.bin',
        };
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    const providerSelect = await screen.findByDisplayValue('ElevenLabs');
    await user.selectOptions(providerSelect, 'local');

    expect(await screen.findByText('Missing')).toBeInTheDocument();
  });

  it('renders OpenAI key controls inside responsive wrapper classes', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') return baseProviderStatus();
      if (command === 'get_model_status') return baseModelStatus();
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    const keyInput = await screen.findByPlaceholderText('Paste key…');
    const controls = keyInput.parentElement;
    expect(controls).not.toBeNull();
    expect(controls).toHaveClass('vw-settingControls');

    const rowRight = controls?.parentElement;
    expect(rowRight).not.toBeNull();
    expect(rowRight).toHaveClass('vw-settingRowRight');
  });
});
