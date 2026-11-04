import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import type { Evaluator, Score, ScoredAnswer } from '@/api/agent/quality';

import {
  answerLink,
  dimensionNote,
  draftFromEvaluator,
  evaluatorProblems,
  formatScore,
  inputFromDraft,
  newEvaluatorDraft,
  overallScore,
  qualityViewOf,
  scorePoints,
  scoreTone,
  trendPoints,
} from './model';

const t = ((key: string, options?: Record<string, unknown>) =>
  options ? `${key} ${JSON.stringify(options)}` : key) as unknown as TFunction;

function score(scorer: string, value: number): Score {
  return {
    id: `${scorer}-1`,
    targetKind: 'chat_answer',
    targetId: 'm1',
    scorer,
    score: value,
    dimensions: [],
    reason: '',
    samples: 1,
    createdAt: 1,
    updatedAt: 1,
  };
}

describe('quality scores', () => {
  it('reads 0 to 1 as 0 to 100 and colours by the threshold', () => {
    expect(scorePoints(0.756)).toBe(76);
    expect(scorePoints(1.4)).toBe(100);
    expect(scorePoints(null)).toBeNull();
    expect(formatScore(undefined)).toBe('—');
    expect(scoreTone(0.5, 0.6)).toBe('poor');
    expect(scoreTone(0.7, 0.6)).toBe('fair');
    expect(scoreTone(0.85, 0.6)).toBe('good');
  });

  it("scores an answer by its evaluators' mean, else by its rules", () => {
    expect(overallScore({ scores: [score('rules', 0.9)] })).toBe(0.9);
    expect(overallScore({ scores: [score('rules', 0.9), score('e1', 0.25), score('e2', 0.75)] })).toBe(0.5);
    expect(overallScore({ scores: [] })).toBeNull();
  });

  it('says what a rule found in words', () => {
    expect(dimensionNote(t, { dimension: 'tool_success', score: 0.5, detail: { failed: 1, total: 2 } })).toBe(
      'quality.notes.tools_failed {"failed":1,"total":2}',
    );
    expect(dimensionNote(t, { dimension: 'evidence', score: 0.5, detail: { cited: 2, backed: false } })).toBe(
      'quality.notes.evidence_unbacked {"count":2}',
    );
    expect(dimensionNote(t, { dimension: 'feedback', score: 0, detail: { rating: 'down', reasons: [] } })).toBe(
      'quality.notes.feedback_down',
    );
  });

  it('links an answer to its chat and an RCA to its incident', () => {
    const answer: Pick<ScoredAnswer, 'targetKind' | 'targetId' | 'attribution'> = {
      targetKind: 'chat_answer',
      targetId: 'm1',
      attribution: { chatId: 'chat 1' },
    };
    expect(answerLink(answer)).toBe('/agent/chat?chat=chat%201');
    expect(answerLink({ ...answer, targetKind: 'incident_rca', targetId: 'inc-1' })).toBe(
      '/alerts/incidents/inc-1',
    );
    expect(answerLink({ ...answer, attribution: {} })).toBeNull();
  });

  it('places each day of the trend at its UTC noon and leaves days nobody evaluated empty', () => {
    const points = trendPoints([
      { day: 20_001, answers: 3, rulesMean: 0.8, evaluated: 0, evaluatorMean: null },
      { day: 20_000, answers: 2, rulesMean: 0.5, evaluated: 1, evaluatorMean: 0.25 },
    ]);
    expect(points.timestamps[0]).toBe(20_000 * 86_400_000 + 43_200_000);
    expect(points.rules).toEqual([50, 80]);
    expect(points.evaluators).toEqual([25, null]);
  });
});

describe('the quality tab', () => {
  it('names the view of a path', () => {
    expect(qualityViewOf('/agent/quality')).toBe('overview');
    expect(qualityViewOf('/agent/quality/reviews')).toBe('reviews');
    expect(qualityViewOf('/agent/quality/evaluators')).toBe('evaluators');
    expect(qualityViewOf('/agent/quality/unknown')).toBe('overview');
    expect(qualityViewOf('/agent/chat')).toBeNull();
  });
});

describe('an evaluator in the editor', () => {
  it('starts with a rubric for team chats and automations, and reads back what was saved', () => {
    const draft = newEvaluatorDraft(t);
    expect(draft.targets).toEqual(['team_chat', 'automation']);
    expect(evaluatorProblems({ ...draft, name: ' ', targets: [] })).toEqual(['name', 'targets']);

    const evaluator: Evaluator = {
      id: 'e1',
      name: 'Accuracy',
      rubric: 'Right?',
      targets: ['incident_rca', 'team_chat'],
      samplePercent: 30,
      providerId: 'p1',
      model: null,
      temperature: 0.2,
      enabled: false,
      language: 'en-us',
      createdBy: 'u1',
      updatedBy: 'u1',
      createdAt: 1,
      updatedAt: 1,
    };

    expect(inputFromDraft({ ...draftFromEvaluator(evaluator), samplePercent: 140, model: '  ' })).toEqual({
      name: 'Accuracy',
      rubric: 'Right?',
      targets: ['team_chat', 'incident_rca'],
      samplePercent: 100,
      providerId: 'p1',
      model: null,
      temperature: 0.2,
      enabled: false,
    });
  });
});
