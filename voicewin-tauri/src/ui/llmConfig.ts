import type {
  LlmApiKind,
  LlmPreflightMode,
  LlmProviderKind,
  LlmReasoningEffort,
  VisualCaptureScope,
  VisualContextMode,
} from '../lib/types';

export function normalizeLlmProviderKind(value: string | null | undefined): LlmProviderKind {
  return value === 'gemini' ? 'gemini' : 'openai_compatible';
}

export function normalizeLlmApiKind(value: string | null | undefined, providerKind: LlmProviderKind): LlmApiKind {
  if (providerKind === 'gemini') {
    return 'stream_generate_content_sse';
  }
  return value === 'chat_completions' ? 'chat_completions' : 'responses_sse';
}

export function normalizeLlmReasoningEffort(value: string | null | undefined): '' | LlmReasoningEffort {
  switch (value) {
    case 'minimal':
    case 'low':
    case 'medium':
    case 'high':
      return value;
    default:
      return '';
  }
}

export function normalizeLlmPreflightMode(value: string | null | undefined): LlmPreflightMode {
  return value === 'http_connect' ? 'http_connect' : 'off';
}

export function normalizeLlmPreflightDelayMs(value: number | string | null | undefined): number {
  const parsed =
    typeof value === 'number'
      ? value
      : typeof value === 'string'
        ? Number.parseInt(value, 10)
        : Number.NaN;

  if (!Number.isFinite(parsed) || parsed < 0) {
    return 1500;
  }

  return Math.min(Math.trunc(parsed), 60_000);
}

export function normalizeScreenshotMaxEdgePx(value: number | string | null | undefined): number {
  const parsed =
    typeof value === 'number'
      ? value
      : typeof value === 'string'
        ? Number.parseInt(value, 10)
        : Number.NaN;

  if (!Number.isFinite(parsed)) {
    return 1280;
  }

  return Math.min(Math.max(Math.trunc(parsed), 256), 3840);
}

export function normalizeVisualContextMode(value: string | null | undefined): VisualContextMode {
  switch (value) {
    case 'auto':
    case 'screenshot':
    case 'ocr':
      return value;
    default:
      return 'off';
  }
}

export function normalizeVisualCaptureScope(value: string | null | undefined): VisualCaptureScope {
  return value === 'foreground_window' ? 'foreground_window' : 'display';
}

export function recommendedApiKindForProvider(providerKind: LlmProviderKind): LlmApiKind {
  return providerKind === 'gemini' ? 'stream_generate_content_sse' : 'responses_sse';
}

export function defaultBaseUrlForProvider(providerKind: LlmProviderKind): string {
  return providerKind === 'gemini'
    ? 'https://generativelanguage.googleapis.com/v1beta'
    : 'https://api.openai.com/v1';
}

export function defaultModelForProvider(providerKind: LlmProviderKind): string {
  return providerKind === 'gemini' ? 'gemini-3-flash-preview' : 'gpt-5.4';
}

export function looksLikeUnknownModelError(value: string | null | undefined): boolean {
  if (!value) return false;
  const normalized = value.toLowerCase();
  return (
    normalized.includes('unknown model') ||
    normalized.includes('model not found') ||
    normalized.includes('unknown model name') ||
    value.includes('未知模型')
  );
}

export function llmSupportsAttachedImages(
  providerKind: string | null | undefined,
  apiKind: string | null | undefined,
): boolean {
  const normalizedProvider = normalizeLlmProviderKind(providerKind);
  const normalizedApiKind = apiKind?.trim() ?? '';

  if (normalizedProvider === 'gemini') {
    return normalizedApiKind === '' || normalizedApiKind === 'stream_generate_content_sse' || normalizedApiKind === 'gemini_stream_sse';
  }

  return normalizedApiKind === 'responses_sse' || normalizedApiKind === 'responses';
}

export function resolveVisualContextDispatch(
  mode: string | null | undefined,
  providerKind: string | null | undefined,
  apiKind: string | null | undefined,
): VisualContextMode {
  const normalizedMode = normalizeVisualContextMode(mode);
  if (normalizedMode === 'off') return 'off';
  if (normalizedMode === 'ocr') return 'ocr';
  if (normalizedMode === 'auto') {
    return llmSupportsAttachedImages(providerKind, apiKind) ? 'screenshot' : 'ocr';
  }
  return llmSupportsAttachedImages(providerKind, apiKind) ? 'screenshot' : 'off';
}

export function screenshotContextWarning(
  providerKind: string | null | undefined,
  apiKind: string | null | undefined,
): string | null {
  if (llmSupportsAttachedImages(providerKind, apiKind)) {
    return null;
  }

  const provider = providerKind?.trim() || 'openai_compatible';
  const api = apiKind?.trim() || 'default';
  return `Configured LLM API does not support screenshot context (provider=${provider} api=${api}); continuing without screenshot context.`;
}

export function isCc2Gateway(baseUrl: string | null | undefined): boolean {
  const value = baseUrl?.trim();
  if (!value) return false;

  try {
    return new URL(value).hostname === 'cc2.caaa.tech';
  } catch {
    return false;
  }
}

export function shouldRecommendResponsesForCc2OpenAiChatCompletions(
  providerKind: string | null | undefined,
  apiKind: string | null | undefined,
  baseUrl: string | null | undefined,
): boolean {
  return (
    normalizeLlmProviderKind(providerKind) === 'openai_compatible' &&
    (apiKind?.trim() ?? '') === 'chat_completions' &&
    isCc2Gateway(baseUrl)
  );
}

export function shouldRecommendGeminiForScreenshotContext(
  providerKind: string | null | undefined,
  apiKind: string | null | undefined,
  baseUrl: string | null | undefined,
  visualMode: string | null | undefined,
): boolean {
  return (
    resolveVisualContextDispatch(visualMode, providerKind, apiKind) === 'screenshot' &&
    normalizeLlmProviderKind(providerKind) === 'openai_compatible' &&
    llmSupportsAttachedImages(providerKind, apiKind) &&
    isCc2Gateway(baseUrl)
  );
}

export function recommendedCompactScreenshotMaxEdgePxForCurrentGateway(
  providerKind: string | null | undefined,
  baseUrl: string | null | undefined,
): number | null {
  if (normalizeLlmProviderKind(providerKind) !== 'gemini') {
    return null;
  }
  if (!isCc2Gateway(baseUrl)) {
    return null;
  }
  return 640;
}

export function shouldRecommendCompactScreenshotForLatency(
  providerKind: string | null | undefined,
  baseUrl: string | null | undefined,
  visualMode: string | null | undefined,
  screenshotMaxEdgePx: number | string | null | undefined,
): boolean {
  const recommended = recommendedCompactScreenshotMaxEdgePxForCurrentGateway(providerKind, baseUrl);
  if (normalizeVisualContextMode(visualMode) === 'off' || recommended == null) {
    return false;
  }
  return normalizeScreenshotMaxEdgePx(screenshotMaxEdgePx) > recommended;
}

export function recommendedGeminiBaseUrlForCurrentGateway(baseUrl: string | null | undefined): string {
  const value = baseUrl?.trim();
  if (!value) return defaultBaseUrlForProvider('gemini');

  try {
    const url = new URL(value);
    if (url.hostname !== 'cc2.caaa.tech') {
      return defaultBaseUrlForProvider('gemini');
    }
    url.pathname = '/v1beta';
    url.search = '';
    url.hash = '';
    return url.toString().replace(/\/$/, '');
  } catch {
    return defaultBaseUrlForProvider('gemini');
  }
}
