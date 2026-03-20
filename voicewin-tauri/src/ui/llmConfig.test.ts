import { describe, expect, it } from 'vitest';

import {
  isCc2Gateway,
  recommendedCompactScreenshotMaxEdgePxForCurrentGateway,
  recommendedGeminiBaseUrlForCurrentGateway,
  shouldRecommendResponsesForCc2OpenAiChatCompletions,
  shouldRecommendCompactScreenshotForLatency,
  shouldRecommendGeminiForScreenshotContext,
} from './llmConfig';

describe('shouldRecommendGeminiForScreenshotContext', () => {
  it('recommends Gemini for screenshot context on the validated cc2 OpenAI-compatible stack', () => {
    expect(
      shouldRecommendGeminiForScreenshotContext(
        'openai_compatible',
        'responses_sse',
        'https://cc2.caaa.tech/v1',
        'screenshot',
      ),
    ).toBe(true);
  });

  it('does not recommend Gemini when screenshot context is disabled', () => {
    expect(
      shouldRecommendGeminiForScreenshotContext(
        'openai_compatible',
        'responses_sse',
        'https://cc2.caaa.tech/v1',
        'off',
      ),
    ).toBe(false);
  });

  it('does not recommend Gemini for non-cc2 gateways or text-only API modes', () => {
    expect(
      shouldRecommendGeminiForScreenshotContext(
        'openai_compatible',
        'responses_sse',
        'https://api.openai.com/v1',
        'screenshot',
      ),
    ).toBe(false);

    expect(
      shouldRecommendGeminiForScreenshotContext(
        'openai_compatible',
        'chat_completions',
        'https://cc2.caaa.tech/v1',
        'screenshot',
      ),
    ).toBe(false);
  });
});

describe('recommendedGeminiBaseUrlForCurrentGateway', () => {
  it('maps the validated cc2 gateway onto the Gemini v1beta base path', () => {
    expect(recommendedGeminiBaseUrlForCurrentGateway('https://cc2.caaa.tech/v1')).toBe(
      'https://cc2.caaa.tech/v1beta',
    );
  });

  it('preserves the current origin when the validated gateway uses a custom port', () => {
    expect(recommendedGeminiBaseUrlForCurrentGateway('https://cc2.caaa.tech:8443/v1?x=1#frag')).toBe(
      'https://cc2.caaa.tech:8443/v1beta',
    );
  });

  it('falls back to the public Gemini default for other gateways', () => {
    expect(recommendedGeminiBaseUrlForCurrentGateway('https://api.openai.com/v1')).toBe(
      'https://generativelanguage.googleapis.com/v1beta',
    );
  });
});

describe('isCc2Gateway', () => {
  it('detects the validated cc2 hostname', () => {
    expect(isCc2Gateway('https://cc2.caaa.tech/v1')).toBe(true);
    expect(isCc2Gateway('https://cc2.caaa.tech:8443/v1beta')).toBe(true);
  });

  it('rejects other hosts and invalid URLs', () => {
    expect(isCc2Gateway('https://api.openai.com/v1')).toBe(false);
    expect(isCc2Gateway('not-a-url')).toBe(false);
    expect(isCc2Gateway(null)).toBe(false);
  });
});

describe('shouldRecommendResponsesForCc2OpenAiChatCompletions', () => {
  it('recommends responses for the validated cc2 openai-compatible chat-completions stack', () => {
    expect(
      shouldRecommendResponsesForCc2OpenAiChatCompletions(
        'openai_compatible',
        'chat_completions',
        'https://cc2.caaa.tech/v1',
      ),
    ).toBe(true);
  });

  it('does not recommend responses for other providers, apis, or gateways', () => {
    expect(
      shouldRecommendResponsesForCc2OpenAiChatCompletions(
        'openai_compatible',
        'responses_sse',
        'https://cc2.caaa.tech/v1',
      ),
    ).toBe(false);
    expect(
      shouldRecommendResponsesForCc2OpenAiChatCompletions(
        'gemini',
        'stream_generate_content_sse',
        'https://cc2.caaa.tech/v1beta',
      ),
    ).toBe(false);
    expect(
      shouldRecommendResponsesForCc2OpenAiChatCompletions(
        'openai_compatible',
        'chat_completions',
        'https://api.openai.com/v1',
      ),
    ).toBe(false);
  });
});

describe('recommendedCompactScreenshotMaxEdgePxForCurrentGateway', () => {
  it('recommends 640 px for Gemini screenshot latency on the validated cc2 gateway', () => {
    expect(
      recommendedCompactScreenshotMaxEdgePxForCurrentGateway('gemini', 'https://cc2.caaa.tech/v1beta'),
    ).toBe(640);
  });

  it('does not recommend a compact screenshot size for other providers or gateways', () => {
    expect(
      recommendedCompactScreenshotMaxEdgePxForCurrentGateway('openai_compatible', 'https://cc2.caaa.tech/v1'),
    ).toBeNull();
    expect(
      recommendedCompactScreenshotMaxEdgePxForCurrentGateway('gemini', 'https://generativelanguage.googleapis.com/v1beta'),
    ).toBeNull();
  });
});

describe('shouldRecommendCompactScreenshotForLatency', () => {
  it('recommends compact screenshots when Gemini screenshot context is enabled above 640 px on cc2', () => {
    expect(
      shouldRecommendCompactScreenshotForLatency('gemini', 'https://cc2.caaa.tech/v1beta', 'screenshot', 1280),
    ).toBe(true);
  });

  it('does not recommend compact screenshots when already compact or when screenshot context is disabled', () => {
    expect(
      shouldRecommendCompactScreenshotForLatency('gemini', 'https://cc2.caaa.tech/v1beta', 'screenshot', 640),
    ).toBe(false);
    expect(
      shouldRecommendCompactScreenshotForLatency('gemini', 'https://cc2.caaa.tech/v1beta', 'off', 1280),
    ).toBe(false);
  });
});
