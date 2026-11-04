import { http } from '@/lib/http';

import type { RumMeasure } from './aggregate';
import type { RumFilter, RumType } from './explorer';

/** How the explorer draws numbers that a dashboard can draw too. */
export type RumPanelChart = 'timeseries' | 'toplist' | 'table' | 'distribution';

export interface RumPanelStatementParams {
  type: RumType;
  filters?: RumFilter[];
  freeText?: string[];
  chart: RumPanelChart;
  /** A measure's `name` is what the panel calls its column. */
  measures: RumMeasure[];
  groupBy?: string[];
  /** What the panel calls the groups, in their order. */
  groupNames?: string[];
  orderBy?: string;
  order?: 'asc' | 'desc';
  limit?: number;
  /** The number a distribution is of. */
  field?: string;
  /** What a chart over time is cut into: a variable of the dashboard (`${interval}`) or a length (`15 minutes`). */
  interval?: string;
}

export interface RumPanelColumn {
  name: string;
  role: 'time' | 'group' | 'measure';
  /** The unit a number is in once shown: a duration in milliseconds. */
  unit: 'none' | 'ms' | 'bytes' | 'count' | 'score' | 'percent' | 'per_second' | 'px';
}

export interface RumPanelStatement {
  /** The stream the statement reads, which the panel names. */
  stream: string;
  /** SQL that names no time: the dashboard runs it over the range its reader picks. */
  statement: string;
  columns: RumPanelColumn[];
  /** How many rows the panel asks for. */
  limit: number;
}

/**
 * The statement a dashboard panel runs to draw what the explorer draws: the same numbers of the
 * rows its query matches, the sessions no person ran left out unless the query asks about them.
 */
export async function panelStatement(params: RumPanelStatementParams): Promise<RumPanelStatement> {
  const { data } = await http.post<RumPanelStatement>('/rum/panel-statement', params);

  return data;
}
