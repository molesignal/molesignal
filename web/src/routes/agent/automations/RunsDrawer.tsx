import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as automationsApi from '@/api/agent/automations';
import type { Automation, AutomationRun } from '@/api/agent/automations';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { FormDrawer } from '@/shell/FormDrawer';
import { formatRelativeMicros } from '@/time/relative';

import { reasonsOf } from './model';
import { RunStatusWord } from './RunStatusWord';

/** While a run is going, the history is read again this often. */
const LIVE_REFRESH_MS = 5_000;

/**
 * The runs of an automation, newest first: how each went and why, what started
 * it, the start of its answer, where its notices went, and a link to the
 * hidden chat that holds the whole run.
 */
export function RunsDrawer({
  automation,
  onClose,
}: {
  automation: Automation | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('agent');
  const runs = useQuery({
    queryKey: ['agent', 'automations', automation?.id, 'runs'],
    queryFn: () => automationsApi.listAutomationRuns(automation!.id),
    enabled: automation !== null,
    retry: false,
    refetchInterval: (query) =>
      query.state.data?.some((run) => run.status === 'running') ? LIVE_REFRESH_MS : false,
  });

  if (!automation) return null;

  return (
    <FormDrawer
      open
      onOpenChange={(open) => !open && onClose()}
      title={automation.name}
      subtitle={t('automations.runs.subtitle')}
      width={620}
    >
      {runs.isPending ? (
        <ProductState variant="loading" />
      ) : runs.isError ? (
        <ProductState variant="error" error={runs.error} />
      ) : runs.data.length === 0 ? (
        <p data-automation-runs="none" className="m-0 py-8 text-center font-sans text-xs text-tx-3">
          {t('automations.runs.none')}
        </p>
      ) : (
        <ul data-automation-runs className="m-0 flex list-none flex-col gap-[2px] p-0">
          {runs.data.map((run) => (
            <RunRow key={run.id} run={run} />
          ))}
        </ul>
      )}
    </FormDrawer>
  );
}

function RunRow({ run }: { run: AutomationRun }) {
  const { t, i18n } = useTranslation('agent');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const now = React.useMemo(() => Date.now() * 1000, []);
  const reasons = reasonsOf(run);
  const failed = (run.outputs.deliveries ?? []).filter((delivery) => !delivery.delivered);

  return (
    <li data-automation-run={run.id} className="rounded-md px-[10px] py-[10px] font-sans hover:bg-bg-2">
      <div className="flex min-w-0 items-center gap-[8px] text-xs">
        <RunStatusWord status={run.status} />
        <span className="text-tx-2">{t(`automations.trigger_kind.${run.trigger}`)}</span>
        <span className="ml-auto shrink-0 whitespace-nowrap text-tx-2" title={formatMicrosActive(run.createdAt)}>
          {formatRelativeMicros(run.createdAt, locale, now)}
        </span>
      </div>
      {reasons.length > 0 && (
        <ul className="m-0 mt-[6px] list-none p-0 text-xs leading-relaxed text-tx-1">
          {reasons.map((reason) => (
            <li key={reason}>{t(`automations.reasons.${reason}`, { defaultValue: reason })}</li>
          ))}
        </ul>
      )}
      {run.input && (
        <p className="m-0 mt-[6px] line-clamp-2 text-xs leading-relaxed text-tx-2">
          {t('automations.runs.input', { text: run.input })}
        </p>
      )}
      {run.summary && <p className="m-0 mt-[6px] line-clamp-3 text-xs leading-relaxed text-tx-1">{run.summary}</p>}
      {run.outputs.rca && (
        <p className="m-0 mt-[6px] text-xs leading-relaxed text-tx-2">{t('automations.runs.rca')}</p>
      )}
      {failed.length > 0 && (
        <p className="m-0 mt-[6px] text-xs leading-relaxed text-[var(--orange-ink)]">
          {t('automations.runs.undelivered', {
            targets: failed.map((delivery) => delivery.targetMasked).join(', '),
          })}
        </p>
      )}
      {run.chatId && (
        <Link
          to={`/agent/chat?chat=${encodeURIComponent(run.chatId)}`}
          className="mt-[6px] inline-block rounded-sm text-xs font-strong text-indigo-soft underline-offset-4 hover:underline focus-visible:underline focus-visible:outline-none"
        >
          {t('automations.runs.open_chat')}
        </Link>
      )}
    </li>
  );
}
