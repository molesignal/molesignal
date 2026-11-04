/** The ways the rows a query matches can be drawn; the list is the one an address without `viz` opens. */
export const RUM_VIZ = ['list', 'timeseries', 'toplist', 'table', 'map', 'distribution'] as const;

export type RumViz = (typeof RUM_VIZ)[number];

/** A way of drawing that needs numbers counted over the rows: every one but the list. */
export type RumChart = Exclude<RumViz, 'list'>;

export const DEFAULT_RUM_VIZ: RumViz = 'list';

export function rumVizOf(value: string | null | undefined): RumViz | null {
  return RUM_VIZ.find((viz) => viz === value) ?? null;
}

/** The way of drawing an address names; one that names none, or an unknown one, is the list. */
export function rumVizFromParams(params: URLSearchParams): RumViz {
  return rumVizOf(params.get('viz')) ?? DEFAULT_RUM_VIZ;
}

/** What an address says about the numbers drawn, as it was written: not yet read against a type. */
export interface RumVizAddress {
  /** Measures as `fn` or `fn:field`, in order. */
  measures: string[];
  /** Fields the rows are grouped by, in order. */
  groups: string[];
  /** A bucket of time as `10s`, `5m`, `1h`, `1d`; none leaves the choice to the range. */
  bucket: string | null;
  /** How many groups are drawn. */
  top: number | null;
  /** A measure's name or a group's field: what a table is ordered by. */
  sort: string | null;
  descending: boolean | null;
  /** The number whose values a distribution is of. */
  field: string | null;
}

/** The keys of an address that belong to the fields of one type: another type has other fields. */
const FIELD_KEYS = ['m', 'by', 'sort', 'dir', 'field'] as const;

export function readRumVizAddress(params: URLSearchParams): RumVizAddress {
  const top = Number.parseInt(params.get('top') ?? '', 10);
  const direction = params.get('dir');

  return {
    measures: params.getAll('m').filter((token) => token.trim() !== ''),
    groups: params.getAll('by').filter((field) => field.trim() !== ''),
    bucket: params.get('bucket'),
    top: Number.isFinite(top) && top > 0 ? top : null,
    sort: params.get('sort'),
    descending: direction === 'desc' ? true : direction === 'asc' ? false : null,
    field: params.get('field'),
  };
}

/** The address of the way of drawing in front. The list is the one an address without `viz` opens, so it is left out. */
export function writeRumVizParams(current: URLSearchParams, viz: RumViz): URLSearchParams {
  const next = new URLSearchParams(current);

  if (viz === DEFAULT_RUM_VIZ) next.delete('viz');
  else next.set('viz', viz);

  return next;
}

/**
 * The address with what it says of the numbers changed as `patch` asks: a key that `patch`
 * names is replaced (or taken out when its value is empty), the others stay.
 */
export function writeRumVizSettings(current: URLSearchParams, patch: Partial<RumVizAddress>): URLSearchParams {
  const next = new URLSearchParams(current);
  const list = (key: string, values: readonly string[]) => {
    next.delete(key);
    for (const value of values) next.append(key, value);
  };
  const single = (key: string, value: string | number | null) => {
    if (value === null || value === '') next.delete(key);
    else next.set(key, String(value));
  };

  if (patch.measures) list('m', patch.measures);
  if (patch.groups) list('by', patch.groups);
  if ('bucket' in patch) single('bucket', patch.bucket ?? null);
  if ('top' in patch) single('top', patch.top ?? null);
  if ('sort' in patch) single('sort', patch.sort ?? null);
  if ('field' in patch) single('field', patch.field ?? null);
  if ('descending' in patch) single('dir', patch.descending === null || patch.descending === undefined ? null : patch.descending ? 'desc' : 'asc');

  return next;
}

/** The address without the fields it names: they were of the type that was left. */
export function clearRumVizFields(current: URLSearchParams): URLSearchParams {
  const next = new URLSearchParams(current);

  for (const key of FIELD_KEYS) next.delete(key);

  return next;
}

/** What a link to the page carries of how the rows are drawn: the way, and the numbers asked. */
export function copyRumVizParams(from: URLSearchParams, to: URLSearchParams): void {
  if (rumVizFromParams(from) === DEFAULT_RUM_VIZ) return;

  for (const key of ['viz', 'm', 'by', 'bucket', 'top', 'sort', 'dir', 'field']) {
    for (const value of from.getAll(key)) to.append(key, value);
  }
}
