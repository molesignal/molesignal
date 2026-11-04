import { http } from '@/lib/http';
import type { Priority, Severity } from '@/types/alerting';

import type { ConditionOperator } from './alertRouting';

export type ResponseRole = 'commander' | 'communications' | 'scribe';
export const RESPONSE_ROLES: ResponseRole[] = ['commander', 'communications', 'scribe'];

export interface WarRoom {
  platform: string;
  connectorId: string;
  chatId: string;
  url: string;
}

/** A major incident's response: who has which role, where the team talks, where it meets. */
export interface IncidentResponse {
  incidentId: string;
  declaredAt: number;
  /** Empty when the organization's rule declared it. */
  declaredBy: string | null;
  roles: Partial<Record<ResponseRole, string>>;
  warRoom: WarRoom | null;
  meetingUrl: string | null;
  updatedAt: number;
}

export interface IncidentTask {
  id: string;
  incidentId: string;
  title: string;
  assigneeId: string | null;
  done: boolean;
  doneBy: string | null;
  doneAt: number | null;
  position: number;
  createdAt: number;
}

export type MeetingMode = 'none' | 'feishu_vc' | 'tencent_meeting' | 'zoom' | 'google_meet' | 'fixed_url';
export const MEETING_MODES: MeetingMode[] = ['none', 'feishu_vc', 'tencent_meeting', 'zoom', 'google_meet', 'fixed_url'];

/** The secret each meeting needs, written once and never read back. */
export const MEETING_SECRETS: Partial<Record<MeetingMode, string>> = {
  tencent_meeting: 'tencentMeeting.secretKey',
  zoom: 'zoom.clientSecret',
  google_meet: 'googleMeet.credentials',
};

export interface ResponseSettings {
  /** The notify connector war rooms are made with: one that can make group chats. */
  warRoomConnectorId: string | null;
  meeting: MeetingMode;
  /** The fixed meeting link. */
  meetingUrl: string;
  /** The Feishu App connector Feishu meetings are booked with; empty uses the war room connector. */
  meetingConnectorId: string | null;
  tencentMeeting: { appId: string; sdkId: string; secretId: string; userid: string };
  zoom: { accountId: string; clientId: string; host: string };
  googleMeet: { organizer: string };
  checklist: string[];
  stakeholderIds: string[];
  auto: {
    enabled: boolean;
    conditions: Array<{ key: string; operator: ConditionOperator; values: string[] }>;
    minSeverity: Severity | null;
    /** At least this priority (P1 the heaviest); incidents without one never match a set bar. */
    minPriority?: Priority | null;
  };
  /** Which meeting secrets are set (read only). */
  secretsSet: string[];
  /** The connector types that can make war rooms (read only). */
  warRoomTypes: string[];
}

export interface StatusUpdate {
  impact: string;
  current: string;
  next: string;
}

const base = (incidentId: string) => `/alerts/incidents/${encodeURIComponent(incidentId)}`;

export async function get(incidentId: string): Promise<{ response: IncidentResponse | null; tasks: IncidentTask[] }> {
  const { data } = await http.get<{ response: IncidentResponse | null; tasks: IncidentTask[] }>(`${base(incidentId)}/response`);
  return data;
}

export async function declare(incidentId: string, commanderId?: string) {
  const { data } = await http.post<{ response: IncidentResponse; tasks: IncidentTask[] }>(
    `${base(incidentId)}/declare`,
    commanderId ? { commanderId: commanderId } : {},
  );
  return data;
}

export async function assignRole(incidentId: string, role: ResponseRole, userId: string | null) {
  await http.put(`${base(incidentId)}/roles`, { role, userId: userId });
}

export async function addTask(incidentId: string, title: string): Promise<IncidentTask> {
  const { data } = await http.post<IncidentTask>(`${base(incidentId)}/tasks`, { title });
  return data;
}

export async function updateTask(
  incidentId: string,
  taskId: string,
  patch: { title?: string; assigneeId?: string | null; done?: boolean },
): Promise<IncidentTask> {
  const { data } = await http.put<IncidentTask>(`${base(incidentId)}/tasks/${encodeURIComponent(taskId)}`, patch);
  return data;
}

export async function deleteTask(incidentId: string, taskId: string) {
  await http.delete(`${base(incidentId)}/tasks/${encodeURIComponent(taskId)}`);
}

export async function statusUpdate(incidentId: string, update: StatusUpdate) {
  await http.post(`${base(incidentId)}/status-updates`, update);
}

export async function settings(): Promise<ResponseSettings> {
  const { data } = await http.get<ResponseSettings>('/alerts/response/settings');
  return data;
}

/** Saves the settings; `secrets` are newly typed meeting secrets, and an empty one keeps the saved value. */
export async function saveSettings(settings: ResponseSettings, secrets: Record<string, string> = {}): Promise<ResponseSettings> {
  const { data } = await http.put<ResponseSettings>('/alerts/response/settings', { ...settings, secrets });
  return data;
}
