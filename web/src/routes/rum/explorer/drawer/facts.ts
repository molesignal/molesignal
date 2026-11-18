import type { RumRow } from '@/api/rum';

import type { RumTab } from '../model';
import { isPresent } from './entity';

/** A number a type is read by, in a tile of its own. */
export interface SummaryTile {
  field: string;
  /** A message under `explorer.`: `columns.*` as the table heads it, or `drawer.facts.*`. */
  label: string;
  /** The ink of a count above nothing, which says something went wrong. */
  alarm?: 'red' | 'orange';
}

/** Where and for whom a row happened, as a pill: one or more fields said together. */
export interface SummaryPill {
  /** A message under `explorer.`, as for a tile. */
  label: string;
  fields: readonly string[];
  /** The fields said instead when none of `fields` has a value. */
  otherwise?: readonly string[];
  /** What stands between the values of the fields. */
  separator?: string;
  /** Fields the pill says in other words: they leave the list of fields with it. */
  covers?: readonly string[];
}

export interface RumSummary {
  /** The fields the drawer is named by, the first that has a value: never the message under it. */
  title: readonly string[];
  /** What the row says, in full, set apart as a log's message is: its fields read as one line. */
  message?: readonly string[];
  tiles: readonly SummaryTile[];
  pills: readonly SummaryPill[];
  /** The flag that says the row was recorded: it offers its replay instead of being listed. */
  replay?: string;
}

/** Who the row was for and where: every type ends its pills with these. */
const WHERE: readonly SummaryPill[] = [
  { label: 'columns.user', fields: ['usr.id'] },
  { label: 'columns.browser', fields: ['browser.name', 'browser.version'] },
  { label: 'drawer.facts.os', fields: ['os.name', 'os.version'] },
  { label: 'drawer.facts.device', fields: ['device.brand', 'device.model'], otherwise: ['device.type'] },
  { label: 'drawer.facts.location', fields: ['geo.city', 'geo.country_iso_code'], separator: ' · ', covers: ['geo.country'] },
  { label: 'drawer.facts.application', fields: ['application.id', 'version'] },
  { label: 'drawer.facts.env', fields: ['env'] },
];

const PAGE: SummaryPill = { label: 'columns.page', fields: ['view.url_path'] };

/** What the drawer says of each type first: its message, its numbers, then where it happened. */
export const RUM_SUMMARY: Record<RumTab, RumSummary> = {
  sessions: {
    title: ['usr.name', 'usr.email', 'usr.id', 'usr.anonymous_id'],
    tiles: [
      { field: 'session.duration', label: 'columns.duration' },
      { field: 'session.view.count', label: 'columns.views' },
      { field: 'session.action.count', label: 'columns.actions' },
      { field: 'session.error.count', label: 'columns.errors', alarm: 'red' },
      { field: 'session.frustration.count', label: 'drawer.facts.frustrations', alarm: 'orange' },
    ],
    pills: [WHERE[0]!, { label: 'drawer.facts.email', fields: ['usr.email'] }, ...WHERE.slice(1)],
    replay: 'session.has_replay',
  },
  views: {
    title: ['view.name', 'view.url_path'],
    tiles: [
      { field: 'view.loading_time', label: 'columns.loading_time' },
      { field: 'view.performance.lcp.timestamp', label: 'columns.lcp' },
      { field: 'view.performance.inp.duration', label: 'columns.inp' },
      { field: 'view.performance.cls.score', label: 'columns.cls' },
      { field: 'view.error.count', label: 'columns.errors', alarm: 'red' },
    ],
    pills: [{ label: 'columns.page', fields: ['view.url'] }, ...WHERE],
  },
  actions: {
    title: ['action.target.name', 'action.type'],
    tiles: [
      { field: 'action.loading_time', label: 'columns.loading_time' },
      { field: 'action.error.count', label: 'columns.errors', alarm: 'red' },
      { field: 'action.resource.count', label: 'drawer.facts.resources' },
      { field: 'action.long_task.count', label: 'drawer.facts.long_tasks' },
      { field: 'action.frustration.count', label: 'drawer.facts.frustrations', alarm: 'orange' },
    ],
    pills: [
      { label: 'columns.type', fields: ['action.type'] },
      { label: 'columns.frustration', fields: ['action.frustration.type'] },
      PAGE,
      ...WHERE,
    ],
  },
  errors: {
    title: ['error.type'],
    message: ['error.message'],
    tiles: [],
    pills: [
      { label: 'columns.source', fields: ['error.source'] },
      { label: 'columns.handling', fields: ['error.handling'] },
      { label: 'drawer.facts.issue', fields: ['error.issue_id'] },
      PAGE,
      ...WHERE,
    ],
  },
  resources: {
    title: ['resource.url_host'],
    message: ['resource.method', 'resource.url'],
    tiles: [
      { field: 'resource.status_code', label: 'columns.status' },
      { field: 'resource.duration', label: 'columns.duration' },
      { field: 'resource.size', label: 'columns.size' },
      { field: 'resource.type', label: 'columns.resource_type' },
    ],
    pills: [{ label: 'drawer.facts.trace', fields: ['resource.trace_id'] }, PAGE, ...WHERE],
  },
  long_tasks: {
    title: ['view.url_path'],
    tiles: [
      { field: 'long_task.duration', label: 'columns.duration' },
      { field: 'long_task.blocking_duration', label: 'columns.blocking' },
    ],
    pills: [{ label: 'columns.entry_type', fields: ['long_task.entry_type'] }, PAGE, ...WHERE],
  },
  logs: {
    title: ['level'],
    message: ['message'],
    tiles: [],
    pills: [
      { label: 'drawer.facts.logger', fields: ['log.logger'] },
      { label: 'columns.source', fields: ['log.origin'] },
      { label: 'columns.error_type', fields: ['log.error_type'] },
      PAGE,
      ...WHERE,
    ],
  },
};

/** The field the drawer is named by, if the row has one of the type's. */
export function titleField(summary: RumSummary, row: RumRow): string | null {
  return summary.title.find((name) => isPresent(row[name])) ?? null;
}

/** The fields of a pill that say something of this row: its own, or the ones said instead; never the title's. */
export function pillFields(pill: SummaryPill, row: RumRow, title: string | null): readonly string[] {
  const present = (names: readonly string[]) => names.filter((name) => name !== title && isPresent(row[name]));
  const own = present(pill.fields);
  return own.length > 0 ? own : present(pill.otherwise ?? []);
}

/** Whether the row was recorded, by the flag of its type. */
export function isRecorded(summary: RumSummary, row: RumRow): boolean {
  const flag = summary.replay ? row[summary.replay] : undefined;
  return flag === true || flag === 'true';
}

/** What the row says in full, if its type has a message and the row has it. */
export function messageOf(summary: RumSummary, row: RumRow): string | null {
  const parts = (summary.message ?? []).map((name) => row[name]).filter(isPresent).map(String);
  return parts.length > 0 ? parts.join(' ') : null;
}

/** The fields the drawer has said above its list of fields: the list does not say them again. */
export function summaryFields(summary: RumSummary, row: RumRow): Set<string> {
  const said = new Set<string>();
  const add = (name: string) => {
    if (isPresent(row[name])) said.add(name);
  };
  const title = titleField(summary, row);

  if (title) said.add(title);
  if (summary.replay && isRecorded(summary, row)) said.add(summary.replay);
  (summary.message ?? []).forEach(add);
  summary.tiles.forEach((tile) => add(tile.field));
  summary.pills.forEach((pill) => {
    const shown = pillFields(pill, row, title);

    shown.forEach(add);
    if (shown.length > 0) (pill.covers ?? []).forEach(add);
  });

  return said;
}
