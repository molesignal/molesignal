/** Grows a text box with its text, up to a limit past which it scrolls. */
export function growWithText(textarea: HTMLTextAreaElement | null, maxHeight = 200): void {
  if (!textarea) return;
  textarea.style.height = '0px';
  const nextHeight = Math.min(textarea.scrollHeight, maxHeight);
  textarea.style.height = `${nextHeight}px`;
  textarea.style.overflowY = textarea.scrollHeight > maxHeight ? 'auto' : 'hidden';
}
