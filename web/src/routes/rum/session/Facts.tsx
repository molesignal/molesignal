import { AlertTriangle, ChevronRight, CircleX, Gauge, Globe2, MousePointerClick } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumRow } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { formatDurationNs, regionName } from '../_helpers';
import { RumSectionHeader } from '../RumLayout';
import { issueTotal, sessionIssues, sessionUser } from './model';

const text = (row: RumRow, name: string): string | undefined => {
  const value = row[name];
  return typeof value === 'string' && value.trim() !== '' ? value : undefined;
};

/** Who the session was of, on what, and where it went. */
export function SessionFacts({ row }: { row: RumRow }) {
  const { t, i18n } = useTranslation('rum');
  const issues = sessionIssues(row);
  const country = text(row, 'geo.country_iso_code');
  const browser = [text(row, 'browser.name'), text(row, 'browser.version')].filter(Boolean).join(' ');
  const landing = text(row, 'session.landing_view.url_path');
  const exit = text(row, 'session.exit_view.url_path');
  const duration = typeof row['session.duration'] === 'number' ? row['session.duration'] : undefined;
  const kind = text(row, 'session.type');
  const check = text(row, 'synthetics.test_id');

  return (
    <div className="space-y-5">
      <section>
        <RumSectionHeader title={t('session_detail.user_context')} />
        <dl className="m-0 mt-3 grid grid-cols-[96px_minmax(0,1fr)] gap-x-3 gap-y-2.5 text-xs">
          {kind && kind !== 'user' && (
            <>
              <dt className="text-tx-3">{t('session_detail.session_type')}</dt>
              <dd data-rum-session-type={kind} className="m-0 min-w-0 truncate text-tx-1">
                {check ? (
                  <Link to={`/synthetics/checks/${encodeURIComponent(check)}/results`} className="text-[var(--blue-ink)] outline-none underline-offset-4 hover:underline focus-visible:underline">
                    {t(`session_detail.session_types.${kind}`, { defaultValue: kind })}
                  </Link>
                ) : (
                  t(`session_detail.session_types.${kind}`, { defaultValue: kind })
                )}
              </dd>
            </>
          )}
          <Fact label={t('session_detail.user')} value={sessionUser(row) ?? t('session_detail.anonymous_user')} />
          <Fact
            label={t('session_detail.device')}
            value={[browser, text(row, 'os.name'), text(row, 'device.type')].filter(Boolean).join(' · ') || '—'}
          />
          <Fact
            label={t('session_detail.location')}
            value={
              [country ? regionName(country, i18n.resolvedLanguage ?? i18n.language) : undefined, text(row, 'geo.city')]
                .filter(Boolean)
                .join(' · ') || '—'
            }
          />
          <Fact label={t('session_detail.ip_address')} value={text(row, 'network.client.ip') ?? '—'} />
          <Fact label={t('session_detail.application')} value={text(row, 'application.id') ?? '—'} />
          <Fact label={t('session_detail.environment')} value={text(row, 'env') ?? '—'} />
          <Fact label={t('session_detail.version')} value={text(row, 'version') ?? '—'} />
          <Fact label={t('session_detail.duration')} value={formatDurationNs(duration)} />
        </dl>
      </section>

      <section>
        <RumSectionHeader title={t('session_detail.journey')} />
        <div className="mt-3 rounded-md bg-bg-2 p-3">
          <div className="flex items-center gap-2">
            <Globe2 className="h-4 w-4 text-[var(--blue-ink)]" />
            <span className="min-w-0 truncate font-mono text-xs font-strong text-tx-0">{exit ?? landing ?? '—'}</span>
          </div>
          {landing && exit && landing !== exit && (
            <div className="mt-3 flex min-w-0 items-center gap-1 overflow-hidden text-xs text-tx-3">
              <span className="min-w-0 truncate">{landing}</span>
              <ChevronRight className="h-3 w-3 shrink-0" />
              <span className="min-w-0 truncate">{exit}</span>
            </div>
          )}
        </div>
      </section>

      <section>
        <RumSectionHeader title={t('session_detail.detected_issues')} />
        <div className="mt-3 space-y-2">
          {issueTotal(issues) === 0 ? (
            <div className="rounded-md bg-[var(--green-tint)] px-3 py-2.5 text-xs font-strong text-[var(--green-ink)]">
              {t('session_detail.no_detected_issues')}
            </div>
          ) : (
            <>
              <IssueFact icon={<AlertTriangle className="h-4 w-4" />} label={t('session_detail.issue.error')} value={issues.errors} tone="red" />
              <IssueFact icon={<CircleX className="h-4 w-4" />} label={t('session_detail.issue.crash')} value={issues.crashes} tone="red" />
              <IssueFact
                icon={<MousePointerClick className="h-4 w-4" />}
                label={t('session_detail.issue.frustration')}
                value={issues.frustrations}
                tone="yellow"
              />
              <IssueFact icon={<Gauge className="h-4 w-4" />} label={t('session_detail.issue.long_task')} value={issues.longTasks} tone="yellow" />
            </>
          )}
        </div>
      </section>
    </div>
  );
}

function Fact({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <>
      <dt className="text-tx-3">{label}</dt>
      <dd className="m-0 min-w-0 truncate font-strong text-tx-1">{value}</dd>
    </>
  );
}

function IssueFact({
  icon,
  label,
  value,
  tone,
}: {
  icon: React.ReactNode;
  label: string;
  value: number;
  tone: 'red' | 'yellow';
}) {
  if (value <= 0) return null;

  return (
    <div
      className={cn(
        'flex items-center gap-2 rounded-md px-3 py-2.5 text-xs font-strong',
        tone === 'red' ? 'bg-red-dim text-red-soft' : 'bg-[var(--yellow-tint)] text-[var(--yellow-ink)]',
      )}
    >
      {icon}
      <span>{label}</span>
      <span className="ml-auto font-mono">{value}</span>
    </div>
  );
}
