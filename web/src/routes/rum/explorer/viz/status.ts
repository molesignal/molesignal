import type { TFunction } from 'i18next';

import type { RumAggregate, RumDistribution } from '@/api/rum';

import { countryValues, seriesGroups } from './answer';
import { bucketText, spanText } from './format';
import type { RumVizRequest, RumVizSpec } from './spec';

/**
 * The line beside the way of drawing that says what is drawn: the range, how long a point is, how
 * many groups. An answer that could not be counted says so here, and the drawing says what to do.
 */
export function vizStatus(
  t: TFunction<'rum'>,
  language: string,
  {
    spec,
    request,
    answer,
    distribution,
    range,
  }: {
    spec: RumVizSpec;
    request: RumVizRequest;
    answer: RumAggregate | undefined;
    distribution?: RumDistribution | undefined;
    range: { from: number; to: number };
  },
): string {
  if (spec.viz === 'distribution') {
    if (!distribution) return '';
    if (distribution.unavailable) return t('explorer.viz.status.unavailable');

    return [spanText(range.to - range.from, language), t('explorer.viz.status.rows', { count: distribution.rows })].join(' · ');
  }

  if (!answer) return '';
  if (answer.unavailable) return t('explorer.viz.status.unavailable');

  const parts = [spanText(range.to - range.from, language)];

  if (spec.viz === 'timeseries' && request.bars) {
    parts.push(t('explorer.viz.status.per_point', { bucket: bucketText(request.bars.binMicros, language) }));
    if (spec.groups.length > 0) parts.push(t('explorer.viz.status.groups', { count: seriesGroups(answer) }));
  } else if (spec.viz === 'map') {
    parts.push(t('explorer.viz.status.countries', { count: countryValues(answer).length }));
  } else {
    // A list that fills what was asked for is the first of more.
    const count = answer.rows.length;
    parts.push(t(count >= spec.top ? 'explorer.viz.status.top_groups' : 'explorer.viz.status.groups', { count }));
  }

  return parts.join(' · ');
}
