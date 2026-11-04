import type { TFunction } from 'i18next';

import type { StatusTone } from '@/shell/StatusWord';

/**
 * An action as a person reads it. The backend names actions as dotted keys
 * (`agent.automation.token_issued`); a sentence for each would have to be kept in
 * step with every new one, so it is read word by word, and a word nobody
 * translated yet stands as it is.
 */
export function humanAuditAction(action: string, t: TFunction<'settings-admin'>): string {
  return action
    .split(/[._]/)
    .filter(Boolean)
    .map((word) => t(`audit.words.${word}`, { defaultValue: word }))
    .join(' ');
}

const ACCESS_ACTION = /^(mfa|signin|sessions?|access_|trace_debug_token|resource_share|api_token|tokens?|roles?|iam|users?|members?|memberships?|sso|invitations?|service_accounts?)\b/;

/** Who can get in and with what: the part of the trail an access review reads. */
export function isAccessAction(action: string): boolean {
  return ACCESS_ACTION.test(action) || /token/.test(action);
}

const WENT_WELL = new Set(['ok', 'success', 'succeeded', 'completed', 'done', 'approved', 'executed', 'resolved']);
const WENT_WRONG = new Set(['failed', 'failure', 'error', 'denied', 'rejected', 'refused']);

/**
 * The colour of a status in the trail. Actions write their own words, so a word that says neither way
 * (pending, running, expired, a word added later) stays plain: red is for what went wrong.
 */
export function auditStatusTone(status: string): StatusTone {
  const word = status.trim().toLowerCase();

  if (WENT_WELL.has(word)) return 'green';
  return WENT_WRONG.has(word) ? 'red' : 'neutral';
}

/** Actions and object kinds seen in the trail, sorted, for the filters' lists. */
export function distinctValues(values: Iterable<string | null | undefined>): string[] {
  const seen = new Set<string>();

  for (const value of values) {
    if (value) seen.add(value);
  }

  return [...seen].sort((a, b) => a.localeCompare(b));
}
