import { X } from 'lucide-react';
import * as React from 'react';
import { Trans, useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { SignalReference } from '@/shell/SignalReference';

import { statusInk, typeInk } from './ink';
import type { LcpParts, WaterfallResource, WaterfallTask } from './model';
import { PhaseBar } from './Phases';
import { formatDurationMs, formatSize } from '../_helpers';
import { FIGURE } from '../performance/Reading';

const PANEL = 'flex min-w-0 flex-col gap-3.5 rounded-md border border-bd-0 px-4 pb-4 pt-3.5 font-sans';

function Head({
  title,
  subtitle,
  onClose,
  closeLabel,
  mono,
}: {
  title: string;
  subtitle: React.ReactNode;
  onClose: () => void;
  closeLabel: string;
  mono?: boolean;
}) {
  return (
    <div className="flex items-start gap-2">
      <div className="min-w-0 flex-1">
        <div className={cn('truncate text-sm font-strong text-tx-0', mono && 'font-mono')} title={title}>
          {title}
        </div>
        <div className="truncate text-xs text-tx-3">{subtitle}</div>
      </div>
      <button
        type="button"
        aria-label={closeLabel}
        onClick={onClose}
        className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md border-0 bg-transparent p-0 text-tx-2 outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
      >
        <X aria-hidden className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

function Fields({ rows }: { rows: ReadonlyArray<readonly [label: string, value: React.ReactNode]> }) {
  return (
    <dl className="m-0 grid grid-cols-[76px_minmax(0,1fr)] gap-x-3 gap-y-1.5 text-xs">
      {rows.map(([label, value]) => (
        <React.Fragment key={label}>
          <dt className="text-tx-3">{label}</dt>
          <dd className="m-0 min-w-0 break-words text-tx-0">{value}</dd>
        </React.Fragment>
      ))}
    </dl>
  );
}

/** What a request in the waterfall says of itself: when it began, what it was, how it ended, and what it took its time on. */
export function ResourceDetail({
  resource,
  lcp,
  afterLoad = false,
  onClose,
}: {
  resource: WaterfallResource;
  /** The parts of the paint of the largest element, where this is the file it was of. */
  lcp: LcpParts | null;
  /** The request began after the page had loaded, and is not drawn on the axis. */
  afterLoad?: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation('rum');
  const duration = formatDurationMs(resource.duration);

  const sentence = () => {
    if (resource.lcp && lcp) {
      return (
        <Trans
          i18nKey="waterfall.detail.sentence.lcp"
          ns="rum"
          values={{ delay: formatDurationMs(lcp.loadDelay), load: formatDurationMs(lcp.loadTime), render: formatDurationMs(lcp.renderDelay) }}
          components={{ n: FIGURE }}
        />
      );
    }

    if (resource.status === 0) {
      return <Trans i18nKey="waterfall.detail.sentence.unanswered" ns="rum" values={{ duration }} components={{ n: FIGURE }} />;
    }

    if (resource.failed) {
      return <Trans i18nKey="waterfall.detail.sentence.failed" ns="rum" values={{ duration, status: resource.status }} components={{ n: FIGURE }} />;
    }

    if (resource.wait !== null && resource.download !== null) {
      return (
        <Trans
          i18nKey="waterfall.detail.sentence.phases"
          ns="rum"
          values={{ wait: formatDurationMs(resource.wait), download: formatDurationMs(resource.download) }}
          components={{ n: FIGURE }}
        />
      );
    }

    return <Trans i18nKey="waterfall.detail.sentence.total" ns="rum" values={{ duration }} components={{ n: FIGURE }} />;
  };

  const delivery = resource.delivery === null ? t('waterfall.detail.delivery.network') : t(`waterfall.detail.delivery.${resource.delivery}`, { defaultValue: resource.delivery });
  const name = [resource.method && resource.method !== 'GET' ? resource.method : null, resource.path].filter(Boolean).join(' ');

  return (
    <aside data-rum-waterfall-detail="resource" aria-label={t('waterfall.detail.aria')} className={PANEL}>
      <Head
        title={name}
        mono
        closeLabel={t('waterfall.detail.close')}
        onClose={onClose}
        subtitle={
          <>
            <span className={cn('font-strong', typeInk(resource.type))}>{resource.type}</span>
            {resource.host && ` · ${resource.host}`}
            {resource.lcp && <span className="font-strong text-[var(--yellow-ink)]">{` · ${t('waterfall.detail.lcp_file')}`}</span>}
          </>
        }
      />
      <p className="m-0 text-sm leading-[21px] text-tx-1">{sentence()}</p>
      {afterLoad && (
        <p data-rum-waterfall-after-load="" className="m-0 text-xs leading-[19px] text-tx-3">
          <Trans i18nKey="waterfall.detail.after_load" ns="rum" values={{ start: formatDurationMs(resource.start) }} components={{ n: FIGURE }} />
        </p>
      )}
      <Fields
        rows={[
          [t('waterfall.detail.fields.address'), <span key="url" className="break-all font-mono text-xs">{resource.url}</span>],
          [
            t('waterfall.detail.fields.status'),
            <span key="status">
              <span className={statusInk(resource.status)}>{resource.status ?? '—'}</span>
              {resource.protocol && ` · ${resource.protocol}`}
            </span>,
          ],
          [
            t('waterfall.detail.fields.size'),
            resource.size === null
              ? '—'
              : resource.transfer === null
                ? formatSize(resource.size)
                : t('waterfall.detail.size', { size: formatSize(resource.size), transfer: formatSize(resource.transfer) }),
          ],
          [t('waterfall.detail.fields.delivery'), delivery],
          [t('waterfall.detail.fields.blocking'), resource.blocking ? t('waterfall.detail.yes') : t('waterfall.detail.no')],
          [t('waterfall.detail.fields.start'), formatDurationMs(resource.start)],
        ]}
      />
      <PhaseBar phases={resource.phases} title={t('waterfall.phases.title')} />
      {resource.traceId && (
        <div className="flex items-center justify-between gap-2 border-t border-bd-0 pt-2.5 text-xs">
          <span className="text-tx-3">{t('waterfall.detail.trace')}</span>
          <SignalReference type="trace_id" value={resource.traceId} className="min-w-0 truncate font-mono text-xs">
            {resource.traceId.slice(0, 16)}
          </SignalReference>
        </div>
      )}
    </aside>
  );
}

/** A long task of the main thread: how long it ran, how much of that held up the page's input, and the script it is put down to. */
export function TaskDetail({ task, onClose }: { task: WaterfallTask; onClose: () => void }) {
  const { t } = useTranslation('rum');
  const invoker =
    task.invoker === null ? null : t(`performance.long_tasks.invokers.${task.invoker}`, { defaultValue: task.invoker });

  return (
    <aside data-rum-waterfall-detail="task" aria-label={t('waterfall.detail.aria')} className={PANEL}>
      <Head
        title={task.script ?? t('waterfall.task.unattributed')}
        mono={task.script !== null}
        closeLabel={t('waterfall.detail.close')}
        onClose={onClose}
        subtitle={t('waterfall.task.subtitle', { duration: formatDurationMs(task.duration) })}
      />
      <p className="m-0 text-sm leading-[21px] text-tx-1">
        {task.blocking === null ? (
          <Trans i18nKey="waterfall.task.sentence" ns="rum" values={{ duration: formatDurationMs(task.duration) }} components={{ n: FIGURE }} />
        ) : (
          <Trans
            i18nKey="waterfall.task.sentence_blocking"
            ns="rum"
            values={{ duration: formatDurationMs(task.duration), blocking: formatDurationMs(task.blocking) }}
            components={{ n: FIGURE }}
          />
        )}
      </p>
      <Fields
        rows={[
          [t('waterfall.task.fields.script'), task.script ? <span key="s" className="break-all font-mono text-xs">{task.script}</span> : t('waterfall.task.none')],
          [t('waterfall.task.fields.function'), task.function ? <span key="f" className="font-mono text-xs">{task.function}</span> : '—'],
          [t('waterfall.task.fields.invoker'), invoker ?? '—'],
          [t('waterfall.detail.fields.start'), formatDurationMs(task.start)],
        ]}
      />
    </aside>
  );
}
