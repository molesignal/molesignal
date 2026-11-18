import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import type { DrillRun, DrillSummary } from '@/api/agent/drills';

import {
  REFERENCE_MAX,
  canRun,
  failureNote,
  referenceProblem,
  runChatLink,
  runGoing,
  runnable,
} from './drillsModel';

const t = ((key: string, options?: Record<string, unknown>) =>
  options ? `${key} ${JSON.stringify(options)}` : key) as unknown as TFunction;

function run(patch: Partial<DrillRun>): DrillRun {
  return {
    id: 'run-1',
    orgId: 'org',
    drillId: 'drill-1',
    chatId: 'chat-1',
    runId: 'agent-run-1',
    status: 'done',
    score: 0.75,
    reasoning: 'Names the pool.',
    reason: null,
    detail: null,
    model: 'gpt-4.1',
    promptVersion: 2,
    graderModel: 'gpt-4.1-mini',
    language: 'en-us',
    batchId: null,
    attempts: 1,
    startedBy: 'dev',
    createdAt: 1,
    finishedAt: 2,
    updatedAt: 2,
    ...patch,
  };
}

function drill(lastRun: DrillRun | null): DrillSummary {
  return {
    id: 'drill-1',
    orgId: 'org',
    incidentId: 'inc-1',
    title: 'Checkout 5xx',
    reference: 'The pool was too small.',
    source: 'postmortem',
    createdBy: 'dev',
    createdAt: 1,
    updatedAt: 1,
    runs: lastRun ? 1 : 0,
    meanScore: lastRun?.score ?? null,
    lastRun,
  };
}

describe('drills', () => {
  it('runs a drill only when no run of it is under way', () => {
    expect(runGoing(run({ status: 'running' }))).toBe(true);
    expect(runGoing(run({ status: 'grading' }))).toBe(true);
    expect(runGoing(run({ status: 'failed' }))).toBe(false);
    expect(runGoing(null)).toBe(false);
    expect(canRun(drill(null))).toBe(true);
    expect(canRun(drill(run({ status: 'running' })))).toBe(false);
    expect(runnable([drill(null), drill(run({ status: 'grading' })), drill(run({}))])).toHaveLength(2);
  });

  it('says why a run has no score, and where its chat is', () => {
    expect(failureNote(t, run({ status: 'failed', reason: 'no_model' }))).toBe(
      'quality.drills.failures.no_model {"defaultValue":"quality.drills.failures.run_failed"}',
    );
    expect(runChatLink(run({}))).toBe('/agent/chat?chat=chat-1');
    expect(runChatLink(run({ chatId: null }))).toBeNull();
  });

  it('checks a hand-written reference', () => {
    expect(referenceProblem('  ')).toBe('empty');
    expect(referenceProblem('x'.repeat(REFERENCE_MAX + 1))).toBe('long');
    expect(referenceProblem('连'.repeat(REFERENCE_MAX))).toBeNull();
  });
});
