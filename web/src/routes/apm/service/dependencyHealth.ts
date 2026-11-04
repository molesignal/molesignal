import type { DependencySummary } from '@/api/apm';

import { OTHER_DIMENSION } from '../scope';

/** What a service asked of one target in a stretch: its operations together, the P95 of the slowest. */
export interface TargetFigures {
  requests: number;
  errors: number;
  timeouts: number;
  retries: number;
  p95: number | null;
}

export type ScoreLevel = 'good' | 'warning' | 'poor';

/** A target's health from 0 to 100, and what took points off it. */
export interface DependencyScore {
  score: number;
  level: ScoreLevel;
  reasons: Array<{ key: 'errors' | 'timeouts' | 'latency'; points: number }>;
}

export function targetFigures(operations: readonly DependencySummary[]): TargetFigures {
  return operations.reduce<TargetFigures>(
    (figures, item) => ({
      requests: figures.requests + item.red.requestCount,
      errors: figures.errors + item.red.errorCount,
      timeouts: figures.timeouts + (item.red.timeoutCount ?? 0),
      retries: figures.retries + (item.red.retryCount ?? 0),
      p95: Math.max(figures.p95 ?? 0, item.red.p95Time ?? 0) || null,
    }),
    { requests: 0, errors: 0, timeouts: 0, retries: 0, p95: null },
  );
}

/**
 * A target's health: 100, less four points a percent of failed calls (at
 * most 60), four a percent of calls that ran out of time (at most 20), and,
 * against the stretch compared with, half a point a percent its P95 grew
 * past a fifth (at most 20). From 90 it is good, from 70 worth a look.
 */
export function dependencyScore(now: TargetFigures, before?: TargetFigures | null): DependencyScore {
  const reasons: DependencyScore['reasons'] = [];
  const take = (key: DependencyScore['reasons'][number]['key'], points: number) => {
    const rounded = Math.round(points);
    if (rounded > 0) reasons.push({ key, points: rounded });
  };

  if (now.requests > 0) {
    take('errors', Math.min(60, (now.errors / now.requests) * 400));
    take('timeouts', Math.min(20, (now.timeouts / now.requests) * 400));
  }

  if (before?.p95 && now.p95) {
    const growth = now.p95 / before.p95 - 1;
    if (growth > 0.2) take('latency', Math.min(20, (growth - 0.2) * 50));
  }

  const score = Math.max(0, 100 - reasons.reduce((sum, reason) => sum + reason.points, 0));
  return { score, level: score >= 90 ? 'good' : score >= 70 ? 'warning' : 'poor', reasons };
}

/**
 * The database statements that take longest at their P95, as templates, the
 * slowest first. An operation of one word (`SELECT`) names no statement.
 */
export function slowStatements(dependencies: readonly DependencySummary[], limit = 10): DependencySummary[] {
  return dependencies
    .filter(
      (item) =>
        item.dependency.category === 'database' &&
        item.dependency.operation &&
        item.dependency.operation !== OTHER_DIMENSION &&
        /\s/.test(item.dependency.operation.trim()) &&
        item.red.requestCount > 0,
    )
    .sort((left, right) => (right.red.p95Time ?? 0) - (left.red.p95Time ?? 0))
    .slice(0, limit);
}
