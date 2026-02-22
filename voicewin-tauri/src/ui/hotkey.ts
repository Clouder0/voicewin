export type HotkeyParseResult = {
  hotkey: string | null;
  error?: string;
};

const CODE_TO_KEY: Record<string, string> = {
  Space: 'Space',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  Backquote: '`',
  Escape: 'Esc',
  Enter: 'Enter',
  Tab: 'Tab',
  Backspace: 'Backspace',
  Delete: 'Delete',
  Insert: 'Insert',
  Home: 'Home',
  End: 'End',
  PageUp: 'PageUp',
  PageDown: 'PageDown',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  CapsLock: 'CapsLock',
  PrintScreen: 'PrintScreen',
  ScrollLock: 'ScrollLock',
  Pause: 'Pause',
  NumLock: 'NumLock',
  NumpadAdd: 'NumAdd',
  NumpadSubtract: 'NumSubtract',
  NumpadMultiply: 'NumMultiply',
  NumpadDivide: 'NumDivide',
  NumpadDecimal: 'NumDecimal',
  NumpadEnter: 'NumEnter',
  NumpadEqual: 'NumEqual',
};

function isModifierKey(key: string, code: string): boolean {
  const normalized = key.toLowerCase();
  if (normalized === 'shift' || normalized === 'control' || normalized === 'ctrl' || normalized === 'alt' || normalized === 'meta' || normalized === 'super') {
    return true;
  }

  return (
    code === 'ShiftLeft' ||
    code === 'ShiftRight' ||
    code === 'ControlLeft' ||
    code === 'ControlRight' ||
    code === 'AltLeft' ||
    code === 'AltRight' ||
    code === 'MetaLeft' ||
    code === 'MetaRight'
  );
}

function keyFromCode(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) {
    return code.slice(3);
  }

  if (/^Digit[0-9]$/.test(code)) {
    return code.slice(5);
  }

  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) {
    return code;
  }

  if (/^Numpad[0-9]$/.test(code)) {
    return `Num${code.slice(6)}`;
  }

  return CODE_TO_KEY[code] ?? null;
}

function keyFromFallback(keyRaw: string): string | null {
  if (!keyRaw) return null;
  if (keyRaw === ' ' || keyRaw.toLowerCase() === 'spacebar') return 'Space';

  const lower = keyRaw.toLowerCase();
  if (lower === 'dead' || lower === 'unidentified' || lower === 'process') {
    return null;
  }

  if (keyRaw.length === 1) {
    return /[a-z]/i.test(keyRaw) ? keyRaw.toUpperCase() : keyRaw;
  }

  return keyRaw;
}

export function keydownToHotkey(e: KeyboardEvent): HotkeyParseResult {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push('Ctrl');
  if (e.shiftKey) mods.push('Shift');
  if (e.altKey) mods.push('Alt');
  if (e.metaKey) mods.push('Super');

  const keyRaw = e.key ?? '';
  const code = e.code ?? '';

  if (isModifierKey(keyRaw, code)) {
    return { hotkey: null };
  }

  const key = keyFromCode(code) ?? keyFromFallback(keyRaw);
  if (!key) {
    return { hotkey: null, error: 'Unsupported key for shortcut.' };
  }

  if (mods.length === 0) {
    return { hotkey: null, error: 'Include at least one modifier (Ctrl/Alt/Shift).' };
  }

  return { hotkey: [...mods, key].join('+') };
}
