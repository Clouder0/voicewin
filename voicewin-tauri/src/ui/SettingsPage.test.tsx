import { fireEvent, render, screen, within } from '@testing-library/react';
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
      llm_provider_kind: 'openai_compatible',
      llm_base_url: 'https://api.openai.com/v1',
      llm_model: 'gpt-4o-mini',
      llm_api_kind: 'chat_completions',
      llm_preflight_mode: 'off',
      llm_preflight_delay_ms: 1500,
      screenshot_max_edge_px: 1280,
      llm_reasoning_effort: null,
      microphone_device: null,
      microphone_device_id: null,
      history_enabled: true,
      context: {
        use_clipboard: false,
        use_selected_text: false,
        use_window_context: false,
        use_custom_vocabulary: false,
        visual_context_mode: 'off',
        visual_capture_scope: 'display',
      },
    },
    profiles: [],
    prompts: [
      {
        id: 'prompt-default',
        title: 'Default Cleanup',
        mode: 'Enhancer',
        prompt_text: 'Fix grammar.',
        trigger_words: ['clean up'],
      },
      {
        id: 'prompt-email',
        title: 'Email',
        mode: 'Enhancer',
        prompt_text: 'Turn this into an email.',
        trigger_words: ['email'],
      },
    ],
    llm_api_key_present: false,
  };
}

function baseProviderStatus() {
  return {
    openai_api_key_present: false,
    openai_api_key_error: null,
    gemini_api_key_present: false,
    gemini_api_key_error: null,
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

function windowsPlatformCapabilities() {
  return {
    platform: 'windows',
    foreground_app_identity: true,
    clipboard_context: true,
    selected_text_context: true,
    window_context: true,
    screenshot_capture: true,
    foreground_window_capture: true,
    auto_insert: true,
  };
}

function getSettingRow(title: string): HTMLElement {
  const node = screen.getByText(title);
  const row = node.closest('.vw-settingRow');
  expect(row).not.toBeNull();
  return row as HTMLElement;
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

  it('saves Responses SSE mode, preflight mode, preflight delay, and reasoning effort in config', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') return baseProviderStatus();
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'set_config') return;
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    await screen.findByDisplayValue('Chat Completions (Legacy)');

    const apiModeSelect = within(getSettingRow('API mode')).getByRole('combobox');
    await user.selectOptions(apiModeSelect, 'responses_sse');

    const preflightSelect = within(getSettingRow('Preflight')).getByRole('combobox');
    await user.selectOptions(preflightSelect, 'http_connect');

    const preflightDelayInput = screen.getByRole('spinbutton', { name: 'Preflight delay' });
    fireEvent.change(preflightDelayInput, { target: { value: '2400' } });

    const reasoningSelect = within(getSettingRow('Reasoning effort')).getByRole('combobox');
    await user.selectOptions(reasoningSelect, 'high');

    await user.click(screen.getByRole('button', { name: 'Save Changes' }));

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'set_config',
        expect.objectContaining({
          cfg: expect.objectContaining({
            defaults: expect.objectContaining({
              llm_api_kind: 'responses_sse',
              llm_preflight_mode: 'http_connect',
              llm_preflight_delay_ms: 2400,
              llm_reasoning_effort: 'high',
            }),
          }),
        }),
      );
    });
  });

  it('switches to Gemini and saves the native SSE provider config', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') return baseProviderStatus();
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'set_config') return;
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    const providerSelect = await screen.findByDisplayValue('OpenAI-Compatible');
    await user.selectOptions(providerSelect, 'gemini');

    await user.click(screen.getByRole('button', { name: 'Save Changes' }));

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'set_config',
        expect.objectContaining({
          cfg: expect.objectContaining({
            defaults: expect.objectContaining({
              llm_provider_kind: 'gemini',
              llm_api_kind: 'stream_generate_content_sse',
              llm_base_url: 'https://generativelanguage.googleapis.com/v1beta',
              llm_model: 'gemini-3-flash-preview',
            }),
          }),
        }),
      );
    });
  });

  it('saves the default prompt and context toggles in config', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') return baseProviderStatus();
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'set_config') return;
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    const promptSelect = await screen.findByDisplayValue('Automatic (Default Cleanup)');
    await user.selectOptions(promptSelect, 'prompt-email');

    const clipboardCheckbox = screen.getByRole('checkbox', { name: 'Clipboard context' });
    await user.click(clipboardCheckbox);

    const selectedTextCheckbox = screen.getByRole('checkbox', { name: 'Selected text context' });
    await user.click(selectedTextCheckbox);

    const visualModeSelect = screen.getByRole('combobox', { name: 'Visual context mode' });
    await user.selectOptions(visualModeSelect, 'screenshot');

    const screenshotMaxEdgeInput = screen.getByRole('spinbutton', { name: 'Screenshot max edge' });
    fireEvent.change(screenshotMaxEdgeInput, { target: { value: '640' } });

    await user.click(screen.getByRole('button', { name: 'Save Changes' }));

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'set_config',
        expect.objectContaining({
          cfg: expect.objectContaining({
            defaults: expect.objectContaining({
              prompt_id: 'prompt-email',
              context: expect.objectContaining({
                use_clipboard: true,
                use_selected_text: true,
                visual_context_mode: 'screenshot',
                visual_capture_scope: 'display',
              }),
              screenshot_max_edge_px: 640,
            }),
          }),
        }),
      );
    });
  });

  it('warns when screenshot context is enabled on a text-only API mode', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') return baseProviderStatus();
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'get_platform_capabilities') return windowsPlatformCapabilities();
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    const visualModeSelect = await screen.findByRole('combobox', { name: 'Visual context mode' });
    await user.selectOptions(visualModeSelect, 'screenshot');

    expect(await screen.findByText(/does not support screenshot context/i)).toBeInTheDocument();

    const apiModeSelect = within(getSettingRow('API mode')).getByRole('combobox');
    await user.selectOptions(apiModeSelect, 'responses_sse');

    await vi.waitFor(() => {
      expect(screen.queryByText(/does not support screenshot context/i)).not.toBeInTheDocument();
    });
    expect(
      screen.getByText(/Visual context will attach one screenshot directly to the enhancement request\./i),
    ).toBeInTheDocument();
  });

  it('shows Linux capability warnings for unsupported selected text, window context, and visual capture', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') {
        const cfg = baseConfig();
        cfg.defaults.llm_api_kind = 'responses_sse';
        cfg.defaults.context.use_selected_text = true;
        cfg.defaults.context.use_window_context = true;
        cfg.defaults.context.visual_context_mode = 'screenshot';
        cfg.defaults.context.visual_capture_scope = 'foreground_window';
        return cfg;
      }
      if (command === 'get_provider_status') return baseProviderStatus();
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'get_platform_capabilities') {
        return {
          platform: 'linux',
          foreground_app_identity: false,
          clipboard_context: true,
          selected_text_context: false,
          window_context: false,
          screenshot_capture: false,
          foreground_window_capture: false,
          auto_insert: false,
        };
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    expect(await screen.findByText(/Selected text capture is not available on Linux yet/i)).toBeInTheDocument();
    expect(screen.getByText(/Window context capture is not available on Linux yet/i)).toBeInTheDocument();
    expect(screen.getByText(/Visual context capture is not available on Linux yet/i)).toBeInTheDocument();
    expect(
      screen.queryByText(/Visual context will attach one screenshot directly to the enhancement request\./i),
    ).not.toBeInTheDocument();
  });

  it('runs the lightweight provider probe against the current LLM draft', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string, args?: any) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') {
        return {
          ...baseProviderStatus(),
          openai_api_key_present: true,
        };
      }
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'probe_llm_provider') {
        expect(args).toEqual({
          req: {
            provider_kind: 'openai_compatible',
            api_kind: 'chat_completions',
            base_url: 'https://api.openai.com/v1',
            model: 'gpt-4o-mini',
            reasoning_effort: null,
            probe_kind: 'smoke',
          },
        });
        return {
          probe_kind: 'smoke',
          elapsed_ms: 432,
          first_token_ms: 210,
          input_tokens: 2048,
          provider_kind: 'openai_compatible',
          api_kind: 'chat_completions',
          model: 'gpt-4o-mini',
          expected_output: 'VoiceWin provider probe ok.',
          final_output: 'VoiceWin provider probe ok.',
        };
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    const probeButton = await screen.findByRole('button', { name: 'Run Probe' });
    await user.click(probeButton);

    expect(await screen.findByText(/smoke probe • openai_compatible • chat_completions • gpt-4o-mini • 432 ms total • 210 ms first token • 2048 input tok • Output: VoiceWin provider probe ok\./i)).toBeInTheDocument();
  });

  it('runs a 3-round provider benchmark and summarizes latency plus warnings', async () => {
    const user = userEvent.setup();
    const probeResponses = [
      {
        probe_kind: 'smoke',
        elapsed_ms: 1200,
        first_token_ms: 800,
        input_tokens: 1024,
        cached_input_tokens: 0,
        provider_kind: 'openai_compatible',
        api_kind: 'chat_completions',
        model: 'gpt-4o-mini',
        expected_output: 'VoiceWin provider probe ok.',
        final_output: 'VoiceWin provider probe ok.',
      },
      {
        probe_kind: 'smoke',
        elapsed_ms: 1500,
        first_token_ms: 1000,
        input_tokens: 1536,
        cached_input_tokens: 0,
        provider_kind: 'openai_compatible',
        api_kind: 'chat_completions',
        model: 'gpt-4o-mini',
        expected_output: 'VoiceWin provider probe ok.',
        final_output: 'VoiceWin provider probe ok.',
        warning: 'LLM output looked conversational; VoiceWin stripped assistant framing from the model output.',
      },
      {
        probe_kind: 'smoke',
        elapsed_ms: 1800,
        first_token_ms: 1200,
        input_tokens: 2048,
        cached_input_tokens: 0,
        provider_kind: 'openai_compatible',
        api_kind: 'chat_completions',
        model: 'gpt-4o-mini',
        expected_output: 'VoiceWin provider probe ok.',
        final_output: 'VoiceWin provider probe ok.',
      },
    ];
    let probeCall = 0;
    invokeMock.mockImplementation(async (command: string, args?: any) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') {
        return {
          ...baseProviderStatus(),
          openai_api_key_present: true,
        };
      }
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'probe_llm_provider') {
        expect(args).toEqual({
          req: {
            provider_kind: 'openai_compatible',
            api_kind: 'chat_completions',
            base_url: 'https://api.openai.com/v1',
            model: 'gpt-4o-mini',
            reasoning_effort: null,
            probe_kind: 'smoke',
          },
        });
        const next = probeResponses[probeCall];
        probeCall += 1;
        return next;
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    const benchmarkButton = await screen.findByRole('button', { name: 'Run 3-Round Benchmark' });
    await user.click(benchmarkButton);

    expect(await screen.findByText(/smoke benchmark • 3 rounds • openai_compatible • chat_completions • gpt-4o-mini • total 1200\/1500\/1800 ms min\/avg\/max • first token 800\/1000\/1200 ms min\/avg\/max • input 1024\/1536\/2048 tok min\/avg\/max • cache 0\/0\/0 tok min\/avg\/max • warnings 1\/3 • mismatches 0\/3 • output variants 1 • Last output: VoiceWin provider probe ok\./i)).toBeInTheDocument();
    expect(await screen.findByText(/Sample warning: LLM output looked conversational; VoiceWin stripped assistant framing from the model output\./i)).toBeInTheDocument();
    expect(probeCall).toBe(3);
  });

  it('runs the screenshot provider probe when the api supports attached images', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string, args?: any) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') {
        return {
          ...baseProviderStatus(),
          openai_api_key_present: true,
        };
      }
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'probe_llm_provider') {
        expect(args).toEqual({
          req: {
            provider_kind: 'openai_compatible',
            api_kind: 'responses_sse',
            base_url: 'https://api.openai.com/v1',
            model: 'gpt-4o-mini',
            reasoning_effort: null,
            probe_kind: 'screenshot_product_name',
          },
        });
        return {
          probe_kind: 'screenshot_product_name',
          elapsed_ms: 987,
          first_token_ms: 654,
          input_tokens: 1536,
          provider_kind: 'openai_compatible',
          api_kind: 'responses_sse',
          model: 'gpt-4o-mini',
          expected_output: 'VoiceWin',
          final_output: 'VoiceWin',
          warning: 'LLM output looked conversational; VoiceWin stripped assistant framing from the model output.',
        };
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    const apiModeSelect = await screen.findByDisplayValue('Chat Completions (Legacy)');
    await user.selectOptions(apiModeSelect, 'responses_sse');

    const screenshotProbeButton = await screen.findByRole('button', { name: 'Run Screenshot Probe' });
    await user.click(screenshotProbeButton);

    expect(await screen.findByText(/screenshot probe • openai_compatible • responses_sse • gpt-4o-mini • 987 ms total • 654 ms first token • 1536 input tok • Output: VoiceWin/i)).toBeInTheDocument();
    expect(await screen.findByText(/Warning: LLM output looked conversational; VoiceWin stripped assistant framing from the model output\./i)).toBeInTheDocument();
  });

  it('recommends responses on the validated cc2 gateway when chat completions is selected', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') {
        const cfg = baseConfig();
        cfg.defaults.llm_base_url = 'https://cc2.caaa.tech/v1';
        cfg.defaults.llm_model = 'gpt-4o-mini';
        cfg.defaults.llm_api_kind = 'chat_completions';
        return cfg;
      }
      if (command === 'get_provider_status') {
        return {
          ...baseProviderStatus(),
          openai_api_key_present: true,
        };
      }
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'set_config') return;
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    expect(await screen.findByText(/chat completions has been the unstable path/i)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Use Responses + GPT-5.4' }));
    await user.click(screen.getByRole('button', { name: 'Save Changes' }));

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'set_config',
        expect.objectContaining({
          cfg: expect.objectContaining({
            defaults: expect.objectContaining({
              llm_provider_kind: 'openai_compatible',
              llm_base_url: 'https://cc2.caaa.tech/v1',
              llm_api_kind: 'responses_sse',
              llm_model: 'gpt-5.4',
              llm_preflight_mode: 'off',
              llm_reasoning_effort: null,
            }),
          }),
        }),
      );
    });
  });

  it('recommends the Gemini screenshot stack on the validated cc2 gateway and switches in one click', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') {
        const cfg = baseConfig();
        cfg.defaults.context.visual_context_mode = 'screenshot';
        cfg.defaults.llm_base_url = 'https://cc2.caaa.tech/v1';
        cfg.defaults.llm_api_kind = 'responses_sse';
        cfg.defaults.llm_model = 'gpt-5.4';
        return cfg;
      }
      if (command === 'get_provider_status') {
        return {
          ...baseProviderStatus(),
          openai_api_key_present: true,
          gemini_api_key_present: true,
        };
      }
      if (command === 'get_model_status') return baseModelStatus();
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    expect(await screen.findByText(/Gemini has been the cleaner screenshot-assisted path/i)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Switch to Gemini' }));

    expect(screen.getByDisplayValue('Google Gemini')).toBeInTheDocument();
    expect(screen.getByDisplayValue('streamGenerateContent (HTTP SSE)')).toBeInTheDocument();
    expect(screen.getByDisplayValue('https://cc2.caaa.tech/v1beta')).toBeInTheDocument();
    expect(screen.getByDisplayValue('gemini-3-flash-preview')).toBeInTheDocument();
  });

  it('recommends the compact Gemini screenshot size on the validated cc2 gateway', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') {
        const cfg = baseConfig();
        cfg.defaults.context.visual_context_mode = 'screenshot';
        cfg.defaults.llm_provider_kind = 'gemini';
        cfg.defaults.llm_base_url = 'https://cc2.caaa.tech/v1beta';
        cfg.defaults.llm_api_kind = 'stream_generate_content_sse';
        cfg.defaults.llm_model = 'gemini-3-flash-preview';
        cfg.defaults.screenshot_max_edge_px = 1280;
        return cfg;
      }
      if (command === 'get_provider_status') {
        return {
          ...baseProviderStatus(),
          gemini_api_key_present: true,
        };
      }
      if (command === 'get_model_status') return baseModelStatus();
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    const compactButton = await screen.findByRole('button', { name: 'Use 640px' });
    expect(compactButton).toBeInTheDocument();

    await user.click(compactButton);

    expect(screen.getByDisplayValue('640')).toBeInTheDocument();
  });

  it('offers to reset the model after an unknown-model probe error', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string, args?: any) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') {
        return {
          ...baseProviderStatus(),
          gemini_api_key_present: true,
        };
      }
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'probe_llm_provider') {
        expect(args).toEqual({
          req: {
            provider_kind: 'gemini',
            api_kind: 'stream_generate_content_sse',
            base_url: 'https://generativelanguage.googleapis.com/v1beta',
            model: 'gemini-3.1-flash-preview',
            reasoning_effort: null,
            probe_kind: 'smoke',
          },
        });
        throw new Error('http sse request failed: status=503 body={"message":"未知模型，请检查模型名称是否正确"}');
      }
      if (command === 'set_config') return;
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    const providerSelect = await screen.findByDisplayValue('OpenAI-Compatible');
    await user.selectOptions(providerSelect, 'gemini');

    const modelInput = screen.getByDisplayValue('gemini-3-flash-preview');
    await user.clear(modelInput);
    await user.type(modelInput, 'gemini-3.1-flash-preview');

    await user.click(screen.getByRole('button', { name: 'Run Probe' }));

    expect(await screen.findByText(/provider rejected this model name/i)).toBeInTheDocument();
    expect(screen.getByText(/未知模型/i)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Use Default Model' }));

    expect(screen.getByDisplayValue('gemini-3-flash-preview')).toBeInTheDocument();
    expect(screen.queryByText(/provider rejected this model name/i)).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Save Changes' }));

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'set_config',
        expect.objectContaining({
          cfg: expect.objectContaining({
            defaults: expect.objectContaining({
              llm_provider_kind: 'gemini',
              llm_model: 'gemini-3-flash-preview',
            }),
          }),
        }),
      );
    });
  });

  it('shows the recommended OpenAI stack callout for the legacy default draft', async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') return baseProviderStatus();
      if (command === 'get_model_status') return baseModelStatus();
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    expect(await screen.findByText(/Recommended OpenAI stack:/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Apply Recommended' })).toBeInTheDocument();
  });

  it('applies the recommended OpenAI-compatible stack before saving', async () => {
    const user = userEvent.setup();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_provider_status') return baseProviderStatus();
      if (command === 'get_model_status') return baseModelStatus();
      if (command === 'set_config') return;
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<SettingsPage />);

    await screen.findByDisplayValue('OpenAI-Compatible');
    await user.click(screen.getByRole('button', { name: 'Apply Recommended' }));
    await user.click(screen.getByRole('button', { name: 'Save Changes' }));

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'set_config',
        expect.objectContaining({
          cfg: expect.objectContaining({
            defaults: expect.objectContaining({
              llm_provider_kind: 'openai_compatible',
              llm_api_kind: 'responses_sse',
              llm_base_url: 'https://api.openai.com/v1',
              llm_model: 'gpt-5.4',
              llm_preflight_mode: 'off',
              llm_reasoning_effort: null,
            }),
          }),
        }),
      );
    });
  });
});
