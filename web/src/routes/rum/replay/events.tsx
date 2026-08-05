import {
  AlertTriangle,
  Circle,
  Globe2,
  MousePointerClick,
  Network,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ReplayEvent, SessionEvent } from '@/api/rum';

import { formatDurationMs } from '../_helpers';

export interface PlayerEvent {
  key: string;
  type: string;
  timestamp: number;
  label: string;
  url?: string;
  detail?: string;
  raw: Record<string, unknown>;
}

export function normalizePlayerEvents(
  replayEvents: ReplayEvent[],
  actionEvents: SessionEvent[],
): PlayerEvent[] {
  const replay = replayEvents.flatMap((event, index) => {
    const raw = event as Record<string, unknown>;
    const type = stringValue(raw.type);
    if (!type) return [];
    const timestamp = numeric(raw.ts) ?? numeric(raw.timestamp) ?? index;
    const url =
      stringValue(raw.href) ??
      stringValue(raw.url) ??
      stringValue(asRecord(raw.payload).path);
    const detail =
      stringValue(raw.selector) ??
      stringValue(raw.name) ??
      stringValue(asRecord(raw.payload).selector);
    const item: PlayerEvent = {
      key: `replay-${timestamp}-${index}`,
      type,
      timestamp,
      label: detail || type,
      raw,
    };
    if (url) item.url = url;
    if (detail) item.detail = detail;
    return [item];
  });
  const actions = actionEvents
    .map((event, index) => {
      const item: PlayerEvent = {
        key: `action-${event.ts_micros}-${index}`,
        type: event.type,
        timestamp: event.ts_micros / 1_000,
        label: event.name ?? event.type,
        raw: event.payload,
      };
      if (event.url) item.url = event.url;
      const details = [
        event.status ? `HTTP ${event.status}` : undefined,
        event.duration_ms != null ? formatDurationMs(event.duration_ms) : undefined,
        stringValue(event.payload.selector),
      ].filter(Boolean);
      if (details.length > 0) item.detail = details.join(' · ');
      return item;
    });

  const seen = new Set(
    replay.map((event) => eventIdentity(event.type, event.timestamp, event.label)),
  );
  return [
    ...replay,
    ...actions.filter(
      (event) => !seen.has(eventIdentity(event.type, event.timestamp, event.label)),
    ),
  ].sort((a, b) => a.timestamp - b.timestamp);
}

export function EventTimeline({
  events,
  sessionStart,
}: {
  events: PlayerEvent[];
  sessionStart?: number | undefined;
}) {
  const { t } = useTranslation('rum');
  if (events.length === 0) {
    return (
      <div className="grid min-h-32 place-items-center text-sm text-tx-3">
        {t('session_detail.no_events')}
      </div>
    );
  }
  return (
    <div className="mt-1 divide-y divide-bd-0">
      {events.map((event, index) => (
        <div
          key={event.key}
          className="grid min-h-[58px] grid-cols-[84px_24px_minmax(0,1fr)_minmax(120px,220px)] items-center gap-3 py-2.5"
        >
          <span className="font-mono text-xs text-tx-3">
            {formatOffset(event.timestamp, sessionStart)}
          </span>
          <span className="relative grid h-6 w-6 place-items-center">
            {index < events.length - 1 && (
              <span className="absolute left-1/2 top-5 h-[42px] w-px -translate-x-1/2 bg-bd-0" />
            )}
            <EventIcon type={event.type} />
          </span>
          <span className="min-w-0">
            <span className="block truncate text-sm font-strong text-tx-0">{event.label}</span>
            {event.detail && (
              <span className="mt-1 block truncate font-mono text-xs text-tx-3">
                {event.detail}
              </span>
            )}
          </span>
          <span className="truncate text-right font-mono text-xs text-tx-3">
            {event.url ?? '—'}
          </span>
        </div>
      ))}
    </div>
  );
}

function EventIcon({ type }: { type: string }) {
  const normalized = type.toLowerCase().replace(/[-\s]/g, '_');
  const iconClass = 'h-3.5 w-3.5';
  if (normalized.includes('error') || normalized === 'crash') {
    return (
      <span className="grid h-6 w-6 place-items-center rounded-full bg-red-dim text-red-soft">
        <AlertTriangle className={iconClass} />
      </span>
    );
  }
  if (normalized.includes('click')) {
    return (
      <span className="grid h-6 w-6 place-items-center rounded-full bg-yellow-dim text-yellow-soft">
        <MousePointerClick className={iconClass} />
      </span>
    );
  }
  if (normalized === 'resource' || normalized.includes('network')) {
    return (
      <span className="grid h-6 w-6 place-items-center rounded-full bg-blue-dim text-blue-soft">
        <Network className={iconClass} />
      </span>
    );
  }
  if (normalized === 'view' || normalized === 'snapshot' || normalized === 'navigation') {
    return (
      <span className="grid h-6 w-6 place-items-center rounded-full bg-indigo-dim text-indigo-soft">
        <Globe2 className={iconClass} />
      </span>
    );
  }
  return (
    <span className="grid h-6 w-6 place-items-center rounded-full bg-bg-3 text-tx-2">
      <Circle className={iconClass} />
    </span>
  );
}

function formatOffset(timestamp: number, sessionStart?: number): string {
  const startMs = sessionStart ? sessionStart / 1_000 : timestamp;
  const seconds = Math.max(0, (timestamp - startMs) / 1_000);
  return `+${seconds.toFixed(seconds < 10 ? 1 : 0)}s`;
}

function numeric(value: unknown): number | undefined {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string') {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return undefined;
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined;
}

function asRecord(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function eventIdentity(type: string, timestamp: number, label: string): string {
  return `${type.toLowerCase()}\0${Math.round(timestamp)}\0${label}`;
}
