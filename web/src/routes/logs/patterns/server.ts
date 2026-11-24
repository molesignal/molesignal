import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as webApi from '@/api/web';

import { logMatchRequest } from '../cursorQuery';
import { logLevelTone, logSourceLabel, logStatement, type LogEntry, type LogLevelTone } from '../viewModel';
import { patternTokens, type LogPattern, type PatternToken } from './model';
import type { PatternSampleQuery } from './sample';

/** The most patterns the server is asked for. */
const MOST_PATTERNS = 200;
const TONE_RANK: Record<LogLevelTone, number> = { error: 3, warn: 2, info: 1, quiet: 0 };

export interface ServerPatterns {
  report: webApi.LogPatternReport | undefined;
  loading: boolean;
  failed: boolean;
  retry: () => void;
}

/**
 * The patterns of the whole range the query matches, as the server counts
 * them: from a sample spread across the range when it holds more logs than it
 * reads, each count scaled to the range, and compared with `baseline`. Without
 * a query (`null`) nothing is asked.
 */
export function useServerPatterns(query: PatternSampleQuery | null, baseline: webApi.LogPatternBaseline): ServerPatterns {
  const asked = React.useMemo(
    () =>
      query && JSON.stringify([query.orgId, query.stream, query.statement, query.pinned, query.timeRange.start, query.timeRange.end, query.ranAt]),
    [query],
  );
  const answer = useQuery({
    queryKey: ['logs', 'server-patterns', asked, baseline],
    enabled: query !== null,
    queryFn: ({ signal }) => {
      if (!query) throw new Error('No query to read patterns from.');
      const match = logMatchRequest(query.stream, query.statement, query.pinned, query.timeRange);
      return webApi.logPatterns({ ...match, baseline, limit: MOST_PATTERNS }, signal);
    },
    // The patterns of a query that has run do not change under the reader; while the same query is
    // counted against another baseline, the last count stays.
    staleTime: Infinity,
    placeholderData: (previous, previousQuery) => (previousQuery?.queryKey[2] === asked ? previous : undefined),
    retry: false,
    refetchOnWindowFocus: false,
  });

  const { refetch } = answer;
  const retry = React.useCallback(() => void refetch(), [refetch]);

  return { report: answer.data, loading: answer.isFetching, failed: answer.isError, retry };
}

/** Whether a message read into `tokens` is one of `pattern`: the same words, anything at its open places. */
export function matchesPattern(tokens: readonly PatternToken[], pattern: readonly PatternToken[]): boolean {
  if (pattern.length === 0 || tokens.length !== pattern.length) return false;

  return pattern.every((want, index) => {
    if (want.wild) return true;
    const word = tokens[index]!;
    return !word.wild && word.lead === want.lead && word.text === want.text && word.trail === want.trail;
  });
}

/**
 * The server's patterns as the view draws them: each with the logs read that
 * are of it, to open, and where they come from. The count is of the whole
 * range; the levels are the server's.
 */
export function serverPatternsOf(report: webApi.LogPatternReport, rows: readonly LogEntry[]): LogPattern[] {
  const read = rows.map((row) => {
    const statement = logStatement(row.raw);
    return statement ? patternTokens(statement.value) : null;
  });

  return report.patterns.map((pattern) => {
    const tokens = pattern.tokens ?? [];
    const matched = read.flatMap((words, index) => (words && matchesPattern(words, tokens) ? [index] : []));
    const sources = new Map<string, number>();

    for (const index of matched) {
      const source = logSourceLabel(rows[index]!.raw);
      if (source !== '—') sources.set(source, (sources.get(source) ?? 0) + 1);
    }

    const tone = pattern.levels.reduce<LogLevelTone>(
      (loudest, { level }) => (TONE_RANK[logLevelTone(level)] > TONE_RANK[loudest] ? logLevelTone(level) : loudest),
      'quiet',
    );

    return {
      key: pattern.pattern,
      tokens,
      count: Math.round(pattern.estimatedCount ?? pattern.count),
      share: pattern.share,
      tone,
      levels: pattern.levels,
      sources: [...sources.entries()]
        .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
        .map(([name, count]) => ({ name, count })),
      rows: matched,
      fragment: pattern.fragment ?? '',
      field: report.field,
      change: pattern.change,
      ratio: pattern.ratio,
      example: pattern.example,
    };
  });
}
