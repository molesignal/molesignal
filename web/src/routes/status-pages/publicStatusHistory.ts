import type {
  IncidentImpact,
  PublicStatusPageIncident,
  PublicStatusPageSnapshot,
  StatusPageLanguage,
} from '@/api/statusPages';

import type { ComponentUptime, ComponentUptimeDay } from './componentUptime';
import { isIncidentWithinHistory, statusPageHistoryDays } from './model';

const DAY_MICROS = 24 * 60 * 60 * 1_000_000;
const ARCHIVE_MONTH_COUNT = 3;

const IMPACT_SEVERITY: Record<IncidentImpact, number> = {
  critical: 4,
  major: 3,
  minor: 2,
  maintenance: 1,
};

export interface IncidentHistoryMonth {
  key: string;
  label: string;
  incidents: PublicStatusPageIncident[];
}

export interface UptimeHistoryMonth {
  key: string;
  label: string;
  leadingDays: number;
  days: Array<ComponentUptimeDay | null>;
  percentage: number | null;
}

export interface ArchiveMonthWindow {
  startIndex: number;
  endIndex: number;
  keys: string[];
  canPrevious: boolean;
  canNext: boolean;
}

export function archiveMonthWindow(
  generatedAt: number,
  historyDays: number,
  timezone: string,
  requestedEndIndex: number | null,
): ArchiveMonthWindow {
  const latestIndex = monthIndexAt(generatedAt, timezone);
  const earliestIndex = monthIndexAt(
    generatedAt - historyDays * DAY_MICROS,
    timezone,
  );
  const endIndex = Math.min(
    latestIndex,
    Math.max(earliestIndex, requestedEndIndex ?? latestIndex),
  );
  const startIndex = Math.max(
    earliestIndex,
    endIndex - ARCHIVE_MONTH_COUNT + 1,
  );

  return {
    startIndex,
    endIndex,
    keys: Array.from(
      { length: endIndex - startIndex + 1 },
      (_, index) => monthKey(startIndex + index),
    ),
    canPrevious: startIndex > earliestIndex,
    canNext: endIndex < latestIndex,
  };
}

export function formatArchiveMonth(
  index: number,
  language: StatusPageLanguage,
): string {
  const { year, month } = monthParts(index);
  return new Intl.DateTimeFormat(language, {
    year: 'numeric',
    month: 'long',
    timeZone: 'UTC',
  }).format(new Date(Date.UTC(year, month - 1, 1)));
}

export function formatArchiveMonthKey(
  key: string,
  language: StatusPageLanguage,
): string {
  const [year = 0, month = 1] = key.split('-').map(Number);
  return new Intl.DateTimeFormat(language, {
    year: 'numeric',
    month: 'long',
    timeZone: 'UTC',
  }).format(new Date(Date.UTC(year, month - 1, 1)));
}

export function incidentHistoryMonths(
  snapshot: PublicStatusPageSnapshot,
  timezone: string,
  language: StatusPageLanguage,
): IncidentHistoryMonth[] {
  const historyDays = statusPageHistoryDays(snapshot.page);
  const months = new Map<string, IncidentHistoryMonth>();

  snapshot.history
    .filter((incident) =>
      isIncidentWithinHistory(incident, snapshot.generated_at, historyDays),
    )
    .forEach((incident) => {
      const shownAt = incident.ended_at ?? incident.started_at;
      const parts = zonedDateParts(shownAt, timezone);
      const key = `${parts.year}-${twoDigits(parts.month)}`;
      const month = months.get(key) ?? {
        key,
        label: formatMonth(shownAt, timezone, language),
        incidents: [],
      };
      month.incidents.push(incident);
      months.set(key, month);
    });

  return [...months.values()]
    .map((month) => ({
      ...month,
      incidents: [...month.incidents].sort(
        (left, right) =>
          IMPACT_SEVERITY[right.impact] - IMPACT_SEVERITY[left.impact]
          || incidentShownAt(right) - incidentShownAt(left),
      ),
    }))
    .sort((left, right) => right.key.localeCompare(left.key));
}

export function uptimeHistoryMonths(
  uptime: ComponentUptime,
  timezone: string,
  language: StatusPageLanguage,
): UptimeHistoryMonth[] {
  const months = new Map<
    string,
    {
      key: string;
      label: string;
      year: number;
      month: number;
      days: Map<number, ComponentUptimeDay>;
    }
  >();

  uptime.days.forEach((day) => {
    const parts = zonedDateParts(day.startAt, timezone);
    const key = `${parts.year}-${twoDigits(parts.month)}`;
    const month = months.get(key) ?? {
      key,
      label: formatMonth(day.startAt, timezone, language),
      year: parts.year,
      month: parts.month,
      days: new Map<number, ComponentUptimeDay>(),
    };
    month.days.set(parts.day, day);
    months.set(key, month);
  });

  return [...months.values()]
    .sort((left, right) => left.key.localeCompare(right.key))
    .map((month) => {
      const totalDays = new Date(Date.UTC(month.year, month.month, 0)).getUTCDate();
      const availableDays = [...month.days.values()];
      const unavailable = availableDays.reduce(
        (total, day) => total + day.unavailableMicros,
        0,
      );
      const eligible = availableDays.reduce(
        (total, day) => total + day.eligibleMicros,
        0,
      );
      return {
        key: month.key,
        label: month.label,
        leadingDays: new Date(Date.UTC(month.year, month.month - 1, 1)).getUTCDay(),
        days: Array.from(
          { length: totalDays },
          (_, index) => month.days.get(index + 1) ?? null,
        ),
        percentage: eligible === 0
          ? null
          : Math.max(0, 100 - (unavailable / eligible) * 100),
      };
    });
}

function incidentShownAt(incident: PublicStatusPageIncident): number {
  return incident.ended_at ?? incident.started_at;
}

function formatMonth(
  timestamp: number,
  timezone: string,
  language: StatusPageLanguage,
): string {
  return new Intl.DateTimeFormat(language, {
    year: 'numeric',
    month: 'long',
    timeZone: timezone,
  }).format(new Date(Math.floor(timestamp / 1_000)));
}

function zonedDateParts(
  timestamp: number,
  timezone: string,
): { year: number; month: number; day: number } {
  const values = Object.fromEntries(
    new Intl.DateTimeFormat('en-US', {
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      timeZone: timezone,
    })
      .formatToParts(new Date(Math.floor(timestamp / 1_000)))
      .map(({ type, value }) => [type, value]),
  );
  return {
    year: Number(values.year),
    month: Number(values.month),
    day: Number(values.day),
  };
}

function twoDigits(value: number): string {
  return String(value).padStart(2, '0');
}

function monthIndexAt(timestamp: number, timezone: string): number {
  const parts = zonedDateParts(timestamp, timezone);
  return parts.year * 12 + parts.month - 1;
}

function monthKey(index: number): string {
  const { year, month } = monthParts(index);
  return `${year}-${twoDigits(month)}`;
}

function monthParts(index: number): { year: number; month: number } {
  return {
    year: Math.floor(index / 12),
    month: index % 12 + 1,
  };
}
