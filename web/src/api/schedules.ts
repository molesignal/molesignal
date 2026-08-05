import { http } from '@/lib/http';
import type { Schedule } from '@/types/alerting';

/** Create/update payload — identity and audit fields are set by the backend. */
export type ScheduleInput = Pick<
  Schedule,
  | 'name'
  | 'description'
  | 'team_id'
  | 'timezone'
  | 'enabled'
  | 'rotations'
  | 'overrides'
>;

/** Add-override payload — mirrors the backend `OverrideReq` (micros fields). */
export interface OverrideInput {
  user_id: string;
  start_at_micros: number;
  end_at_micros: number;
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
export async function whoIsOnCall(id: string, at?: number): Promise<{ user_id: string | null }> {
  const { data } = await http.get<{ user_id: string | null }>(`/schedules/${id}/on-call`, {
    params: at ? { at } : undefined,
  });
  return data;
}
