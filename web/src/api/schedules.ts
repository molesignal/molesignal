import { http } from '@/lib/http';
import type { Schedule, ScheduleCoverage } from '@/types/alerting';

/** Create/update payload — identity and audit fields are set by the backend. */
export type ScheduleInput = Pick<
  Schedule,
  | 'name'
  | 'description'
  | 'teamId'
  | 'timezone'
  | 'enabled'
  | 'rotations'
  | 'overrides'
  | 'swapNeedsApproval'
>;

/** Add-override payload — mirrors the backend `OverrideReq` (micros fields). */
export interface OverrideInput {
  userId: string;
  startAt: number;
  endAt: number;
  reason?: string;
}

export async function list(): Promise<Schedule[]> {
  const { data } = await http.get<Schedule[]>('/schedules');
  return data;
}
export async function get(id: string): Promise<Schedule> {
  const { data } = await http.get<Schedule>(`/schedules/${id}`);
  return data;
}
export async function create(s: ScheduleInput): Promise<Schedule> {
  const { data } = await http.post<Schedule>('/schedules', s);
  return data;
}
export async function update(id: string, s: ScheduleInput): Promise<Schedule> {
  const { data } = await http.put<Schedule>(`/schedules/${id}`, s);
  return data;
}
export async function remove(id: string) {
  await http.delete(`/schedules/${id}`);
}
/** Append an override; the backend returns the full updated schedule. */
export async function addOverride(id: string, ov: OverrideInput): Promise<Schedule> {
  const { data } = await http.post<Schedule>(`/schedules/${id}/overrides`, ov);
  return data;
}
export async function removeOverride(id: string, overrideId: string): Promise<Schedule> {
  const { data } = await http.delete<Schedule>(`/schedules/${id}/overrides/${overrideId}`);
  return data;
}
export async function updateOverride(
  id: string,
  overrideId: string,
  ov: OverrideInput,
): Promise<Schedule> {
  const { data } = await http.put<Schedule>(
    `/schedules/${id}/overrides/${overrideId}`,
    ov,
  );
  return data;
}
/** Who is on call when in `[from, to)` (micros, at most 90 days), as the server works it out. */
export async function coverage(id: string, from: number, to: number): Promise<ScheduleCoverage> {
  const { data } = await http.get<ScheduleCoverage>(`/schedules/${id}/coverage`, {
    params: { from, to },
  });
  return data;
}

/** The coverage an unsaved schedule would have, for the editor's preview. */
export type CoveragePreviewInput = Pick<Schedule, 'timezone' | 'enabled' | 'rotations' | 'overrides'> & {
  from?: number;
  to?: number;
};

export async function previewCoverage(input: CoveragePreviewInput): Promise<ScheduleCoverage> {
  const { data } = await http.post<ScheduleCoverage>('/schedules/coverage-preview', input);
  return data;
}

export async function whoIsOnCall(id: string, at?: number): Promise<{ userId: string | null }> {
  const { data } = await http.get<{ userId: string | null }>(`/schedules/${id}/on-call`, {
    params: at ? { at } : undefined,
  });
  return data;
}

/** A calendar subscription: someone's own shifts in every schedule, or all of one schedule's. */
export interface CalendarFeed {
  id: string;
  /** Empty for the person's own shifts. */
  scheduleId: string | null;
  scheduleName: string | null;
  createdAt: number;
  lastUsedAt: number | null;
}

/** A feed just made or reset: its address is given only now. */
export interface IssuedCalendarFeed {
  feed: CalendarFeed;
  path: string;
  /** Set when the instance knows its external address; otherwise the page's own origin is used. */
  url: string | null;
}

export async function calendarFeeds(): Promise<CalendarFeed[]> {
  const { data } = await http.get<{ feeds: CalendarFeed[] }>('/schedules/calendar-feeds');
  return data.feeds;
}

/** Makes the feed, or gives the existing one a new address (the old one stops working). */
export async function createCalendarFeed(scheduleId?: string): Promise<IssuedCalendarFeed> {
  const { data } = await http.post<IssuedCalendarFeed>(
    '/schedules/calendar-feeds',
    scheduleId ? { scheduleId: scheduleId } : {},
  );
  return data;
}

export async function deleteCalendarFeed(id: string) {
  await http.delete(`/schedules/calendar-feeds/${encodeURIComponent(id)}`);
}

export type SwapStatus =
  | 'pending'
  | 'awaiting_approval'
  | 'accepted'
  | 'declined'
  | 'rejected'
  | 'cancelled'
  | 'expired';

/** Someone on call asking a member of the schedule to take a stretch of their shift. */
export interface SwapRequest {
  id: string;
  scheduleId: string;
  scheduleName: string | null;
  requesterId: string;
  targetId: string;
  startAt: number;
  endAt: number;
  note: string;
  status: SwapStatus;
  /** The override added when the swap took effect. */
  overrideId: string | null;
  decidedBy: string | null;
  decidedAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface SwapInput {
  targetId: string;
  startAt: number;
  endAt: number;
  note?: string;
}

/** accept / decline are the asked person's, cancel the asker's, approve / reject a manager's. */
export type SwapAnswer = 'accept' | 'decline' | 'cancel' | 'approve' | 'reject';

/** The swaps I asked for and those asking me. */
export async function mySwaps(): Promise<SwapRequest[]> {
  const { data } = await http.get<{ swaps: SwapRequest[] }>('/schedules/swaps');
  return data.swaps;
}

export async function swaps(scheduleId: string): Promise<SwapRequest[]> {
  const { data } = await http.get<{ swaps: SwapRequest[] }>(
    `/schedules/${encodeURIComponent(scheduleId)}/swaps`,
  );
  return data.swaps;
}

export async function requestSwap(scheduleId: string, input: SwapInput): Promise<SwapRequest> {
  const { data } = await http.post<SwapRequest>(
    `/schedules/${encodeURIComponent(scheduleId)}/swaps`,
    input,
  );
  return data;
}

export async function answerSwap(
  scheduleId: string,
  swapId: string,
  answer: SwapAnswer,
): Promise<SwapRequest> {
  const { data } = await http.post<SwapRequest>(
    `/schedules/${encodeURIComponent(scheduleId)}/swaps/${encodeURIComponent(swapId)}/${answer}`,
  );
  return data;
}
