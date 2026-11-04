import type { TFunction } from 'i18next';

import type {
  DailyQuality,
  DimensionScore,
  Evaluator,
  EvaluatorInput,
  EvaluatorName,
  ScoredAnswer,
  ScoreSource,
} from '@/api/agent/quality';

/** The views of the Quality tab, each at an address of its own. */
export const QUALITY_VIEWS = [
  { value: 'overview', path: '/agent/quality', key: 'quality.views.overview' },
  { value: 'answers', path: '/agent/quality/answers', key: 'quality.views.answers' },
  { value: 'reviews', path: '/agent/quality/reviews', key: 'quality.views.reviews' },
  { value: 'evaluators', path: '/agent/quality/evaluators', key: 'quality.views.evaluators' },
] as const;

export type QualityView = (typeof QUALITY_VIEWS)[number]['value'];

/** Which view of the Quality tab a path is, or null outside it. */
export function qualityViewOf(pathname: string): QualityView | null {
  const match = /^\/agent\/quality(?:\/([^/]+))?\/?$/.exec(pathname);
  if (!match) return null;
  return QUALITY_VIEWS.find((view) => view.value === match[1])?.value ?? 'overview';
}

/** The periods the tab looks back over, in days. */
export const QUALITY_DAYS = [7, 30, 90] as const;

/** A score of 0 to 1 as the 0 to 100 a reader sees; null when there is none. */
export function scorePoints(score: number | null | undefined): number | null {
  if (score === null || score === undefined || !Number.isFinite(score)) return null;
  return Math.round(Math.min(1, Math.max(0, score)) * 100);
}

export function formatScore(score: number | null | undefined): string {
  const points = scorePoints(score);
  return points === null ? '—' : String(points);
}

export type ScoreTone = 'good' | 'fair' | 'poor';

/** Below the review threshold is poor; from 0.8 up it is good. */
export function scoreTone(score: number, threshold: number): ScoreTone {
  if (score < threshold) return 'poor';
  return score >= 0.8 ? 'good' : 'fair';
}

/** The score of an answer: its evaluators' mean when evaluated, else its rules score. */
export function overallScore(answer: Pick<ScoredAnswer, 'scores'>): number | null {
  const evaluated = answer.scores.filter((score) => score.scorer !== 'rules');
  if (evaluated.length > 0) {
    return evaluated.reduce((sum, score) => sum + score.score, 0) / evaluated.length;
  }
  return answer.scores.find((score) => score.scorer === 'rules')?.score ?? null;
}

/** An evaluator's name, or a word for one that was deleted. */
export function evaluatorName(t: TFunction, evaluators: readonly EvaluatorName[], id: string): string {
  return evaluators.find((evaluator) => evaluator.id === id)?.name ?? t('quality.evaluator_deleted');
}

function count(detail: Record<string, unknown>, key: string): number {
  const value = detail[key];
  return typeof value === 'number' ? value : 0;
}

/** What a rule dimension found, in a sentence. */
export function dimensionNote(t: TFunction, dimension: DimensionScore): string {
  const detail = dimension.detail ?? {};
  switch (dimension.dimension) {
    case 'tool_success': {
      const failed = count(detail, 'failed');
      const total = count(detail, 'total');
      return failed === 0
        ? t('quality.notes.tools_ok', { count: total })
        : t('quality.notes.tools_failed', { failed, total });
    }
    case 'evidence': {
      const cited = count(detail, 'cited');
      if (cited === 0) return t('quality.notes.evidence_none');
      return detail.backed === false
        ? t('quality.notes.evidence_unbacked', { count: cited })
        : t('quality.notes.evidence', { count: cited });
    }
    case 'query_validity': {
      const invalid = count(detail, 'invalid');
      const total = count(detail, 'total');
      return invalid === 0
        ? t('quality.notes.queries_ok', { count: total })
        : t('quality.notes.queries_invalid', { invalid, total });
    }
    case 'answer_format':
      return detail.structured === true
        ? t('quality.notes.format_ok')
        : t('quality.notes.format_plain');
    case 'completion':
      return t(`quality.notes.completion.${String(detail.status ?? 'failed')}`, {
        defaultValue: t('quality.notes.completion.failed'),
      });
    case 'feedback': {
      if (detail.rating === 'up') return t('quality.notes.feedback_up');
      const reasons = Array.isArray(detail.reasons) ? (detail.reasons as string[]) : [];
      return reasons.length > 0
        ? t('quality.notes.feedback_down_reasons', {
            reasons: reasons.map((reason) => t(`feedback.reasons.${reason}`)).join(t('quality.list_separator')),
          })
        : t('quality.notes.feedback_down');
    }
  }
}

/** Where the answer can be read in full: its chat, or the incident its RCA is on. */
export function answerLink(answer: Pick<ScoredAnswer, 'targetKind' | 'targetId' | 'attribution'>): string | null {
  if (answer.targetKind === 'incident_rca') {
    return `/alerts/incidents/${encodeURIComponent(answer.targetId)}`;
  }
  return answer.attribution.chatId
    ? `/agent/chat?chat=${encodeURIComponent(answer.attribution.chatId)}`
    : null;
}

const MICROS_PER_DAY = 86_400_000_000;

/** A day of the trend at its UTC noon, in milliseconds, and its means as 0 to 100. */
export function trendPoints(daily: readonly DailyQuality[]): {
  timestamps: number[];
  rules: Array<number | null>;
  evaluators: Array<number | null>;
} {
  const sorted = [...daily].sort((a, b) => a.day - b.day);
  return {
    timestamps: sorted.map((day) => (day.day * MICROS_PER_DAY + MICROS_PER_DAY / 2) / 1_000),
    rules: sorted.map((day) => scorePoints(day.rulesMean)),
    evaluators: sorted.map((day) => (day.evaluated > 0 ? scorePoints(day.evaluatorMean) : null)),
  };
}

/** The sources an evaluator can be pointed at, in the order the form lists them. */
export const EVALUATOR_TARGETS: readonly ScoreSource[] = ['team_chat', 'automation', 'incident_rca'];

export interface EvaluatorDraft {
  name: string;
  rubric: string;
  targets: ScoreSource[];
  samplePercent: number;
  providerId: string;
  model: string;
  temperature: number;
  enabled: boolean;
}

export function newEvaluatorDraft(t: TFunction): EvaluatorDraft {
  return {
    name: t('quality.evaluators.default_name'),
    rubric: t('quality.evaluators.default_rubric'),
    targets: ['team_chat', 'automation'],
    samplePercent: 20,
    providerId: '',
    model: '',
    temperature: 0,
    enabled: true,
  };
}

export function draftFromEvaluator(evaluator: Evaluator): EvaluatorDraft {
  return {
    name: evaluator.name,
    rubric: evaluator.rubric,
    targets: [...evaluator.targets],
    samplePercent: evaluator.samplePercent,
    providerId: evaluator.providerId ?? '',
    model: evaluator.model ?? '',
    temperature: evaluator.temperature,
    enabled: evaluator.enabled,
  };
}

export function inputFromDraft(draft: EvaluatorDraft): EvaluatorInput {
  return {
    name: draft.name.trim(),
    rubric: draft.rubric.trim(),
    targets: EVALUATOR_TARGETS.filter((target) => draft.targets.includes(target)),
    samplePercent: Math.round(Math.min(100, Math.max(0, draft.samplePercent))),
    providerId: draft.providerId || null,
    model: draft.model.trim() || null,
    temperature: Math.min(1, Math.max(0, draft.temperature)),
    enabled: draft.enabled,
  };
}

/** What still keeps an evaluator from being saved. */
export function evaluatorProblems(draft: EvaluatorDraft): Array<'name' | 'rubric' | 'targets'> {
  const problems: Array<'name' | 'rubric' | 'targets'> = [];
  if (!draft.name.trim()) problems.push('name');
  if (!draft.rubric.trim()) problems.push('rubric');
  if (draft.targets.length === 0) problems.push('targets');
  return problems;
}
