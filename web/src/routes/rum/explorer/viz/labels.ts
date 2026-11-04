import type { TFunction } from 'i18next';

import { columnLabel } from '../labels';
import type { RumTab } from '../model';
import type { RumMeasureChoice } from './spec';

/**
 * What a number is called to a reader: the function and the column it takes, named as the table
 * names its columns. A count of rows is a count of what the type is of.
 */
export function measureLabel(t: TFunction<'rum'>, measure: RumMeasureChoice, tab: RumTab): string {
  if (!measure.field) {
    return measure.fn === 'count'
      ? t('explorer.viz.count_of', { type: t(`explorer.types.${tab}`).toLocaleLowerCase() })
      : measure.fn;
  }

  return `${measure.fn} ${columnLabel(t, measure.field)}`;
}

/** The fields the rows are told apart by, as a phrase: `page · browser`. */
export function groupsTitle(t: TFunction<'rum'>, fields: readonly string[]): string {
  return fields.map((field) => columnLabel(t, field)).join(' · ');
}

/** The line over a drawing: what is drawn, and by what the rows are grouped. */
export function vizTitle(t: TFunction<'rum'>, measure: string, groups: readonly string[]): string {
  return groups.length > 0 ? t('explorer.viz.title_by', { measure, groups: groupsTitle(t, groups) }) : measure;
}

/** What the filters of a drawing can do: the values it names are narrowed to or left out of the query. */
export interface VizFilters {
  /** Whether the query can say `field = value`. */
  can: (field: string, value: string) => boolean;
  isFiltered: (field: string, value: string) => boolean;
  /** Narrows the query to the value, or takes that filter out again. */
  toggle: (field: string, value: string) => void;
  /** Narrows the query to, or leaves out, the values of a series at once. */
  apply: (entries: ReadonlyArray<{ field: string; value: string; op: '=' | '!=' }>) => void;
}
