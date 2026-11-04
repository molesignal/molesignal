import type { MapRed } from '@/api/apmMap';

import { formatDuration, formatRate, formatThroughput } from '../format';
import { perSecond } from './graph';

/** What a node served or was asked over `seconds`, as one line: requests a second, failures, P95. */
export function nodeFigures(red: MapRed, seconds: number): string {
  return [formatThroughput(perSecond(red, seconds)), formatRate(red.errorRate), `P95 ${formatDuration(red.p95Time ?? undefined)}`].join(' · ');
}
