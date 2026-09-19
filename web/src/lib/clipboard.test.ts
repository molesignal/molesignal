import { afterEach, describe, expect, it, vi } from 'vitest';

import { writeClipboardText } from './clipboard';

const restoreProperty = (
  target: object,
  property: PropertyKey,
  value: unknown,
): (() => void) => {
  const descriptor = Object.getOwnPropertyDescriptor(target, property);
  Object.defineProperty(target, property, {
    configurable: true,
    value,
  });
  return () => {
    if (descriptor) Object.defineProperty(target, property, descriptor);
    else Reflect.deleteProperty(target, property);
  };
};

describe('writeClipboardText', () => {
  afterEach(() => vi.restoreAllMocks());

  it('uses the Clipboard API in a secure context', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const restoreSecureContext = restoreProperty(
      globalThis,
      'isSecureContext',
      true,
    );
    const restoreClipboard = restoreProperty(navigator, 'clipboard', {
      writeText,
    });

    try {
      await writeClipboardText('secure value');
      expect(writeText).toHaveBeenCalledWith('secure value');
    } finally {
      restoreClipboard();
      restoreSecureContext();
    }
  });

  it('uses a synchronous selection fallback on plain HTTP', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const execCommand = vi.fn(() => true);
    const restoreSecureContext = restoreProperty(
      globalThis,
      'isSecureContext',
      false,
    );
    const restoreClipboard = restoreProperty(navigator, 'clipboard', {
      writeText,
    });
    const restoreExecCommand = restoreProperty(
      document,
      'execCommand',
      execCommand,
    );
    const input = document.createElement('input');
    document.body.appendChild(input);
    input.focus();

    try {
      await writeClipboardText('intranet value');
      expect(writeText).not.toHaveBeenCalled();
      expect(execCommand).toHaveBeenCalledWith('copy');
      expect(document.activeElement).toBe(input);
      expect(document.querySelector('textarea[aria-hidden="true"]')).toBeNull();
    } finally {
      input.remove();
      restoreExecCommand();
      restoreClipboard();
      restoreSecureContext();
    }
  });

  it('rejects when neither copy mechanism succeeds', async () => {
    const restoreSecureContext = restoreProperty(
      globalThis,
      'isSecureContext',
      false,
    );
    const restoreClipboard = restoreProperty(navigator, 'clipboard', undefined);
    const restoreExecCommand = restoreProperty(
      document,
      'execCommand',
      vi.fn(() => false),
    );

    try {
      await expect(writeClipboardText('blocked value')).rejects.toThrow(
        'Unable to copy text to the clipboard',
      );
    } finally {
      restoreExecCommand();
      restoreClipboard();
      restoreSecureContext();
    }
  });
});
