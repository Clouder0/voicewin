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

describe('ProfilesPage refresh behavior', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'get_platform_capabilities') return windowsPlatformCapabilities();
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

  it('duplicates the selected profile with a new name and id', async () => {
    const user = userEvent.setup();
    render(<ProfilesPage />);

    await screen.findByText('Profile One');
    await user.click(screen.getByRole('button', { name: 'Duplicate' }));

    await screen.findByDisplayValue('Profile One Copy');

    await vi.waitFor(() => {
      const setConfigCalls = invokeMock.mock.calls.filter(([command]) => command === 'set_config');
      expect(setConfigCalls.length).toBeGreaterThanOrEqual(1);
      const lastArgs = setConfigCalls.at(-1)?.[1] as {
        cfg: { profiles: Array<{ id: string; name: string; matchers: Array<Record<string, string>> }> };
      };

      expect(lastArgs.cfg.profiles).toHaveLength(3);
      expect(lastArgs.cfg.profiles[2].name).toBe('Profile One Copy');
      expect(lastArgs.cfg.profiles[2].id).not.toBe('p1');
      expect(lastArgs.cfg.profiles[2].matchers).toEqual([{ ProcessNameEquals: 'code.exe' }]);
    });
  });

  it('saves prompt, provider, and context overrides for a profile', async () => {
    const user = userEvent.setup();
    render(<ProfilesPage />);

    await screen.findByText('Profile One');

    const promptSelect = screen.getByRole('combobox', { name: 'Profile prompt override' });
    await user.selectOptions(promptSelect, 'prompt-email');

    const providerSelect = screen.getByRole('combobox', { name: 'Profile provider override' });
    await user.selectOptions(providerSelect, 'gemini');

    const contextSelect = screen.getByRole('combobox', { name: 'Profile Selected text' });
    await user.selectOptions(contextSelect, 'on');

    const visualModeSelect = screen.getByRole('combobox', { name: 'Profile Visual mode' });
    await user.selectOptions(visualModeSelect, 'screenshot');

    const captureTargetSelect = screen.getByRole('combobox', { name: 'Profile Visual capture target' });
    await user.selectOptions(captureTargetSelect, 'foreground_window');

    await vi.waitFor(() => {
      const setConfigCalls = invokeMock.mock.calls.filter(([command]) => command === 'set_config');
      expect(setConfigCalls.length).toBeGreaterThanOrEqual(3);
      const lastArgs = setConfigCalls.at(-1)?.[1] as { cfg: { profiles: Array<{ overrides: Record<string, unknown> }> } };
      expect(lastArgs.cfg.profiles[0].overrides).toEqual(
        expect.objectContaining({
          prompt_id: 'prompt-email',
          llm_provider_kind: 'gemini',
          llm_api_kind: 'stream_generate_content_sse',
          llm_base_url: 'https://generativelanguage.googleapis.com/v1beta',
          llm_model: 'gemini-3-flash-preview',
          context: expect.objectContaining({
            use_selected_text: true,
            visual_context_mode: 'screenshot',
            visual_capture_scope: 'foreground_window',
          }),
        }),
      );
    });
  });

  it('shows a profile warning when screenshot context resolves to a text-only API', async () => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') {
        const cfg = baseConfig();
        cfg.defaults.context.visual_context_mode = 'screenshot';
        return cfg;
      }
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

    render(<ProfilesPage />);

    expect(await screen.findByText(/does not support screenshot context/i)).toBeInTheDocument();
  });

  it('shows Linux profile capability warnings and disables use-foreground capture', async () => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') {
        const cfg = baseConfig();
        cfg.defaults.llm_api_kind = 'responses_sse';
        cfg.defaults.context.use_selected_text = true;
        cfg.defaults.context.visual_context_mode = 'screenshot';
        cfg.defaults.context.visual_capture_scope = 'foreground_window';
        return cfg;
      }
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
      if (command === 'set_config') return;
      if (command === 'capture_foreground_app') {
        return {
          process_name: null,
          exe_path: null,
          window_title: null,
        };
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<ProfilesPage />);

    expect(await screen.findByText(/Automatic profile matching is not available on Linux yet/i)).toBeInTheDocument();
    expect(screen.getByText(/Selected text capture is not available on Linux yet/i)).toBeInTheDocument();
    expect(screen.getByText(/Visual context capture is not available on Linux yet/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Use foreground for matcher 1' })).toBeDisabled();
  });

  it('supports multiple matcher rows and matcher types for one profile', async () => {
    const user = userEvent.setup();
    render(<ProfilesPage />);

    await screen.findByText('Profile One');

    await user.click(screen.getByRole('button', { name: '+ Window Title' }));
    const matcherValue2 = screen.getByRole('textbox', { name: 'Profile matcher value 2' });
    await user.type(matcherValue2, 'Inbox');
    await user.tab();

    await user.click(screen.getByRole('button', { name: '+ Executable' }));
    const matcherValue3 = screen.getByRole('textbox', { name: 'Profile matcher value 3' });
    await user.type(matcherValue3, 'C:/Apps/Code/code.exe');
    await user.tab();

    await vi.waitFor(() => {
      const setConfigCalls = invokeMock.mock.calls.filter(([command]) => command === 'set_config');
      expect(setConfigCalls.length).toBeGreaterThanOrEqual(4);
      const lastArgs = setConfigCalls.at(-1)?.[1] as { cfg: { profiles: Array<{ matchers: Array<Record<string, string>> }> } };
      expect(lastArgs.cfg.profiles[0].matchers).toEqual([
        { ProcessNameEquals: 'code.exe' },
        { WindowTitleContains: 'Inbox' },
        { ExePathEquals: 'C:/Apps/Code/code.exe' },
      ]);
    });
  });

  it('uses foreground capture for a matcher row', async () => {
    const user = userEvent.setup();
    render(<ProfilesPage />);

    await screen.findByText('Profile One');

    await user.click(screen.getByRole('button', { name: '+ Executable' }));
    await user.click(screen.getByRole('button', { name: 'Use foreground for matcher 2' }));

    await vi.waitFor(() => {
      const setConfigCalls = invokeMock.mock.calls.filter(([command]) => command === 'set_config');
      expect(setConfigCalls.length).toBeGreaterThanOrEqual(2);
      const lastArgs = setConfigCalls.at(-1)?.[1] as { cfg: { profiles: Array<{ matchers: Array<Record<string, string>> }> } };
      expect(lastArgs.cfg.profiles[0].matchers).toEqual([
        { ProcessNameEquals: 'code.exe' },
        { ExePathEquals: 'C:/code.exe' },
      ]);
    });
  });

  it('shows and applies the recommended OpenAI override for a legacy effective profile stack', async () => {
    const user = userEvent.setup();
    render(<ProfilesPage />);

    expect(await screen.findByText(/legacy OpenAI stack/i)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Apply Recommended Override' }));

    await vi.waitFor(() => {
      const setConfigCalls = invokeMock.mock.calls.filter(([command]) => command === 'set_config');
      expect(setConfigCalls.length).toBeGreaterThanOrEqual(1);
      const lastArgs = setConfigCalls.at(-1)?.[1] as { cfg: { profiles: Array<{ overrides: Record<string, unknown> }> } };
      expect(lastArgs.cfg.profiles[0].overrides).toEqual(
        expect.objectContaining({
          llm_api_kind: 'responses_sse',
          llm_base_url: 'https://api.openai.com/v1',
          llm_model: 'gpt-5.4',
          llm_preflight_mode: 'off',
        }),
      );
    });
  });

  it('recommends responses overrides for cc2 openai-compatible chat completions', async () => {
    const user = userEvent.setup();
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') {
        const cfg = baseConfig();
        cfg.defaults.llm_base_url = 'https://cc2.caaa.tech/v1';
        cfg.defaults.llm_api_kind = 'chat_completions';
        cfg.defaults.llm_model = 'gpt-4o-mini';
        return cfg;
      }
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

    render(<ProfilesPage />);

    expect(await screen.findByText(/chat completions, which has been failing live validation/i)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Use Responses Override' }));

    await vi.waitFor(() => {
      const setConfigCalls = invokeMock.mock.calls.filter(([command]) => command === 'set_config');
      expect(setConfigCalls.length).toBeGreaterThanOrEqual(1);
      const lastArgs = setConfigCalls.at(-1)?.[1] as { cfg: { profiles: Array<{ overrides: Record<string, unknown> }> } };
        expect(lastArgs.cfg.profiles[0].overrides).toEqual(
          expect.objectContaining({
            llm_provider_kind: 'openai_compatible',
            llm_api_kind: 'responses_sse',
            llm_base_url: 'https://cc2.caaa.tech/v1',
            llm_model: 'gpt-5.4',
            llm_preflight_mode: 'off',
          }),
        );
    });
  });
});
