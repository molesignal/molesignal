import type { TFunction } from 'i18next';

import type { DrillRun, DrillSummary } from '@/api/agent/drills';

/** The longest reference root cause a drill takes, as the backend counts it (characters). */
export const REFERENCE_MAX = 4000;

/** A run still under way: investigating, waiting to be graded, or being graded. */
export function runGoing(run: Pick<DrillRun, 'status'> | null | undefined): boolean {
  return run?.status === 'running' || run?.status === 'grading';
}

/** A drill can be run when no run of it is under way. */
export function canRun(drill: Pick<DrillSummary, 'lastRun'>): boolean {
  return !runGoing(drill.lastRun);
}

/** How a run stands, in a word or two. */
export function runStatusLabel(t: TFunction, run: Pick<DrillRun, 'status'>): string {
  return t(`quality.drills.status.${run.status}`);
}

/** Why a run ended without a score. */
export function failureNote(t: TFunction, run: Pick<DrillRun, 'reason'>): string {
  return t(`quality.drills.failures.${run.reason ?? 'run_failed'}`, {
    defaultValue: t('quality.drills.failures.run_failed'),
  });
}

/** Where the investigation of a run can be read: its hidden chat. */
export function runChatLink(run: Pick<DrillRun, 'chatId'>): string | null {
  return run.chatId ? `/agent/chat?chat=${encodeURIComponent(run.chatId)}` : null;
}

/** What keeps a hand-written reference from being saved. */
export function referenceProblem(reference: string): 'empty' | 'long' | null {
  const text = reference.trim();
  if (!text) return 'empty';
  return Array.from(text).length > REFERENCE_MAX ? 'long' : null;
}

/** Drills a "run all" starts: those with no run under way. */
export function runnable(drills: readonly DrillSummary[]): DrillSummary[] {
  return drills.filter(canRun);
}
