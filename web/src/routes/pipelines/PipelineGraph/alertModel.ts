import type { ScheduledPipeline } from '@/api/pipelines';
import type { TimeCondition, WeeklyRange } from '@/types/alerting';

import type { TransformStep } from './model';
import { processorOf, stepObject, stringsFrom, wholeNumber } from './savedValues';

/**
 * When an alert pipeline runs: before the incident opens, changing the incident,
 * or before a notification policy sends, changing only what that policy sends.
 */
export type AlertStage = 'open' | 'notify';

/** Where an alert event comes from (`AlertEventSource`). */
export type AlertSourceKind = 'rule' | 'external' | 'synthetic' | 'no_data' | 'manual';
export const ALERT_SOURCES: readonly AlertSourceKind[] = ['rule', 'external', 'synthetic', 'no_data'];
/** An incident opened by hand opens as it was written; what a policy sends about it can still change. */
export const NOTIFY_ALERT_SOURCES: readonly AlertSourceKind[] = [...ALERT_SOURCES, 'manual'];

export type AlertSeverity = 'critical' | 'error' | 'warning' | 'info';
export const ALERT_SEVERITIES: readonly AlertSeverity[] = ['critical', 'error', 'warning', 'info'];

export interface AlertLabel {
  key: string;
  value: string;
}

/**
 * Which alert events an alert pipeline is for (each list empty: every one),
 * and its place among the organization's alert pipelines (lowest first).
 */
export interface AlertConditions {
  stage: AlertStage;
  /** The notification policies a pipeline before notifications is for; empty is every one. */
  policies: string[];
  sources: AlertSourceKind[];
  severities: AlertSeverity[];
  labels: AlertLabel[];
  /** When the event must open: inside or outside some weekly hours; null is any time. */
  time: TimeCondition | null;
  order: number;
}

export const DEFAULT_ALERT_CONDITIONS: AlertConditions = {
  stage: 'open',
  policies: [],
  sources: [],
  severities: [],
  labels: [],
  time: null,
  order: 0,
};

/** The annotation an AI summary step writes unless it names another. */
export const DEFAULT_SUMMARY_ANNOTATION = 'ai_summary';

/** What a query enrich step runs, and the annotation its rows go into (`EnrichQuery`). */
export interface EnrichQuerySettings {
  language: 'sql' | 'promql';
  stream: string;
  streamType: 'logs' | 'metrics' | 'traces';
  /** `{{labels.host}}` stands for a value of the event, inside a string of the statement. */
  statement: string;
  windowSecs: number;
  maxRows: number;
  annotation: string;
}

export const DEFAULT_QUERY_ANNOTATION = 'query_result';
export const DEFAULT_QUERY_WINDOW_SECS = 300;
export const DEFAULT_QUERY_ROWS = 5;
export const MAX_QUERY_WINDOW_SECS = 86_400;
export const MAX_QUERY_ROWS = 50;

export const defaultEnrichQuery: EnrichQuerySettings = {
  language: 'sql',
  stream: '',
  streamType: 'logs',
  statement: '',
  windowSecs: DEFAULT_QUERY_WINDOW_SECS,
  maxRows: DEFAULT_QUERY_ROWS,
  annotation: DEFAULT_QUERY_ANNOTATION,
};

/** Whether MoleSignal sets this label itself and goes by it (`is_own_label`): no step changes it. */
export function isOwnAlertLabel(key: string): boolean {
  return (
    key.startsWith('alert.') ||
    key.startsWith('__molesignal') ||
    key === 'external.kind' ||
    key === 'synthetic.state' ||
    key === 'discovery.finding' ||
    key === 'discovery.kind' ||
    key === 'discovery.subject'
  );
}

/** Whether a step goes outside MoleSignal, which only happens when an incident opens. */
export function goesOutside(step: TransformStep): boolean {
  return step.kind === 'ai_summary' || step.kind === 'http_enrich' || step.kind === 'query_enrich';
}

/** When a saved alert pipeline runs. */
export function alertStageOf(pipeline: ScheduledPipeline | null | undefined): AlertStage {
  return stepObject(stepObject(pipeline?.functionSteps).alert).stage === 'notify' ? 'notify' : 'open';
}

/** What a step of an alert pipeline saves, by its kind. */
export function alertStepInput(step: TransformStep) {
  switch (step.kind) {
    case 'ai_summary':
      return {
        kind: 'ai_summary',
        prompt: step.prompt ?? '',
        annotation: step.annotation?.trim() || DEFAULT_SUMMARY_ANNOTATION,
      };
    case 'http_enrich':
      return { kind: 'http_enrich', connectorId: step.connectorId ?? '' };
    case 'query_enrich': {
      const query = step.query ?? defaultEnrichQuery;

      return {
        kind: 'query_enrich',
        language: query.language,
        stream: query.stream.trim(),
        streamType: query.streamType,
        statement: query.statement,
        windowSecs: query.windowSecs,
        maxRows: query.maxRows,
        annotation: query.annotation.trim() || DEFAULT_QUERY_ANNOTATION,
      };
    }
    default:
      return {
        kind: 'vrl',
        script: step.script,
        ...(processorOf(step) && { processor: processorOf(step) }),
      };
  }
}

/** A saved query enrich step; what it does not say reads as its default. */
export function enrichQueryOf(raw: Record<string, unknown>): EnrichQuerySettings {
  return {
    language: raw.language === 'promql' ? 'promql' : 'sql',
    stream: typeof raw.stream === 'string' ? raw.stream : '',
    streamType: raw.streamType === 'metrics' || raw.streamType === 'traces' ? raw.streamType : 'logs',
    statement: typeof raw.statement === 'string' ? raw.statement : '',
    windowSecs: wholeNumber(raw.windowSecs, DEFAULT_QUERY_WINDOW_SECS, MAX_QUERY_WINDOW_SECS),
    maxRows: wholeNumber(raw.maxRows, DEFAULT_QUERY_ROWS, MAX_QUERY_ROWS),
    annotation: typeof raw.annotation === 'string' ? raw.annotation : DEFAULT_QUERY_ANNOTATION,
  };
}

/** The conditions of a saved alert pipeline; what is not one reads as every event. */
export function alertConditions(value: unknown): AlertConditions {
  const raw = stepObject(value);
  const labels = stepObject(raw.labels);

  return {
    stage: raw.stage === 'notify' ? 'notify' : 'open',
    policies: stringsFrom(raw.policies),
    sources: stringsFrom(raw.sources).filter((item): item is AlertSourceKind =>
      (NOTIFY_ALERT_SOURCES as readonly string[]).includes(item),
    ),
    severities: stringsFrom(raw.severities).filter((item): item is AlertSeverity =>
      (ALERT_SEVERITIES as readonly string[]).includes(item),
    ),
    labels: Object.entries(labels)
      .filter(([, labelValue]) => typeof labelValue === 'string')
      .map(([key, labelValue]) => ({ key, value: labelValue as string })),
    time: timeCondition(raw.time),
    order: typeof raw.order === 'number' && Number.isInteger(raw.order) ? raw.order : 0,
  };
}

/** A saved time condition; anything else reads as any time. */
function timeCondition(value: unknown): TimeCondition | null {
  const raw = stepObject(value);

  if ((raw.mode !== 'inside' && raw.mode !== 'outside') || typeof raw.timezone !== 'string') return null;
  if (!Array.isArray(raw.ranges)) return null;

  return { mode: raw.mode, timezone: raw.timezone, ranges: raw.ranges as WeeklyRange[] };
}
