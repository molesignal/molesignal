import { http } from '@/lib/http';

/** A day off, or a make-up workday (counted as a Monday by weekly hours). */
export type DayKind = 'holiday' | 'workday';

export interface HolidayDay {
  /** `YYYY-MM-DD`, a local date read in the timezone of whatever uses the calendar. */
  date: string;
  kind: DayKind;
  name: string;
}

export interface HolidayCalendar {
  id: string;
  orgId: string;
  name: string;
  description: string;
  days: HolidayDay[];
  createdBy?: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface HolidayCalendarInput {
  name: string;
  description: string;
  days: HolidayDay[];
}

/** A place that counts by a calendar: it can't be deleted while one is left. */
export interface CalendarReference {
  kind: 'routing_rule' | 'escalation_policy' | 'schedule' | 'mute_rule' | 'notify_policy' | 'alert_pipeline';
  id: string;
  name: string;
}

const BASE = '/alerts/holiday-calendars';

const one = (id: string) => `${BASE}/${encodeURIComponent(id)}`;

export async function list(): Promise<HolidayCalendar[]> {
  const { data } = await http.get<HolidayCalendar[]>(BASE);
  return data;
}

export async function get(id: string): Promise<HolidayCalendar> {
  const { data } = await http.get<HolidayCalendar>(one(id));
  return data;
}

export async function create(input: HolidayCalendarInput): Promise<HolidayCalendar> {
  const { data } = await http.post<HolidayCalendar>(BASE, input);
  return data;
}

export async function update(id: string, input: HolidayCalendarInput): Promise<HolidayCalendar> {
  const { data } = await http.put<HolidayCalendar>(one(id), input);
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(one(id));
}

export async function references(id: string): Promise<CalendarReference[]> {
  const { data } = await http.get<CalendarReference[]>(`${one(id)}/references`);
  return data;
}

/** Reads the days of an iCalendar (ICS) file; nothing is saved. */
export async function parse(content: string): Promise<HolidayDay[]> {
  const { data } = await http.post<{ days: HolidayDay[] }>(`${BASE}/parse`, { content });
  return data.days;
}
