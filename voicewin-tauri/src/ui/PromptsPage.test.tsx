import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { PromptsPage } from './PromptsPage';

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
      enable_enhancement: true,
      prompt_id: null,
      insert_mode: 'Paste',
      stt_provider: 'local',
      stt_model: 'ggml-base.bin',
      language: 'auto',
      llm_provider_kind: 'openai_compatible',
      llm_base_url: 'https://api.openai.com/v1',
      llm_model: 'gpt-5.4',
      llm_api_kind: 'responses_sse',
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
        use_window_context: true,
        use_custom_vocabulary: false,
        visual_context_mode: 'off',
        visual_capture_scope: 'display',
      },
    },
    profiles: [
      {
        id: 'profile-mail',
        name: 'Mail',
        enabled: true,
        matchers: [{ ProcessNameEquals: 'mail.exe' }],
        overrides: {},
      },
      {
        id: 'profile-disabled',
        name: 'Disabled',
        enabled: false,
        matchers: [{ ProcessNameEquals: 'disabled.exe' }],
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
        prompt_text: 'Write an email.',
        trigger_words: ['email'],
      },
    ],
    llm_api_key_present: true,
  };
}

describe('PromptsPage', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string, args?: any) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'set_config') return;
      if (command === 'preview_prompt') {
        const forcedProfileId = args?.req?.forced_profile_id ?? null;
        const forceDefaults = Boolean(args?.req?.force_defaults);
        return {
          elapsed_ms: 842,
          first_token_ms: 211,
          input_tokens: 2048,
          visual_context_runtime: {
            mode: 'auto',
            capture_scope: 'foreground_window',
            capture_actual_scope: 'display',
            dispatch: 'ocr',
            screenshot_capture_elapsed_ms: 17,
            capture_fallback_reason: 'foreground_window_not_implemented',
            screen_ocr_source: 'prepared',
            screen_ocr_elapsed_ms: 64,
            screen_ocr_first_token_ms: 50,
            screen_ocr_text_chars: 8,
          },
          app_process_name: forcedProfileId === 'profile-mail' ? 'mail.exe' : 'voicewin.exe',
          app_window_title: null,
          matched_profile_name: forceDefaults ? null : forcedProfileId === 'profile-mail' ? 'Mail' : null,
          provider_kind: 'openai_compatible',
          api_kind: 'responses_sse',
          model: 'gpt-5.4',
          system_message: '<SYSTEM>\nFix grammar.\n</SYSTEM>',
          user_message: '<TRANSCRIPT>\nhello voicewin world\n</TRANSCRIPT>',
          raw_output: 'Hello, Voicewin world.',
          final_output: 'Hello, Voicewin world.',
          warning:
            forcedProfileId === 'profile-mail'
              ? 'LLM output looked conversational; VoiceWin stripped assistant framing from the model output.'
              : null,
        };
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });
  });

  it('saves prompt edits and default prompt selection', async () => {
    const user = userEvent.setup();
    render(<PromptsPage />);

    await screen.findByText('Default Cleanup');

    await user.click(screen.getByText('Email'));
    const titleInput = screen.getByDisplayValue('Email');
    await user.clear(titleInput);
    await user.type(titleInput, 'Email Reply');

    await user.click(screen.getByRole('button', { name: 'Set As Default' }));
    await user.click(screen.getByRole('button', { name: 'Save Changes' }));

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'set_config',
        expect.objectContaining({
          cfg: expect.objectContaining({
            defaults: expect.objectContaining({
              prompt_id: 'prompt-email',
            }),
            prompts: expect.arrayContaining([
              expect.objectContaining({
                id: 'prompt-email',
                title: 'Email Reply',
              }),
            ]),
          }),
        }),
      );
    });
  });

  it('runs preview with the selected prompt draft', async () => {
    const user = userEvent.setup();
    render(<PromptsPage />);

    await screen.findByText('Default Cleanup');

    await user.click(screen.getByText('Email'));
    const instructions = screen.getByRole('textbox', { name: 'Prompt instructions' });
    const transcript = screen.getByRole('textbox', { name: 'Sample transcript' });

    await user.clear(instructions);
    await user.type(instructions, 'Draft a concise email reply.');
    await user.clear(transcript);
    await user.type(transcript, 'please email the customer about the shipment delay');
    await user.click(screen.getByRole('button', { name: 'Run Preview' }));

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenLastCalledWith('preview_prompt', {
        req: expect.objectContaining({
          force_defaults: false,
          forced_profile_id: null,
          prompt: expect.objectContaining({
            id: 'prompt-email',
            title: 'Email',
            prompt_text: 'Draft a concise email reply.',
          }),
          transcript: 'please email the customer about the shipment delay',
        }),
      });
    });

    expect(await screen.findAllByDisplayValue('Hello, Voicewin world.')).toHaveLength(2);
    expect(
      screen.getByText('openai_compatible • responses_sse • gpt-5.4 • 842 ms total • 211 ms first token • 2048 input tok'),
    ).toBeInTheDocument();
    expect(screen.getByText('voicewin.exe • Using defaults')).toBeInTheDocument();
    expect(
      screen.getByText(
        'Visual auto -> ocr • capture foreground-window • captured display • Capture 17 ms • fallback foreground_window_not_implemented • OCR prepared • OCR 64 ms • OCR first token 50 ms • OCR text 8 chars',
      ),
    ).toBeInTheDocument();
  });

  it('passes explicit profile preview scope through to the backend', async () => {
    const user = userEvent.setup();
    render(<PromptsPage />);

    await screen.findByText('Default Cleanup');
    expect(screen.queryByRole('option', { name: 'Profile: Disabled' })).not.toBeInTheDocument();

    await user.selectOptions(screen.getByRole('combobox', { name: 'Preview scope' }), 'profile:profile-mail');
    await user.click(screen.getByRole('button', { name: 'Run Preview' }));

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('preview_prompt', {
        req: expect.objectContaining({
          forced_profile_id: 'profile-mail',
          force_defaults: false,
        }),
      });
    });

    expect(await screen.findByText('mail.exe • Profile Mail')).toBeInTheDocument();
    expect(
      await screen.findByText(
        'Warning: LLM output looked conversational; VoiceWin stripped assistant framing from the model output.',
      ),
    ).toBeInTheDocument();

    await user.selectOptions(screen.getByRole('combobox', { name: 'Preview scope' }), 'defaults');
    await vi.waitFor(() => {
      expect(screen.queryByText('mail.exe • Profile Mail')).not.toBeInTheDocument();
    });
  });

  it('passes sample context overrides through to preview', async () => {
    const user = userEvent.setup();
    render(<PromptsPage />);

    await screen.findByText('Default Cleanup');

    await user.type(screen.getByRole('textbox', { name: 'Sample selected text' }), 'Current paragraph');
    await user.type(screen.getByRole('textbox', { name: 'Sample clipboard text' }), 'Clipboard note');
    await user.type(screen.getByRole('textbox', { name: 'Sample window context' }), 'Application: Mail');
    await user.click(screen.getByRole('button', { name: 'Run Preview' }));

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('preview_prompt', {
        req: expect.objectContaining({
          context_override: expect.objectContaining({
            clipboard: 'Clipboard note',
            selected_text: 'Current paragraph',
            window_context: 'Application: Mail',
          }),
        }),
      });
    });
  });

  it('passes a sample screenshot image through to preview', async () => {
    const user = userEvent.setup();
    render(<PromptsPage />);

    await screen.findByText('Default Cleanup');

    const file = new File(['voicewin'], 'sample.png', { type: 'image/png' });
    await user.upload(screen.getByLabelText('Sample screenshot image'), file);
    await screen.findByAltText('Sample screenshot preview');
    await user.click(screen.getByRole('button', { name: 'Run Preview' }));

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('preview_prompt', {
        req: expect.objectContaining({
          context_override: expect.objectContaining({
            screenshot_data_url: expect.stringMatching(/^data:image\/png;base64,/),
          }),
        }),
      });
    });
  });

  it('runs a 3-round preview benchmark and summarizes latency plus output stability', async () => {
    const user = userEvent.setup();
    const previewResponses = [
      {
        elapsed_ms: 1200,
        first_token_ms: 700,
        input_tokens: 1024,
        cached_input_tokens: 0,
        visual_context_runtime: {
          mode: 'auto',
          capture_scope: 'foreground_window',
          capture_actual_scope: 'foreground_window',
          dispatch: 'ocr',
          screenshot_capture_elapsed_ms: 19,
          screen_ocr_source: 'inline',
          screen_ocr_elapsed_ms: 40,
          screen_ocr_first_token_ms: 30,
          screen_ocr_text_chars: 8,
        },
        app_process_name: 'voicewin.exe',
        app_window_title: null,
        matched_profile_name: null,
        provider_kind: 'openai_compatible',
        api_kind: 'responses_sse',
        model: 'gpt-5.4',
        system_message: '<SYSTEM>\nFix grammar.\n</SYSTEM>',
        user_message: '<TRANSCRIPT>\nhello voicewin world\n</TRANSCRIPT>',
        raw_output: 'Hello, Voicewin world.',
        final_output: 'Hello, Voicewin world.',
        warning: null,
      },
      {
        elapsed_ms: 1800,
        first_token_ms: 1100,
        input_tokens: 2048,
        cached_input_tokens: 0,
        visual_context_runtime: {
          mode: 'auto',
          capture_scope: 'foreground_window',
          capture_actual_scope: 'display',
          dispatch: 'ocr',
          screenshot_capture_elapsed_ms: 17,
          capture_fallback_reason: 'foreground_window_not_implemented',
          screen_ocr_source: 'prepared',
          screen_ocr_elapsed_ms: 60,
          screen_ocr_first_token_ms: 50,
          screen_ocr_text_chars: 12,
        },
        app_process_name: 'voicewin.exe',
        app_window_title: null,
        matched_profile_name: null,
        provider_kind: 'openai_compatible',
        api_kind: 'responses_sse',
        model: 'gpt-5.4',
        system_message: '<SYSTEM>\nFix grammar.\n</SYSTEM>',
        user_message: '<TRANSCRIPT>\nhello voicewin world\n</TRANSCRIPT>',
        raw_output: 'Hello, VoiceWin world.',
        final_output: 'Hello, VoiceWin world.',
        warning: 'LLM output looked conversational; VoiceWin stripped assistant framing from the model output.',
      },
      {
        elapsed_ms: 1500,
        first_token_ms: 900,
        input_tokens: 1536,
        cached_input_tokens: 0,
        visual_context_runtime: {
          mode: 'auto',
          capture_scope: 'foreground_window',
          capture_actual_scope: 'display',
          dispatch: 'ocr',
          screenshot_capture_elapsed_ms: 15,
          capture_fallback_reason: 'foreground_window_not_implemented',
          screen_ocr_source: 'prepared',
          screen_ocr_elapsed_ms: 50,
          screen_ocr_first_token_ms: 40,
          screen_ocr_text_chars: 10,
        },
        app_process_name: 'voicewin.exe',
        app_window_title: null,
        matched_profile_name: null,
        provider_kind: 'openai_compatible',
        api_kind: 'responses_sse',
        model: 'gpt-5.4',
        system_message: '<SYSTEM>\nFix grammar.\n</SYSTEM>',
        user_message: '<TRANSCRIPT>\nhello voicewin world\n</TRANSCRIPT>',
        raw_output: 'Hello, VoiceWin world.',
        final_output: 'Hello, VoiceWin world.',
        warning: null,
      },
    ];
    let previewCall = 0;
    invokeMock.mockImplementation(async (command: string, args?: any) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'set_config') return;
      if (command === 'preview_prompt') {
        expect(args).toEqual({
          req: expect.objectContaining({
            force_defaults: false,
            forced_profile_id: null,
            prompt: expect.objectContaining({
              id: 'prompt-default',
              title: 'Default Cleanup',
            }),
            transcript: 'hello voicewin world',
          }),
        });
        const next = previewResponses[previewCall];
        previewCall += 1;
        return next;
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<PromptsPage />);

    await screen.findByText('Default Cleanup');
    await user.click(screen.getByRole('button', { name: 'Run 3-Round Preview Benchmark' }));

    expect(
      await screen.findByText(
        /preview benchmark • 3 rounds • openai_compatible • responses_sse • gpt-5.4 • total 1200\/1500\/1800 ms min\/avg\/max • first token 700\/900\/1100 ms min\/avg\/max • visual auto->ocr \/ foreground-window \/ ocr-prepared \/ actual-display • visual variants 2 • capture 15\/17\/19 ms min\/avg\/max • ocr 40\/50\/60 ms min\/avg\/max • ocr first token 30\/40\/50 ms min\/avg\/max • ocr text 8\/10\/12 chars min\/avg\/max • input 1024\/1536\/2048 tok min\/avg\/max • cache 0\/0\/0 tok min\/avg\/max • capture fallbacks 2\/3 • warnings 1\/3 • final variants 2 • raw variants 2 • Last output: Hello, VoiceWin world\./i,
      ),
    ).toBeInTheDocument();
    expect(await screen.findByText(/Sample capture fallback: foreground_window_not_implemented/i)).toBeInTheDocument();
    expect(
      await screen.findByText(
        /Sample warning: LLM output looked conversational; VoiceWin stripped assistant framing from the model output\./i,
      ),
    ).toBeInTheDocument();
    expect(previewCall).toBe(3);
    expect(screen.getAllByDisplayValue('Hello, VoiceWin world.')).toHaveLength(2);
  });

  it('surfaces which round failed when the preview benchmark errors mid-run', async () => {
    const user = userEvent.setup();
    let previewCall = 0;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_config') return baseConfig();
      if (command === 'set_config') return;
      if (command === 'preview_prompt') {
        previewCall += 1;
        if (previewCall === 2) {
          throw new Error('gateway timeout');
        }
        return {
          elapsed_ms: 1200,
          first_token_ms: 700,
          input_tokens: 1024,
          cached_input_tokens: 0,
          app_process_name: 'voicewin.exe',
          app_window_title: null,
          matched_profile_name: null,
          provider_kind: 'openai_compatible',
          api_kind: 'responses_sse',
          model: 'gpt-5.4',
          system_message: '<SYSTEM>\nFix grammar.\n</SYSTEM>',
          user_message: '<TRANSCRIPT>\nhello voicewin world\n</TRANSCRIPT>',
          raw_output: 'Hello, Voicewin world.',
          final_output: 'Hello, Voicewin world.',
          warning: null,
        };
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<PromptsPage />);

    await screen.findByText('Default Cleanup');
    await user.click(screen.getByRole('button', { name: 'Run 3-Round Preview Benchmark' }));

    expect(await screen.findByText(/Preview benchmark failed on round 2\/3: Error: gateway timeout/i)).toBeInTheDocument();
    expect(previewCall).toBe(2);
  });
});
