import type { MessageReference } from '@/api/agent/chat';

/** An `@` being typed: where it starts and what follows it so far. */
export interface Mention {
  start: number;
  query: string;
}

/**
 * The `@` the caret is in: at the start of the text or after a space, followed by
 * the word typed so far (no spaces, at most 40 characters).
 */
export function mentionAt(text: string, caret: number): Mention | null {
  const before = text.slice(0, caret);
  const match = /(^|\s)@([^\s@]{0,40})$/.exec(before);
  if (!match) return null;
  const query = match[2] ?? '';
  return { start: before.length - query.length - 1, query };
}

/** Whether a reference is attached already. */
export function attached(references: readonly MessageReference[], candidate: MessageReference) {
  return references.some(
    (reference) => reference.kind === candidate.kind && reference.id === candidate.id,
  );
}
