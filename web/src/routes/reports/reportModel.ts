import type { ReportRecipient, ScheduledReport } from '@/api/reports';

export interface ReportMetadata {
  preset: string;
  timezone: string;
  description: string;
}

export type RecipientParseError = 'empty' | 'unsupported' | 'invalid_email';

export type RecipientParseResult =
  | { recipient: ReportRecipient; error: null }
  | { recipient: null; error: RecipientParseError };

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function readString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

export function readReportMetadata(value: unknown): ReportMetadata {
  const metadata = isRecord(value) ? value : {};
  return {
    preset: readString(metadata.preset) ?? 'previous-7-days',
    timezone: readString(metadata.timezone) ?? 'Asia/Shanghai',
    description: readString(metadata.description) ?? '',
  };
}

export function buildReportMetadata(metadata: ReportMetadata): Record<string, unknown> {
  return {
    preset: metadata.preset,
    timezone: metadata.timezone,
    description: metadata.description.trim(),
  };
}

export function parseIntervalSeconds(schedule: string): number | null {
  const match = /^every:(\d+)([smhd])$/.exec(schedule.trim());
  if (!match) return null;

  const amount = Number(match[1]);
  if (!Number.isFinite(amount) || amount <= 0) return null;

  const multiplier = {
    s: 1,
    m: 60,
    h: 3_600,
    d: 86_400,
  }[match[2] ?? ''];

  return multiplier ? amount * multiplier : null;
}

/**
 * Mirrors the scheduler's current behavior: interval schedules are exact;
 * unsupported five-field cron expressions use the backend's 24-hour fallback.
 */
export function nextRunAtMicros(
  report: Pick<ScheduledReport, 'cron' | 'enabled' | 'last_run_at_micros'>,
  nowMicros = Date.now() * 1_000,
): number | null {
  if (report.enabled === false) return null;
  if (!report.last_run_at_micros) return nowMicros;

  const intervalSeconds = parseIntervalSeconds(report.cron ?? '') ?? 86_400;
  return report.last_run_at_micros + intervalSeconds * 1_000_000;
}

export function normalizeMicros(value: unknown): number | null {
  if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0) return null;
  if (value > 1e14) return value;
  if (value > 1e11) return value * 1_000;
  return value * 1_000_000;
}

export function parseRecipient(value: string): RecipientParseResult {
  const target = value.trim();
  if (!target) return { recipient: null, error: 'empty' };

  if (/^https?:\/\/\S+$/i.test(target)) {
    return { recipient: { kind: 'webhook', target }, error: null };
  }

  if (/^s3:\/\/\S+$/i.test(target)) {
    return { recipient: { kind: 's3', target }, error: null };
  }

  if (target.startsWith('#') || target.startsWith('@')) {
    return { recipient: null, error: 'unsupported' };
  }

  if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(target)) {
    return { recipient: { kind: 'email', target }, error: null };
  }

  return { recipient: null, error: 'invalid_email' };
}

export function reportSource(
  report: Pick<ScheduledReport, 'dashboard_id' | 'saved_view_id'>,
): { kind: 'dashboard' | 'saved_view'; id: string } {
  if (report.saved_view_id) return { kind: 'saved_view', id: report.saved_view_id };
  return { kind: 'dashboard', id: report.dashboard_id ?? '' };
}

export function sanitizeFilename(value: string): string {
  const withoutControlCharacters = Array.from(value)
    .map((character) => (character.charCodeAt(0) < 32 ? '-' : character))
    .join('');
  const sanitized = withoutControlCharacters
    .trim()
    .replace(/[<>:"/\\|?*]/g, '-')
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '');
  return sanitized || 'report';
}
