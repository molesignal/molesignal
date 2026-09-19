const COPY_FAILED_MESSAGE = 'Unable to copy text to the clipboard';

function copyTextWithSelection(text: string): void {
  const activeElement =
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
  const selection = document.getSelection();
  const ranges = selection
    ? Array.from({ length: selection.rangeCount }, (_, index) =>
        selection.getRangeAt(index).cloneRange(),
      )
    : [];
  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.readOnly = true;
  textarea.setAttribute('aria-hidden', 'true');
  textarea.style.position = 'fixed';
  textarea.style.inset = '0 auto auto -9999px';
  textarea.style.opacity = '0';
  textarea.style.pointerEvents = 'none';
  document.body.appendChild(textarea);

  let copied = false;
  try {
    textarea.select();
    textarea.setSelectionRange(0, textarea.value.length);
    copied =
      typeof document.execCommand === 'function' &&
      document.execCommand('copy');
  } finally {
    textarea.remove();
    activeElement?.focus({ preventScroll: true });
    selection?.removeAllRanges();
    for (const range of ranges) selection?.addRange(range);
  }

  if (!copied) throw new Error(COPY_FAILED_MESSAGE);
}

/**
 * Copy text across both secure deployments and plain-HTTP private networks.
 *
 * The fallback must execute synchronously inside the originating click when
 * `isSecureContext` is false; awaiting a rejected Clipboard API call first
 * would consume the transient user activation required by legacy copy.
 */
export async function writeClipboardText(text: string): Promise<void> {
  if (
    globalThis.isSecureContext !== false &&
    navigator.clipboard?.writeText
  ) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // Permission policies can block the modern API even on HTTPS.
    }
  }

  copyTextWithSelection(text);
}
