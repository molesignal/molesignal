import type { TFunction } from 'i18next';
import {
  BarChart3,
  CheckCircle2,
  CircleAlert,
  Database,
  FileText,
  Image,
  Search,
  ShieldCheck,
} from 'lucide-react';

import type * as reportsApi from '@/api/reports';

import {
  buildReportMetadata,
  readReportMetadata,
  reportSource,
} from './reportModel';

export type ReportTab = 'schedules' | 'history' | 'templates';
export type SourceKind = 'dashboard' | 'saved_view';
export type ReportFormat = 'pdf' | 'png' | 'csv' | 'json';
export type WorkbenchStep = 'content' | 'schedule' | 'delivery';

export interface DisplayReport {
  id: string;
  title: string;
  description: string;
  sourceKind: SourceKind;
  sourceId: string;
  cron: string;
  rangePreset: string;
  timezone: string;
  recipients: reportsApi.ReportRecipient[];
  format: ReportFormat;
  enabled: boolean;
  raw: reportsApi.ScheduledReport;
}

export interface DeliveryRow {
  delivery: reportsApi.ReportDelivery;
  report: DisplayReport;
}

export interface TemplatePreset {
  id: string;
  serverId: string | null;
  isBuiltin: boolean;
  name: string;
  description: string;
  sourceKind: SourceKind;
  format: ReportFormat;
  rangePreset: string;
  icon: 'platform' | 'rum' | 'alerts' | 'sla' | 'query';
}

export interface WorkbenchSeed {
  report: DisplayReport | null;
  template: TemplatePreset | null;
}

export interface ReportDraft {
  title: string;
  description: string;
  sourceKind: SourceKind;
  sourceId: string;
  rangePreset: string;
  cron: string;
  timezone: string;
  recipients: reportsApi.ReportRecipient[];
  format: ReportFormat;
  enabled: boolean;
}

export interface TemplateDraft {
  name: string;
  description: string;
  sourceKind: SourceKind;
  format: ReportFormat;
  rangePreset: string;
}

export const SCHEDULE_OPTIONS = [
  { value: 'every:1h', labelKey: 'schedule.hourly' },
  { value: 'every:6h', labelKey: 'schedule.every_6_hours' },
  { value: 'every:12h', labelKey: 'schedule.every_12_hours' },
  { value: 'every:24h', labelKey: 'schedule.daily' },
  { value: 'every:7d', labelKey: 'schedule.weekly' },
] as const;

export const RANGE_OPTIONS = [
  { value: 'previous-24-hours', labelKey: 'workbench.ranges.previous_24_hours' },
  { value: 'previous-7-days', labelKey: 'workbench.ranges.previous_7_days' },
  {
    value: 'previous-calendar-day',
    labelKey: 'workbench.ranges.previous_calendar_day',
  },
  {
    value: 'previous-calendar-week',
    labelKey: 'workbench.ranges.previous_calendar_week',
  },
  {
    value: 'previous-calendar-month',
    labelKey: 'workbench.ranges.previous_calendar_month',
  },
  { value: 'custom', labelKey: 'workbench.ranges.custom' },
] as const;

export const LEGACY_RANGE_OPTIONS = [
  { value: 'previous-1-hour', labelKey: 'workbench.ranges.previous_1_hour' },
  { value: 'previous-30-days', labelKey: 'workbench.ranges.previous_30_days' },
] as const;

export const FORMAT_OPTIONS: Array<{
  value: ReportFormat;
  descriptionKey: string;
  icon: typeof FileText;
}> = [
  {
    value: 'pdf',
    descriptionKey: 'workbench.formats.pdf_description',
    icon: FileText,
  },
  {
    value: 'png',
    descriptionKey: 'workbench.formats.png_description',
    icon: Image,
  },
  {
    value: 'csv',
    descriptionKey: 'workbench.formats.csv_description',
    icon: BarChart3,
  },
  {
    value: 'json',
    descriptionKey: 'workbench.formats.json_description',
    icon: Database,
  },
];

export function adaptReport(
  report: reportsApi.ScheduledReport,
): DisplayReport {
  const metadata = readReportMetadata(report.time_range_json);
  const source = reportSource(report);
  const rawFormat = report.format?.toLowerCase();
  const format: ReportFormat =
    rawFormat === 'png' || rawFormat === 'csv' || rawFormat === 'json'
      ? rawFormat
      : 'pdf';

  return {
    id: report.id,
    title: report.name || report.id,
    description: report.description?.trim() || metadata.description,
    sourceKind: source.kind,
    sourceId: source.id,
    cron: report.cron?.trim() || 'every:7d',
    rangePreset: metadata.preset,
    timezone: metadata.timezone,
    recipients: report.recipients ?? [],
    format,
    enabled: report.enabled !== false,
    raw: report,
  };
}

export function reportInputFromDisplay(
  report: DisplayReport,
  enabled: boolean,
): reportsApi.ReportInput {
  return {
    name: report.title,
    dashboard_id:
      report.sourceKind === 'dashboard' ? report.sourceId : null,
    saved_view_id:
      report.sourceKind === 'saved_view' ? report.sourceId : null,
    cron: report.cron,
    recipients: report.recipients,
    format: report.format,
    time_range_json: buildReportMetadata({
      preset: report.rangePreset,
      timezone: report.timezone,
      description: report.description,
    }),
    enabled,
  };
}

export function reportInputFromDraft(
  draft: ReportDraft,
): reportsApi.ReportInput {
  return {
    name: draft.title.trim(),
    dashboard_id: draft.sourceKind === 'dashboard' ? draft.sourceId : null,
    saved_view_id: draft.sourceKind === 'saved_view' ? draft.sourceId : null,
    cron: draft.cron.trim(),
    recipients: draft.recipients,
    format: draft.format,
    time_range_json: buildReportMetadata({
      preset: draft.rangePreset,
      timezone: draft.timezone,
      description: draft.description,
    }),
    enabled: draft.enabled,
  };
}

export function draftFromSeed(seed: WorkbenchSeed | null): ReportDraft {
  if (seed?.report) {
    return {
      title: seed.report.title,
      description: seed.report.description,
      sourceKind: seed.report.sourceKind,
      sourceId: seed.report.sourceId,
      rangePreset: seed.report.rangePreset,
      cron: seed.report.cron,
      timezone: seed.report.timezone,
      recipients: [...seed.report.recipients],
      format: seed.report.format,
      enabled: seed.report.enabled,
    };
  }
  if (seed?.template) {
    return {
      title: seed.template.name,
      description: seed.template.description,
      sourceKind: seed.template.sourceKind,
      sourceId: '',
      rangePreset: seed.template.rangePreset,
      cron: 'every:7d',
      timezone: 'Asia/Shanghai',
      recipients: [],
      format: seed.template.format,
      enabled: true,
    };
  }
  return {
    title: '',
    description: '',
    sourceKind: 'dashboard',
    sourceId: '',
    rangePreset: 'previous-7-days',
    cron: 'every:7d',
    timezone: 'Asia/Shanghai',
    recipients: [],
    format: 'pdf',
    enabled: true,
  };
}

export function validateDraft(draft: ReportDraft): string | null {
  if (!draft.title.trim()) return 'errors.name_required';
  if (!draft.sourceId) return 'errors.source_required';
  if (draft.recipients.length === 0) return 'errors.recipient_required';
  return null;
}

export function templateDraftFromPreset(
  template: TemplatePreset | null,
): TemplateDraft {
  if (template) {
    return {
      name: template.name,
      description: template.description,
      sourceKind: template.sourceKind,
      format:
        template.sourceKind === 'dashboard' || template.format === 'png'
          ? 'pdf'
          : template.format,
      rangePreset: template.rangePreset,
    };
  }
  return {
    name: '',
    description: '',
    sourceKind: 'dashboard',
    format: 'pdf',
    rangePreset: 'previous-calendar-month',
  };
}

export function getSourceName(
  report: DisplayReport,
  dashboardNames: Map<string, string>,
  savedViewNames: Map<string, string>,
): string {
  return (
    (report.sourceKind === 'dashboard'
      ? dashboardNames.get(report.sourceId)
      : savedViewNames.get(report.sourceId)) ?? report.sourceId
  );
}

export function scheduleLabel(
  cron: string,
  t: TFunction<'reports'>,
): string {
  const option = SCHEDULE_OPTIONS.find(
    (candidate) => candidate.value === cron,
  );
  return option
    ? t(option.labelKey)
    : t('schedule.custom_summary', { value: cron });
}

export function rangeLabel(
  preset: string,
  t: TFunction<'reports'>,
): string {
  const option = [...RANGE_OPTIONS, ...LEGACY_RANGE_OPTIONS].find(
    (candidate) => candidate.value === preset,
  );
  return option ? t(option.labelKey) : preset;
}

export function normalizeFormat(value: string): ReportFormat {
  const normalized = value.toLowerCase();
  if (
    normalized === 'png' ||
    normalized === 'csv' ||
    normalized === 'json'
  ) {
    return normalized;
  }
  return 'pdf';
}

export function templateIcon(icon: TemplatePreset['icon']) {
  if (icon === 'rum') return BarChart3;
  if (icon === 'alerts') return CircleAlert;
  if (icon === 'sla') return ShieldCheck;
  if (icon === 'query') return Search;
  return CheckCircle2;
}

export function builtInTemplates(
  t: TFunction<'reports'>,
): TemplatePreset[] {
  return [
    {
      id: 'builtin:platform-health',
      serverId: null,
      isBuiltin: true,
      name: t('templates.platform_health.name'),
      description: t('templates.platform_health.description'),
      sourceKind: 'dashboard',
      format: 'pdf',
      rangePreset: 'previous-calendar-week',
      icon: 'platform',
    },
    {
      id: 'builtin:rum-experience',
      serverId: null,
      isBuiltin: true,
      name: t('templates.rum_experience.name'),
      description: t('templates.rum_experience.description'),
      sourceKind: 'dashboard',
      format: 'pdf',
      rangePreset: 'previous-calendar-day',
      icon: 'rum',
    },
    {
      id: 'builtin:alert-review',
      serverId: null,
      isBuiltin: true,
      name: t('templates.alert_review.name'),
      description: t('templates.alert_review.description'),
      sourceKind: 'dashboard',
      format: 'pdf',
      rangePreset: 'previous-calendar-week',
      icon: 'alerts',
    },
    {
      id: 'builtin:sla-compliance',
      serverId: null,
      isBuiltin: true,
      name: t('templates.sla_compliance.name'),
      description: t('templates.sla_compliance.description'),
      sourceKind: 'dashboard',
      format: 'pdf',
      rangePreset: 'previous-calendar-month',
      icon: 'sla',
    },
    {
      id: 'builtin:saved-query',
      serverId: null,
      isBuiltin: true,
      name: t('templates.saved_query.name'),
      description: t('templates.saved_query.description'),
      sourceKind: 'saved_view',
      format: 'csv',
      rangePreset: 'previous-24-hours',
      icon: 'query',
    },
  ];
}
