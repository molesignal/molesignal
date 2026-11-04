import { type AddressParams, getOrPost } from '@/lib/http';

import type { RumFilter } from './explorer';
import type { ReplayEvent } from './session';

interface MapParams {
  from?: number;
  to?: number;
  /** The application, the environment, the page, the size of the window… */
  filters?: RumFilter[];
  limit?: number;
}

export type HeatmapParams = MapParams;

interface Answered {
  /** The range the answer is of, in microseconds. */
  from: number;
  to: number;
  unavailable?: 'time_budget' | 'busy' | 'timestamp_masked';
}

/** One clicked element and how its clicks fell. */
export interface ClickedElement {
  /** What finds it in a recording of the page. */
  element: string;
  name?: string | null;
  selector?: string | null;
  clicks: number;
  sessions: number;
  /** Clicks that came with a sign of frustration. */
  frustrated: number;
  /** Where on the page its clicks landed, on average. */
  pageX?: number | null;
  pageY?: number | null;
  /** The clicks by the cell of the element they landed in: `[x, y, clicks]`, ten cells each way. */
  cells: Array<[number, number, number]>;
}

export interface ClickMap extends Answered {
  clicks: number;
  elements: ClickedElement[];
}

export interface ScrollBand {
  /** CSS pixels from the top of the page to the top of the band. */
  depth: number;
  /** The share of the views that saw it, from 0 to 1. */
  reached: number;
}

export interface ScrollMap extends Answered {
  views: number;
  pageHeight?: number | null;
  medianDepth?: number | null;
  bands: ScrollBand[];
}

export interface HeatmapCandidate {
  sessionId: string;
  viewId: string;
  atTime: number;
  width?: number | null;
  height?: number | null;
}

/** A recorded page: the events that build it, up to the moment it shows. */
export interface HeatmapSnapshot extends HeatmapCandidate {
  events: ReplayEvent[];
}

export interface HeatmapSnapshotAnswer {
  /** Null when no session that opened the page was recorded. */
  snapshot: HeatmapSnapshot | null;
  /** The views it can be drawn over, the newest first. */
  candidates: HeatmapCandidate[];
}

/** A question as its address says it: the filters are a JSON list, so that it can be sent as it stands. */
const addressOf = <T extends { filters?: RumFilter[] | undefined }>(params: T): AddressParams => {
  const { filters, ...rest } = params;

  return { ...rest, filters: filters && filters.length > 0 ? JSON.stringify(filters) : undefined } as AddressParams;
};

/** Where people click on a page. */
export async function heatmapClicks(params: HeatmapParams, signal?: AbortSignal): Promise<ClickMap> {
  return getOrPost<ClickMap>('/rum/heatmaps/clicks', { params: addressOf(params), body: params }, { ...(signal && { signal }) });
}

/** How far down a page people go. */
export async function heatmapScroll(params: HeatmapParams, signal?: AbortSignal): Promise<ScrollMap> {
  return getOrPost<ScrollMap>('/rum/heatmaps/scroll', { params: addressOf(params), body: params }, { ...(signal && { signal }) });
}

/** A recording of a page to draw a map over: of the view asked for, or the newest. */
export async function heatmapSnapshot(
  params: Omit<HeatmapParams, 'limit'> & { viewId?: string },
  signal?: AbortSignal,
): Promise<HeatmapSnapshotAnswer> {
  return getOrPost<HeatmapSnapshotAnswer>('/rum/heatmaps/snapshot', { params: addressOf(params), body: params }, { ...(signal && { signal }) });
}
