import { useQuery } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';

import * as qualityApi from '@/api/agent/quality';
import type { ScoreSource } from '@/api/agent/quality';

import { QUALITY_DAYS } from './model';

/** Everything the Quality tab reads lies under this key. */
export const QUALITY_KEY = ['agent', 'quality'] as const;

const DEFAULT_DAYS = 30;
const SOURCES: readonly ScoreSource[] = ['personal_chat', 'team_chat', 'automation', 'incident_rca'];

/** Below this a score waits for review while the settings are still being read. */
export const DEFAULT_THRESHOLD = 0.6;

/**
 * The period and the source the tab looks at, kept in the address so the
 * overview and the list of answers show the same ones.
 */
export function useQualityFilter() {
  const [params, setParams] = useSearchParams();
  const days =
    QUALITY_DAYS.find((choice) => String(choice) === params.get('days')) ?? DEFAULT_DAYS;
  const source = SOURCES.find((choice) => choice === params.get('source'));

  const change = (key: 'days' | 'source', value: string | null) =>
    setParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (value === null) next.delete(key);
        else next.set(key, value);
        return next;
      },
      { replace: true },
    );

  return {
    days,
    source,
    setDays: (value: number) => change('days', value === DEFAULT_DAYS ? null : String(value)),
    setSource: (value: ScoreSource | 'all') => change('source', value === 'all' ? null : value),
  };
}

/** The organization's scoring settings, and the threshold scores are coloured by. */
export function useScoringSettings() {
  const settings = useQuery({
    queryKey: [...QUALITY_KEY, 'settings'],
    queryFn: qualityApi.getSettings,
    retry: false,
  });

  return {
    settings,
    threshold: settings.data?.reviewThreshold ?? DEFAULT_THRESHOLD,
  };
}
