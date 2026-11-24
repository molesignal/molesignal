import type { BusinessContext, BusinessKey } from '@/api/businessContext';

/** A key as the card edits it: every field as typed, the important values one text. */
export interface BusinessKeyDraft {
  key: string;
  label: string;
  span: string;
  log: string;
  rum: string;
  important: string;
}

export const MAX_BUSINESS_KEYS = 8;
const KEY_PATTERN = /^[a-z][a-z0-9_]{0,63}$/;

export function blankBusinessKey(): BusinessKeyDraft {
  return { key: '', label: '', span: '', log: '', rum: '', important: '' };
}

export function draftOf(context: BusinessContext): BusinessKeyDraft[] {
  return context.keys.map((key) => ({
    key: key.key,
    label: key.label,
    span: key.span ?? '',
    log: key.log ?? '',
    rum: key.rum ?? '',
    important: key.important.join(', '),
  }));
}

/** The keys to save: blank fields name nothing, and important values are split on commas and new lines. */
export function contextOf(drafts: readonly BusinessKeyDraft[]): BusinessContext {
  const named = (value: string) => {
    const trimmed = value.trim();
    return trimmed ? trimmed : undefined;
  };

  return {
    keys: drafts.map((draft): BusinessKey => {
      const span = named(draft.span);
      const log = named(draft.log);
      const rum = named(draft.rum);

      return {
        key: draft.key.trim(),
        label: draft.label.trim(),
        ...(span ? { span } : {}),
        ...(log ? { log } : {}),
        ...(rum ? { rum } : {}),
        important: [
          ...new Set(
            draft.important
              .split(/[,\n]/)
              .map((value) => value.trim())
              .filter(Boolean),
          ),
        ],
      };
    }),
  };
}

/** What is wrong with a key, as the key of the message that says it; `null` when nothing is. */
export type BusinessKeyIssue = 'key' | 'twice' | 'nowhere';

export function businessKeyIssues(drafts: readonly BusinessKeyDraft[]): Map<number, BusinessKeyIssue> {
  const issues = new Map<number, BusinessKeyIssue>();
  const seen = new Map<string, number>();

  drafts.forEach((draft, index) => {
    const key = draft.key.trim();

    if (!KEY_PATTERN.test(key)) {
      issues.set(index, 'key');
    } else if (seen.has(key)) {
      issues.set(index, 'twice');
    } else if (![draft.span, draft.log, draft.rum].some((field) => field.trim())) {
      issues.set(index, 'nowhere');
    }

    if (!seen.has(key)) seen.set(key, index);
  });

  return issues;
}
