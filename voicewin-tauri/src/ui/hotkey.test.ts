import { describe, expect, it } from 'vitest';

import { keydownToHotkey } from './hotkey';

describe('keydownToHotkey', () => {
  it('uses physical code for Alt+KeyS even when key is a glyph', () => {
    const out = keydownToHotkey({
      key: 'ß',
      code: 'KeyS',
      altKey: true,
      ctrlKey: false,
      shiftKey: false,
      metaKey: false,
    } as KeyboardEvent);

    expect(out.hotkey).toBe('Alt+S');
  });

  it('uses physical code for Alt+Digit2 when key is a symbol', () => {
    const out = keydownToHotkey({
      key: '™',
      code: 'Digit2',
      altKey: true,
      ctrlKey: false,
      shiftKey: false,
      metaKey: false,
    } as KeyboardEvent);

    expect(out.hotkey).toBe('Alt+2');
  });

  it('falls back to code when key is Dead', () => {
    const out = keydownToHotkey({
      key: 'Dead',
      code: 'KeyE',
      altKey: true,
      ctrlKey: false,
      shiftKey: false,
      metaKey: false,
    } as KeyboardEvent);

    expect(out.hotkey).toBe('Alt+E');
  });

  it('requires at least one modifier', () => {
    const out = keydownToHotkey({
      key: 'a',
      code: 'KeyA',
      altKey: false,
      ctrlKey: false,
      shiftKey: false,
      metaKey: false,
    } as KeyboardEvent);

    expect(out.hotkey).toBeNull();
    expect(out.error).toMatch(/modifier/i);
  });
});
