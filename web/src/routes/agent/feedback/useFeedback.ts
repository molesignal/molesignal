import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as feedbackApi from '@/api/agent/feedback';

/** Everything cached about one kind of target starts with this key. */
export function feedbackKindKey(kind: feedbackApi.FeedbackTargetKind) {
  return ['agent', 'feedback', kind] as const;
}

/** The ids of a lookup, without repeats and in one order, so equal sets share a cache entry. */
export function lookupIds(ids: readonly string[]): string[] {
  return [...new Set(ids.filter(Boolean))].sort();
}

/**
 * The signed-in person's ratings of these targets, by target id. One request
 * for a whole conversation rather than one per answer.
 */
export function useFeedbackMap(
  kind: feedbackApi.FeedbackTargetKind,
  ids: readonly string[],
): Map<string, feedbackApi.AgentFeedback> {
  const joined = lookupIds(ids).join(',');
  const sorted = React.useMemo(() => (joined ? joined.split(',') : []), [joined]);

  const query = useQuery({
    queryKey: [...feedbackKindKey(kind), sorted],
    queryFn: () => feedbackApi.listFeedback(kind, sorted),
    enabled: sorted.length > 0,
    retry: false,
    staleTime: 60_000,
  });

  return React.useMemo(
    () => new Map((query.data ?? []).map((feedback) => [feedback.target_id, feedback])),
    [query.data],
  );
}
