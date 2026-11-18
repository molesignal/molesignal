import type { RuleGuide, RuleGuideQuery } from '@/types/alertRuleGuide';

/** The limits the backend holds a guide to. */
export const GUIDE_LIMITS = {
  stepsChars: 8_000,
  queries: 10,
  queryNameChars: 120,
  statementChars: 4_000,
  dashboards: 5,
} as const;

export type GuideLanguage = RuleGuideQuery['language'];
export type GuideStreamType = 'logs' | 'traces' | 'metrics';

/** The kinds of stream a SQL query of a guide can read, in the order the editor offers them. */
export const GUIDE_STREAM_TYPES: GuideStreamType[] = ['logs', 'traces', 'metrics'];

/** One query of a guide while it is written; `key` tells the rows apart while they change. */
export interface GuideQueryDraft {
  key: string;
  name: string;
  language: GuideLanguage;
  streamName: string;
  streamType: GuideStreamType;
  statement: string;
}

export interface GuideDraft {
  steps: string;
  queries: GuideQueryDraft[];
  dashboards: string[];
}

export const EMPTY_GUIDE_DRAFT: GuideDraft = { steps: '', queries: [], dashboards: [] };

/** What keeps a guide from being saved; `index` is the query's place in the list. */
export type GuideProblem =
  | { kind: 'steps_too_long'; limit: number }
  | { kind: 'too_many_queries'; limit: number }
  | { kind: 'query_name_required'; index: number }
  | { kind: 'query_name_too_long'; index: number; limit: number }
  | { kind: 'query_statement_required'; index: number }
  | { kind: 'query_statement_too_long'; index: number; limit: number }
  | { kind: 'query_stream_required'; index: number }
  | { kind: 'too_many_dashboards'; limit: number };

let lastKey = 0;

function nextKey(): string {
  lastKey += 1;
  return `guide-query-${lastKey}`;
}

/** Characters as the backend counts them: code points, not UTF-16 units. */
export function charCount(text: string): number {
  return [...text].length;
}

/** A new, empty query of the given language. */
export function newGuideQuery(language: GuideLanguage = 'promql'): GuideQueryDraft {
  return { key: nextKey(), name: '', language, streamName: '', streamType: 'logs', statement: '' };
}

/** The draft of a saved guide; an absent guide is an empty draft. */
export function guideDraftOf(guide: RuleGuide | null | undefined): GuideDraft {
  if (!guide) return { ...EMPTY_GUIDE_DRAFT, queries: [], dashboards: [] };
  return {
    steps: guide.steps ?? '',
    queries: (guide.queries ?? []).map((query) => ({
      key: nextKey(),
      name: query.name,
      language: query.language,
      streamName: query.stream?.name ?? '',
      streamType: query.stream?.streamType ?? 'logs',
      statement: query.statement,
    })),
    dashboards: [...(guide.dashboards ?? [])],
  };
}

/** The first thing that keeps the guide from being saved, or `null` when it can be saved. */
export function guideProblem(draft: GuideDraft): GuideProblem | null {
  if (charCount(draft.steps.trim()) > GUIDE_LIMITS.stepsChars) {
    return { kind: 'steps_too_long', limit: GUIDE_LIMITS.stepsChars };
  }
  if (draft.queries.length > GUIDE_LIMITS.queries) {
    return { kind: 'too_many_queries', limit: GUIDE_LIMITS.queries };
  }
  for (const [index, query] of draft.queries.entries()) {
    const name = charCount(query.name.trim());
    if (name === 0) return { kind: 'query_name_required', index };
    if (name > GUIDE_LIMITS.queryNameChars) {
      return { kind: 'query_name_too_long', index, limit: GUIDE_LIMITS.queryNameChars };
    }
    const statement = charCount(query.statement.trim());
    if (statement === 0) return { kind: 'query_statement_required', index };
    if (statement > GUIDE_LIMITS.statementChars) {
      return { kind: 'query_statement_too_long', index, limit: GUIDE_LIMITS.statementChars };
    }
    if (query.language === 'sql' && !query.streamName.trim()) {
      return { kind: 'query_stream_required', index };
    }
  }
  if (new Set(draft.dashboards).size > GUIDE_LIMITS.dashboards) {
    return { kind: 'too_many_dashboards', limit: GUIDE_LIMITS.dashboards };
  }
  return null;
}

/**
 * The guide as it is saved: trimmed, a PromQL query without the stream it may
 * have had while it was SQL, each dashboard once. An empty draft is an empty
 * guide, which clears the one the rule had.
 */
export function guidePayload(draft: GuideDraft): RuleGuide {
  const steps = draft.steps.trim();
  const queries = draft.queries.map((query): RuleGuideQuery => {
    const streamName = query.streamName.trim();
    return {
      name: query.name.trim(),
      language: query.language,
      ...(query.language === 'sql' && streamName
        ? { stream: { name: streamName, streamType: query.streamType } }
        : {}),
      statement: query.statement.trim(),
    };
  });
  const dashboards = [...new Set(draft.dashboards.map((id) => id.trim()).filter(Boolean))];
  return {
    ...(steps ? { steps } : {}),
    ...(queries.length > 0 ? { queries } : {}),
    ...(dashboards.length > 0 ? { dashboards } : {}),
  };
}
