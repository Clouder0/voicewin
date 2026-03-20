import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { HistoryPage } from './HistoryPage';

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  isTauri: () => true,
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
}));

function historyEntry() {
  return {
    id: 'entry-1',
    ts_unix_ms: 1,
    app_process_name: 'Code',
    app_exe_path: null,
    app_window_title: null,
    text: 'Please ship the update today.',
    raw_transcript: 'please ship the update today',
    enhanced_text: 'Please ship the update today.',
    prompt_title: 'Email',
    matched_profile_name: 'Mail',
    detected_trigger_word: 'email',
    stt_provider: 'scribe',
    stt_model: 'scribe-v2',
    llm_provider: 'openai_compatible',
    llm_model: 'gpt-5.4',
    transcription_ms: 380,
    enhancement_ms: 145,
    enhancement_first_token_ms: 61,
    enhancement_input_tokens: 2048,
    visual_context_runtime: {
      mode: 'auto',
      capture_scope: 'foreground_window',
      capture_actual_scope: 'display',
      dispatch: 'ocr',
      screenshot_capture_elapsed_ms: 19,
      capture_fallback_reason: 'foreground_window_not_implemented',
      screen_ocr_source: 'prepared',
      screen_ocr_elapsed_ms: 52,
      screen_ocr_first_token_ms: 41,
      screen_ocr_text_chars: 8,
    },
    context_flags: {
      use_clipboard: false,
      use_selected_text: false,
      use_window_context: true,
      use_custom_vocabulary: false,
      visual_context_mode: 'auto',
      visual_capture_scope: 'foreground_window',
    },
    stage: 'error',
    warning: 'LLM output looked conversational; VoiceWin fell back to the dictated transcript.',
    error: 'microphone denied',
  };
}

function replayPreview(overrides: Record<string, unknown> = {}) {
  return {
    elapsed_ms: 321,
    first_token_ms: 123,
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
      screen_ocr_elapsed_ms: 48,
      screen_ocr_first_token_ms: 33,
      screen_ocr_text_chars: 8,
    },
    app_process_name: 'Code',
    app_window_title: 'Inbox',
    matched_profile_name: 'Mail',
    provider_kind: 'openai_compatible',
    api_kind: 'responses_sse',
    model: 'gpt-5.4',
    system_message: 'system',
    user_message: 'user',
    raw_output: 'Please ship the update today.',
    final_output: 'Please ship the update today.',
    warning: 'LLM output looked conversational; VoiceWin stripped assistant framing from the model output.',
    ...overrides,
  };
}

describe('HistoryPage', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_history') {
        return [historyEntry()];
      }
      if (command === 'delete_history_entry_by_id') {
        return true;
      }
      if (command === 'preview_history_entry') {
        return replayPreview();
      }
      if (command === 'clear_history') {
        return;
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });
  });

  it('deletes rows by stable id when available', async () => {
    const user = userEvent.setup();
    render(<HistoryPage />);

    expect(await screen.findByText('Please ship the update today.')).toBeInTheDocument();
    expect(screen.getByText('Transcript: please ship the update today')).toBeInTheDocument();
    expect(
      screen.getByText(/Stage: error • Profile: Mail • Prompt: Email • Trigger: email • STT: scribe \/ scribe-v2/i),
    ).toBeInTheDocument();
    expect(screen.getByText(/LLM: openai_compatible \/ gpt-5.4/i)).toBeInTheDocument();
    expect(
      screen.getByText(
        /Latency: STT 380 ms • LLM 145 ms • LLM first token 61 ms • LLM input 2048 tok • Capture 19 ms • OCR prepared • OCR 52 ms • OCR first token 41 ms • OCR text 8 chars/i,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /Context: window, visual:auto->ocr, capture:foreground-window, captured:display, fallback:foreground_window_not_implemented, ocr:prepared/i,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /Warning: LLM output looked conversational; VoiceWin fell back to the dictated transcript./i,
      ),
    ).toBeInTheDocument();

    const row = screen.getByText('Please ship the update today.');
    const container = row.closest('.vw-historyRow');
    expect(container).not.toBeNull();

    const deleteButton = within(container as HTMLElement).getByRole('button', { name: 'Delete' });
    await user.click(deleteButton);

    expect(invokeMock).toHaveBeenCalledWith('delete_history_entry_by_id', { id: 'entry-1' });
  });

  it('replays a history entry through prompt preview', async () => {
    const user = userEvent.setup();
    render(<HistoryPage />);

    const row = await screen.findByText('Please ship the update today.');
    const container = row.closest('.vw-historyRow');
    expect(container).not.toBeNull();

    const replayButton = within(container as HTMLElement).getByRole('button', { name: 'Enhance Again' });
    await user.click(replayButton);

    expect(invokeMock).toHaveBeenCalledWith('preview_history_entry', { id: 'entry-1' });
    expect(await screen.findByText('Replay Preview: Please ship the update today.')).toBeInTheDocument();
    expect(screen.getByText(/openai_compatible \/ gpt-5.4 • 321 ms total • 123 ms first token • 2048 input tok • 0 cached tok/i)).toBeInTheDocument();
    expect(screen.getByText(/Code • Profile Mail/i)).toBeInTheDocument();
    expect(
      screen.getByText(
        /Visual auto -> ocr • capture foreground-window • captured display • Capture 17 ms • fallback foreground_window_not_implemented • OCR prepared • OCR 48 ms • OCR first token 33 ms • OCR text 8 chars/i,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /Warning: LLM output looked conversational; VoiceWin stripped assistant framing from the model output./i,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Uses the saved transcript and prompt when available. Clipboard and selection context are not replayed from history./i),
    ).toBeInTheDocument();
  });

  it('benchmarks a history replay across three rounds and renders the summary', async () => {
    const user = userEvent.setup();
    const previewResponses = [
      replayPreview({
        elapsed_ms: 1800,
        first_token_ms: 1100,
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
        raw_output: 'Please ship the update today.',
        final_output: 'Please ship the update today.',
        warning: null,
      }),
      replayPreview({
        elapsed_ms: 1500,
        first_token_ms: 900,
        input_tokens: 2048,
        cached_input_tokens: 12,
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
        raw_output: 'Please ship the update today!',
        final_output: 'Please ship the update today!',
      }),
      replayPreview({
        elapsed_ms: 1200,
        first_token_ms: 700,
        input_tokens: 3072,
        cached_input_tokens: 24,
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
        raw_output: 'Please ship the update today!',
        final_output: 'Please ship the update today!',
        warning: null,
      }),
    ];
    let previewCall = 0;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_history') return [historyEntry()];
      if (command === 'delete_history_entry_by_id') return true;
      if (command === 'clear_history') return;
      if (command === 'preview_history_entry') {
        const next = previewResponses[previewCall];
        previewCall += 1;
        return next;
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<HistoryPage />);

    const row = await screen.findByText('Please ship the update today.');
    const container = row.closest('.vw-historyRow');
    expect(container).not.toBeNull();

    await user.click(within(container as HTMLElement).getByRole('button', { name: 'Benchmark Again' }));

    expect(
      await screen.findByText(
        /Replay benchmark • 3 rounds • openai_compatible • responses_sse • gpt-5.4 • total 1200\/1500\/1800 ms min\/avg\/max • first token 700\/900\/1100 ms min\/avg\/max • visual auto->ocr \/ foreground-window \/ ocr-prepared \/ actual-display • visual variants 2 • capture 15\/17\/19 ms min\/avg\/max • ocr 40\/50\/60 ms min\/avg\/max • ocr first token 30\/40\/50 ms min\/avg\/max • ocr text 8\/10\/12 chars min\/avg\/max • input 1024\/2048\/3072 tok min\/avg\/max • cache 0\/12\/24 tok min\/avg\/max • capture fallbacks 2\/3 • warnings 1\/3 • final variants 2 • raw variants 2 • Last output: Please ship the update today!/i,
      ),
    ).toBeInTheDocument();
    expect(screen.getByText(/Sample capture fallback: foreground_window_not_implemented/i)).toBeInTheDocument();
    expect(
      screen.getByText(
        /Sample warning: LLM output looked conversational; VoiceWin stripped assistant framing from the model output\./i,
      ),
    ).toBeInTheDocument();
    expect(screen.getByText('Replay Preview: Please ship the update today!')).toBeInTheDocument();
    expect(previewCall).toBe(3);
  });

  it('reports which round failed during replay benchmarking', async () => {
    const user = userEvent.setup();
    let previewCall = 0;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_history') return [historyEntry()];
      if (command === 'delete_history_entry_by_id') return true;
      if (command === 'clear_history') return;
      if (command === 'preview_history_entry') {
        previewCall += 1;
        if (previewCall === 2) {
          throw new Error('gateway timeout');
        }
        return replayPreview({ warning: null });
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<HistoryPage />);

    const row = await screen.findByText('Please ship the update today.');
    const container = row.closest('.vw-historyRow');
    expect(container).not.toBeNull();

    await user.click(within(container as HTMLElement).getByRole('button', { name: 'Benchmark Again' }));

    expect(await screen.findByText(/Replay benchmark failed on round 2\/3: Error: gateway timeout/i)).toBeInTheDocument();
    expect(previewCall).toBe(2);
  });
});
