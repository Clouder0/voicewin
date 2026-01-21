import type { AppConfig, HistoryEntry, ProviderStatus } from './types';

export type SessionResult = {
  stage: string;
  final_text?: string;
  error?: string;
};

export type Invoker = {
  runSession: (args: { transcript: string }) => Promise<SessionResult>;
  toggleRecording: () => Promise<SessionResult>;
  getConfig: () => Promise<AppConfig>;
  setConfig: (cfg: AppConfig) => Promise<void>;
  getProviderStatus: () => Promise<ProviderStatus>;
  setOpenAiApiKey: (apiKey: string) => Promise<void>;
  setElevenLabsApiKey: (apiKey: string) => Promise<void>;
  getHistory: () => Promise<HistoryEntry[]>;
  clearHistory: () => Promise<void>;
  getModelStatus: () => Promise<{
    bootstrap_ok: boolean;
    bootstrap_path: string;
    preferred_ok: boolean;
    preferred_path: string;
  }>;
  installPreferredModel: (args: { src_path: string }) => Promise<string>;
  cancelRecording: () => Promise<SessionResult>;
  pickPreferredModelFile: () => Promise<string | null>;
};

export function createMockInvoker(): Invoker {
  const mockConfig: AppConfig = {
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

  let openaiKey = '';
  let elevenKey = '';

  let history: HistoryEntry[] = [];

  return {
    async runSession({ transcript }) {
      const result = { stage: 'done', final_text: `Enhanced: ${transcript}` };
      if (mockConfig.defaults.history_enabled && result.final_text) {
        history = [
          ...history,
          {
            ts_unix_ms: Date.now(),
            text: result.final_text,
            stage: result.stage,
          },
        ].slice(-200);
      }
      return result;
    },
    async toggleRecording() {
      return { stage: 'recording', final_text: undefined };
    },
    async getConfig() {
      return mockConfig;
    },
    async setConfig(cfg) {
      mockConfig.defaults = cfg.defaults;
      mockConfig.profiles = cfg.profiles;
      mockConfig.prompts = cfg.prompts;
      mockConfig.llm_api_key_present = cfg.llm_api_key_present;
    },
    async getProviderStatus() {
      return {
        openai_api_key_present: openaiKey.length > 0,
        elevenlabs_api_key_present: elevenKey.length > 0,
      };
    },
    async setOpenAiApiKey(apiKey) {
      openaiKey = apiKey;
    },
    async setElevenLabsApiKey(apiKey) {
      elevenKey = apiKey;
    },
    async getHistory() {
      return history;
    },
    async clearHistory() {
      history = [];
    },
    async getModelStatus() {
      return {
        bootstrap_ok: true,
        bootstrap_path: './models/bootstrap.gguf',
        preferred_ok: false,
        preferred_path: './models/whisper-large-v3-turbo-q5_k.gguf',
      };
    },
    async installPreferredModel({ src_path }) {
      // Mock: accept any path.
      void src_path;
      return './models/whisper-large-v3-turbo-q5_k.gguf';
    },
    async cancelRecording() {
      return { stage: 'cancelled', final_text: undefined };
    },
    async pickPreferredModelFile() {
      return './models/whisper-large-v3-turbo-q5_k.gguf';
    },
  };
}

export async function createTauriInvoker(): Promise<Invoker> {
  const { invoke } = await import('@tauri-apps/api/core');

  return {
    async runSession(args) {
      return invoke<SessionResult>('run_session', args);
    },
    async toggleRecording() {
      return invoke<SessionResult>('toggle_recording');
    },
    async getConfig() {
      return invoke<AppConfig>('get_config');
    },
    async setConfig(cfg) {
      await invoke('set_config', { cfg });
    },
    async getProviderStatus() {
      return invoke<ProviderStatus>('get_provider_status');
    },
    async setOpenAiApiKey(apiKey) {
      await invoke('set_openai_api_key', { api_key: apiKey });
    },
    async setElevenLabsApiKey(apiKey) {
      await invoke('set_elevenlabs_api_key', { api_key: apiKey });
    },
    async getHistory() {
      return invoke<HistoryEntry[]>('get_history');
    },
    async clearHistory() {
      await invoke('clear_history');
    },
    async getModelStatus() {
      return invoke('get_model_status');
    },
    async installPreferredModel(args) {
      return invoke<string>('install_preferred_model', args);
    },
    async cancelRecording() {
      return invoke<SessionResult>('cancel_recording');
    },
    async pickPreferredModelFile() {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selected = await open({
          multiple: false,
          filters: [
            {
              name: 'GGUF model',
              extensions: ['gguf'],
            },
          ],
        });

        return typeof selected === 'string' ? selected : null;
      } catch {
        return null;
      }
    },
  };
}
