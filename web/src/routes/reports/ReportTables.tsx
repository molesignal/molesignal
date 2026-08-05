import {
  Clock3,
  Download,
  HardDrive,
  Link2,
  Loader2,
  Mail,
  Share2,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type * as reportsApi from '@/api/reports';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import {
  Card,
  ChromeButton,
  DataTable,
  Dot,
  Pill,
  Td,
  Th,
  Tr,
} from '@/shell/chrome';
import { DisabledControl } from '@/shell/DisabledControl';
import { Switch } from '@/shell/ui/switch';

import { nextRunAtMicros, normalizeMicros } from './reportModel';
import {
  type DeliveryRow,
  type DisplayReport,
  getSourceName,
  scheduleLabel,
} from './reportTypes';

export function ScheduleTable({
  reports,
  dashboardNames,
  savedViewNames,
  latestDeliveryByReport,
  exportingId,
  sharingId,
  toggling,
  onExport,
  onEdit,
  onShare,
  onToggle,
}: {
  reports: DisplayReport[];
  dashboardNames: Map<string, string>;
  savedViewNames: Map<string, string>;
  latestDeliveryByReport: Map<string, reportsApi.ReportDelivery>;
  exportingId: string | null;
  sharingId: string | null;
  toggling: boolean;
  onExport: (report: DisplayReport) => void;
  onEdit: (report: DisplayReport) => void;
  onShare: (report: DisplayReport) => void;
  onToggle: (report: DisplayReport, enabled: boolean) => void;
}) {
  const { t } = useTranslation('reports');
  const scheduleAccess = useActionAccess({ permission: 'reports.schedule' });
  const shareAccess = useActionAccess({ permission: 'reports.share' });

  return (
    <Card className="overflow-hidden">
      <DataTable>
        <thead>
          <tr>
            <Th className="min-w-[260px]">{t('table.name_source')}</Th>
            <Th className="min-w-[180px]">{t('table.schedule')}</Th>
            <Th className="min-w-[170px]">{t('table.next_run')}</Th>
            <Th className="min-w-[140px]">{t('table.last_result')}</Th>
            <Th className="min-w-[190px]">{t('table.recipients')}</Th>
            <Th className="w-[130px]">{t('table.status')}</Th>
            <Th className="w-[270px] text-right">{t('table.actions')}</Th>
          </tr>
        </thead>
        <tbody>
          {reports.map((report) => {
            const sourceName = getSourceName(
              report,
              dashboardNames,
              savedViewNames,
            );
            const nextRun = nextRunAtMicros(report.raw);
            const lastDelivery = latestDeliveryByReport.get(report.id);
            return (
              <Tr key={report.id}>
                <Td>
                  <DisabledControl
                    disabled={scheduleAccess.disabled}
                    reason={scheduleAccess.reason}
                    className="max-w-full"
                  >
                    <button
                      type="button"
                      disabled={scheduleAccess.disabled}
                      aria-disabled={scheduleAccess.disabled || undefined}
                      onClick={() => onEdit(report)}
                      className="block max-w-full text-left enabled:cursor-pointer disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
                    >
                      <span className="block truncate font-sans text-sm font-bold text-tx-0">
                        {report.title}
                      </span>
                      <span className="mt-0.5 block truncate text-xs text-tx-3">
                        {t(`source.${report.sourceKind}`)} · {sourceName}
                      </span>
                    </button>
                  </DisabledControl>
                </Td>
                <Td>
                  <div className="flex items-center gap-2 text-sm text-tx-1">
                    <Clock3 className="h-3.5 w-3.5 text-tx-3" />
                    {scheduleLabel(report.cron, t)}
                  </div>
                  <div className="mt-0.5 text-xs text-tx-3">
                    {report.timezone}
                  </div>
                </Td>
                <Td className="tabular-nums">
                  {report.enabled
                    ? nextRun === null || !report.raw.last_run_at_micros
                      ? t('table.first_run')
                      : formatMicrosActive(nextRun, false)
                    : '—'}
                </Td>
                <Td>
                  {lastDelivery ? (
                    <DeliveryStatus status={lastDelivery.status} />
                  ) : (
                    <span className="text-xs text-tx-3">
                      {t('table.never')}
                    </span>
                  )}
                </Td>
                <Td>
                  <RecipientSummary recipients={report.recipients} />
                </Td>
                <Td>
                  <div className="flex items-center gap-2">
                    <Switch
                      checked={report.enabled}
                      disabled={toggling || scheduleAccess.disabled}
                      disabledReason={
                        !toggling ? scheduleAccess.reason : undefined
                      }
                      onCheckedChange={(enabled) =>
                        onToggle(report, enabled)
                      }
                      aria-label={
                        report.enabled
                          ? `${t('actions.pause')} ${report.title}`
                          : `${t('actions.enable')} ${report.title}`
                      }
                    />
                    <span className="text-xs text-tx-2">
                      {report.enabled
                        ? t('status.enabled')
                        : t('status.paused')}
                    </span>
                  </div>
                </Td>
                <Td className="text-right">
                  <div className="flex justify-end gap-1.5">
                    <ChromeButton
                      size="sm"
                      onClick={() => onShare(report)}
                      disabled={
                        sharingId === report.id || shareAccess.disabled
                      }
                      disabledReason={
                        sharingId !== report.id
                          ? shareAccess.reason
                          : undefined
                      }
                    >
                      {sharingId === report.id ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <Share2 className="h-3.5 w-3.5" />
                      )}
                      {t('actions.share')}
                    </ChromeButton>
                    <ChromeButton
                      size="sm"
                      onClick={() => onExport(report)}
                      disabled={exportingId === report.id}
                    >
                      {exportingId === report.id ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <Download className="h-3.5 w-3.5" />
                      )}
                      {t('actions.export')}
                    </ChromeButton>
                    <ChromeButton
                      size="sm"
                      disabled={scheduleAccess.disabled}
                      disabledReason={scheduleAccess.reason}
                      onClick={() => onEdit(report)}
                    >
                      {t('actions.edit')}
                    </ChromeButton>
                  </div>
                </Td>
              </Tr>
            );
          })}
        </tbody>
      </DataTable>
    </Card>
  );
}

export function HistoryTable({
  rows,
  onViewError,
}: {
  rows: DeliveryRow[];
  onViewError: (row: DeliveryRow) => void;
}) {
  const { t } = useTranslation('reports');
  return (
    <Card className="overflow-hidden">
      <DataTable>
        <thead>
          <tr>
            <Th className="min-w-[220px]">
              {t('history.columns.report')}
            </Th>
            <Th className="min-w-[180px]">
              {t('history.columns.generated_at')}
            </Th>
            <Th className="min-w-[260px]">
              {t('history.columns.recipient')}
            </Th>
            <Th className="w-[90px]">
              {t('history.columns.attempt')}
            </Th>
            <Th className="w-[130px]">
              {t('history.columns.result')}
            </Th>
            <Th className="w-[130px] text-right">
              {t('history.columns.details')}
            </Th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const attemptedAt = normalizeMicros(row.delivery.attempted_at);
            return (
              <Tr key={`${row.report.id}:${row.delivery.id}`}>
                <Td>
                  <div className="truncate font-sans text-sm font-bold text-tx-0">
                    {row.report.title}
                  </div>
                  <div className="mt-0.5 text-xs uppercase text-tx-3">
                    {row.report.format}
                  </div>
                </Td>
                <Td className="tabular-nums">
                  {attemptedAt
                    ? formatMicrosActive(attemptedAt)
                    : '—'}
                </Td>
                <Td>
                  <div className="flex items-center gap-2">
                    <RecipientIcon
                      kind={row.delivery.recipient_kind ?? 'email'}
                    />
                    <span className="truncate">
                      {row.delivery.recipient_target ?? '—'}
                    </span>
                  </div>
                </Td>
                <Td className="tabular-nums">
                  {row.delivery.attempt ?? 1}
                </Td>
                <Td>
                  <DeliveryStatus status={row.delivery.status} />
                </Td>
                <Td className="text-right">
                  <div className="flex justify-end">
                    {row.delivery.status === 'failed' ? (
                      <ChromeButton
                        size="sm"
                        onClick={() => onViewError(row)}
                      >
                        {t('actions.view_error')}
                      </ChromeButton>
                    ) : (
                      <span className="text-tx-3">—</span>
                    )}
                  </div>
                </Td>
              </Tr>
            );
          })}
        </tbody>
      </DataTable>
    </Card>
  );
}

function DeliveryStatus({ status }: { status: string | undefined }) {
  const { t } = useTranslation('reports');
  const normalized =
    status === 'sent' || status === 'failed' || status === 'pending'
      ? status
      : 'unknown';
  const tone =
    normalized === 'sent'
      ? ('green' as const)
      : normalized === 'failed'
        ? ('red' as const)
        : normalized === 'pending'
          ? ('yellow' as const)
          : ('dim' as const);
  return (
    <Pill tone={tone}>
      <Dot tone={tone} />
      {t(`status.${normalized}`)}
    </Pill>
  );
}

function RecipientSummary({
  recipients,
}: {
  recipients: reportsApi.ReportRecipient[];
}) {
  const { t } = useTranslation('reports');
  const first = recipients[0];
  if (!first) return <span className="text-tx-3">—</span>;
  return (
    <div className="flex min-w-0 items-center gap-2">
      <RecipientIcon kind={first.kind} />
      <span className="min-w-0 truncate text-sm text-tx-1">
        {first.target}
      </span>
      {recipients.length > 1 && (
        <span className="shrink-0 text-xs text-tx-3">
          {t('table.recipient_more', {
            count: recipients.length - 1,
          })}
        </span>
      )}
    </div>
  );
}

export function RecipientIcon({ kind }: { kind: string }) {
  const Icon =
    kind === 'webhook' || kind === 'slack'
      ? Link2
      : kind === 's3'
        ? HardDrive
        : Mail;
  return (
    <span className="grid h-7 w-7 shrink-0 place-items-center rounded-md border border-bd-0 bg-bg-2 text-tx-2">
      <Icon className="h-3.5 w-3.5" />
    </span>
  );
}
