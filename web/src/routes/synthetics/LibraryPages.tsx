import type { TFunction } from 'i18next';
import {
  ArrowRight,
  BellRing,
  Braces,
  Clock3,
  Cookie,
  FileJson,
  Gauge,
  Globe2,
  RadioTower,
  Regex,
  ShieldCheck,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';

import { KindLabel, Section, SyntheticsPage, WorkspaceBoundary } from './components';
import { useSyntheticsWorkspace, type CheckRow } from './data';
import { formatRelativeTimestamp, scheduleText } from './model';

export function Schedules() {
  const { t, i18n } = useTranslation('synthetics');
  const navigate = useNavigate();
  const workspace = useSyntheticsWorkspace({ resultLimit: 1 });
  return (
    <SyntheticsPage title={t('schedules.title')} subtitle={t('schedules.subtitle')}>
      <WorkspaceBoundary pending={workspace.pending} error={workspace.error} onRetry={() => void workspace.refetch()}>
        <Section title={t('schedules.title')} description={t('schedules.subtitle')}>
          <div className="overflow-x-auto">
            <DataTable
              rows={workspace.rows}
              columns={scheduleColumns(i18n.language, t)}
              rowKey={(row) => row.monitor.id}
              onRowClick={(row) => navigate(`/synthetics/checks/${row.monitor.id}/configuration`)}
              emptyLabel={t('states.empty_title')}
              className="min-w-[880px]"
            />
          </div>
        </Section>
      </WorkspaceBoundary>
    </SyntheticsPage>
  );
}

const ASSERTION_TEMPLATES = [
  ['response_time', Gauge, 'duration_ms < 500', 'latency'],
  ['http_code', Braces, 'status == 200', 'status'],
  ['json', FileJson, '$.data.success == true', 'json'],
  ['regex', Regex, 'body matches pattern', 'regex'],
  ['header', ShieldCheck, 'header:content-type exists', 'header'],
  ['cookie', Cookie, 'header:set-cookie exists', 'cookie'],
  ['tls', ShieldCheck, 'certificate valid > 30 days', 'tls'],
  ['dns', Globe2, 'A record contains expected IP', 'dns'],
] as const;

export function AssertionsLibrary() {
  const { t } = useTranslation('synthetics');
  return (
    <SyntheticsPage title={t('assertions.title')} subtitle={t('assertions.subtitle')}>
      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        {ASSERTION_TEMPLATES.map(([key, Icon, expression, preset]) => (
          <article key={key} className="flex min-h-48 flex-col rounded-lg border border-bd-0 bg-bg-1 p-4">
            <span className="grid h-10 w-10 place-items-center rounded-md bg-indigo/10 text-indigo-soft"><Icon className="h-5 w-5" /></span>
            <h2 className="mt-4 type-section-title font-strong text-tx-0">{t(`assertions.${key}`)}</h2>
            <code className="mt-2 block rounded-md border border-bd-0 bg-bg-2 px-2.5 py-2 font-code text-xs text-tx-2">{expression}</code>
            <Link to={`/synthetics/checks/new?preset=${preset}`} className="mt-auto inline-flex min-h-10 items-center justify-between rounded-md px-2 text-xs font-strong text-indigo-soft hover:bg-bg-2 focus-visible:bg-bg-2">
              {t('assertions.use_template')}
              <ArrowRight className="h-3.5 w-3.5" />
            </Link>
          </article>
        ))}
      </div>
    </SyntheticsPage>
  );
}

export function SyntheticsSettings() {
  const { t } = useTranslation('synthetics');
  const cards = [
    [Clock3, t('settings.state_model'), t('settings.state_model_hint'), '/synthetics/checks'],
    [BellRing, t('settings.alerting'), t('settings.alerting_hint'), '/alerts/escalations'],
    [RadioTower, t('settings.status_pages'), t('settings.status_pages_hint'), '/status-pages'],
    [ShieldCheck, t('settings.retention'), t('settings.retention_hint'), '/settings/general'],
  ] as const;
  return (
    <SyntheticsPage title={t('settings.title')} subtitle={t('settings.subtitle')}>
      <div className="grid gap-4 lg:grid-cols-2">
        {cards.map(([Icon, title, description, to]) => (
          <Link key={title} to={to} className="group flex min-h-40 gap-4 rounded-lg border border-bd-0 bg-bg-1 p-5 hover:bg-bg-2 focus-visible:bg-bg-2">
            <span className="grid h-11 w-11 shrink-0 place-items-center rounded-md border border-bd-0 bg-bg-2 text-indigo-soft group-hover:bg-bg-3"><Icon className="h-5 w-5" /></span>
            <div className="min-w-0"><h2 className="type-section-title font-strong text-tx-0">{title}</h2><p className="mt-2 text-sm leading-relaxed text-tx-2">{description}</p><span className="mt-4 inline-flex items-center gap-1 text-xs font-strong text-indigo-soft">{t('actions.open')} <ArrowRight className="h-3.5 w-3.5" /></span></div>
          </Link>
        ))}
      </div>
    </SyntheticsPage>
  );
}

function scheduleColumns(locale: string, t: TFunction<'synthetics'>): DataTableColumn<CheckRow>[] {
  return [
    { key: 'name', header: t('checks.columns.name'), width: 230, cell: (row) => <span className="font-strong text-tx-0">{row.monitor.name}</span> },
    { key: 'type', header: t('checks.columns.type'), cell: (row) => <KindLabel kind={row.monitor.kind} /> },
    { key: 'schedule', header: t('checks.columns.interval'), cell: (row) => scheduleText(row.revision?.schedule) },
    { key: 'next', header: t('schedules.next_run'), cell: (row) => formatRelativeTimestamp(row.monitor.next_due_at, locale) },
    { key: 'freshness', header: t('schedules.freshness'), cell: (row) => row.revision ? `${row.revision.freshness_seconds}s` : '—' },
    { key: 'threshold', header: t('schedules.threshold'), cell: (row) => row.revision ? `${row.revision.consecutive_failures} / ${row.revision.consecutive_recoveries}` : '—' },
    { key: 'retries', header: t('editor.retries'), cell: (row) => row.revision?.max_retries ?? '—' },
  ];
}
