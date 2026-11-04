import { Crosshair, PanelRightClose } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { TraceWindow } from '@/api/web';
import { writeClipboardText } from '@/lib/clipboard';
import { useTimeFormatter, zonedMoment } from '@/lib/time';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';
import { SignalReference, type SignalReferenceTime } from '@/shell/SignalReference';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Button } from '@/shell/ui/button';

import { formatTraceDurationNs } from '../duration';
import type { TraceNode, TraceTree } from '../model/tree';
import { TraceOperationName } from '../TraceOperationName';
import { SpanEvents } from './Events';
import { type OpenLinkedTrace, SpanLinks } from './Links';
import type { TraceProfiles } from './profileLinks';
import { SpanLogs } from './SpanLogs';
import { SpanMetadata } from './SpanMetadata';
import { SpanMetrics } from './SpanMetrics';
import { SpanProfile } from './SpanProfile';
import { TagList } from './TagList';
import { HTTP_STATUS_TEXT, httpStatusOf, httpStatusTone, spanKindName, splitAttributes } from '../model/span';
import { QUIET_REFERENCE, signalLabelsForSpan } from '../waterfall/model';

export type SpanPanelTab = 'attributes' | 'resource' | 'events' | 'links' | 'logs' | 'metrics' | 'profile';

interface SpanPanelProps {
  tree: TraceTree;
  node: TraceNode;
  tab: SpanPanelTab;
  onTabChange: (tab: SpanPanelTab) => void;
  time: SignalReferenceTime | undefined;
  /** When the trace ran, a little widened: where its logs are read. */
  window: TraceWindow;
  /** When the trace ran, widened further: where the spans linking to it are looked for. */
  linkWindow: TraceWindow;
  /** Where the span's logs, and the trace's, are searched in full. */
  logsHref: { span: string | undefined; trace: string | undefined };
  /** The profiles that kept the trace's id, once read. */
  profiles: TraceProfiles | undefined;
  profilesLoading: boolean;
  onSelect: (id: string) => void;
  onFocus: (id: string) => void;
  onClose: () => void;
  onOpenTrace: OpenLinkedTrace;
}

/**
 * The detail of the selected span, beside the waterfall: what it is, whose,
 * how it ended and how long it took, said once at the head; then its tags,
 * its resource's, its events, its links, its logs and how its service was
 * doing, one tab each.
 */
export function SpanPanel({
  tree,
  node,
  tab,
  onTabChange,
  time,
  window,
  linkWindow,
  logsHref,
  profiles,
  profilesLoading,
  onSelect,
  onFocus,
  onClose,
  onOpenTrace,
}: SpanPanelProps) {
  const { t } = useTranslation('traces');
  const fmt = useTimeFormatter();
  const { span } = node;
  const { attributes, resource } = React.useMemo(() => splitAttributes(span), [span]);
  const labels = signalLabelsForSpan(tree.traceId, span);
  const source = { type: 'trace' as const, id: tree.traceId };
  const status = httpStatusOf(span);
  const kind = spanKindName(span.kind);
  const share = Math.round((node.durationNs / tree.totalNs) * 1000) / 10;
  const parent = node.parentId ? tree.byId.get(node.parentId) : undefined;
  const color = tree.services.find((service) => service.name === node.service)?.color ?? 'var(--chart-1)';

  // A count only where it tells whether there is anything: every span has tags and a resource, few have events or links.
  const counted = (key: SpanPanelTab, label: string, count?: number) => ({ key, label, ...(count ? { count } : {}) });
  const tabs = [
    counted('attributes', t('view.tabs.attributes')),
    counted('resource', t('view.tabs.resource')),
    counted('events', t('view.tabs.events'), span.events.length),
    counted('links', t('view.tabs.links'), span.links.length),
    counted('logs', t('view.tabs.logs')),
    counted('metrics', t('view.tabs.metrics')),
    counted('profile', t('view.tabs.profile'), profiles?.bySpan.get(node.id)?.length),
  ];
  const started = zonedMoment(Math.floor(node.startNs / 1_000_000), fmt.tz);

  return (
    <aside aria-label={t('view.panel.aria')} data-testid="trace-span-panel" className="flex h-full min-h-0 min-w-0 flex-col bg-bg-0">
      <div className="shrink-0 border-b border-bd-0 px-3 pb-2 pt-2.5">
        <div className="flex items-start gap-2">
          <h2 className="min-w-0 flex-1">
            <span className="flex min-w-0 items-center gap-1.5">
              <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-sm" style={{ backgroundColor: color }} />
              <SignalReference
                type="service"
                value={node.service}
                labelName="service.name"
                labels={labels}
                time={time}
                source={source}
                showIcon={false}
                className={cn('min-w-0 font-sans text-xs text-tx-2 hover:text-indigo-soft [&>span]:truncate', QUIET_REFERENCE)}
              >
                {node.service}
              </SignalReference>
            </span>
            <SignalReference
              type="span_id"
              value={span.spanId}
              labelName="span_id"
              labels={labels}
              time={time}
              source={source}
              showIcon={false}
              className={cn('mt-0.5 max-w-full justify-start text-left text-tx-0', QUIET_REFERENCE)}
            >
              <TraceOperationName operation={span.operation} className="font-sans text-sm font-strong text-tx-0" />
            </SignalReference>
          </h2>
          <Button variant="ghost" size="icon" className="h-7 w-7 shrink-0" aria-label={t('view.panel.focus')} title={t('view.panel.focus')} onClick={() => onFocus(node.id)}>
            <Crosshair className="h-3.5 w-3.5" />
          </Button>
          <Button variant="ghost" size="icon" className="h-7 w-7 shrink-0" aria-label={t('view.panel.close')} title={t('view.panel.close')} onClick={onClose}>
            <PanelRightClose className="h-3.5 w-3.5" />
          </Button>
        </div>

        <div className="mt-1.5 flex flex-wrap items-center gap-1.5 font-sans text-xs">
          <span className="rounded-sm bg-bg-2 px-1.5 leading-5 text-tx-1">{t(`view.kind.${kind}`)}</span>
          <span
            className={cn(
              'font-strong leading-5',
              node.failed ? 'text-[var(--red-ink)]' : span.status === 'OK' ? 'text-[var(--green-ink)]' : 'text-tx-2',
            )}
          >
            {t(`view.status.${node.failed ? 'error' : span.status === 'OK' ? 'ok' : 'unset'}`)}
          </span>
          {status !== null && (
            <span className={cn('font-strong leading-5 tabular-nums', HTTP_STATUS_TEXT[httpStatusTone(status)])}>HTTP {status}</span>
          )}
        </div>

        {node.failed && span.statusMessage && (
          <p className="mt-1.5 line-clamp-3 break-words font-sans text-xs text-[var(--red-ink)] [overflow-wrap:anywhere]">{span.statusMessage}</p>
        )}

        <dl className="mt-2 grid grid-cols-3 gap-2" data-testid="trace-span-facts">
          <Fact label={t('view.panel.duration')} value={formatTraceDurationNs(node.durationNs)}>
            <span className="mt-1 block h-1 overflow-hidden rounded-full bg-bg-3" title={t('view.panel.share', { percent: share })}>
              <span className="block h-full rounded-full" style={{ width: `${Math.max(1, share)}%`, backgroundColor: node.failed ? 'var(--red)' : color }} />
            </span>
          </Fact>
          {/* A span that called nothing spent all its time on its own work: that is its duration, said once. */}
          {node.children.length > 0 && <Fact label={t('view.panel.self')} value={formatTraceDurationNs(node.selfNs)} />}
          <Fact
            label={t('view.panel.offset')}
            value={node.offsetNs > 0 ? `+${formatTraceDurationNs(node.offsetNs)}` : '0 ms'}
            title={started.isValid() ? started.format('YYYY-MM-DD HH:mm:ss.SSS') : undefined}
          />
        </dl>

        <div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-x-3 gap-y-0.5 font-sans text-xs">
          <span className="flex min-w-0 items-center gap-1">
            <span className="shrink-0 text-tx-3">{t('view.panel.span_id')}</span>
            <span className="min-w-0 truncate tabular-nums text-tx-1" title={span.spanId}>{span.spanId}</span>
            <CopyIconButton
              className="h-6 w-6 shrink-0 text-tx-3 hover:text-tx-0"
              label={t('detail.copy_span_id')}
              onClick={() => {
                void writeClipboardText(span.spanId);
              }}
            />
          </span>
          {parent && (
            <span className="flex min-w-0 items-center gap-1">
              <span className="shrink-0 text-tx-3">{t('view.panel.parent')}</span>
              <button
                type="button"
                onClick={() => onSelect(parent.id)}
                className="min-w-0 truncate rounded-sm text-left text-[var(--blue-ink)] hover:underline focus-visible:underline focus-visible:outline-none"
                title={`${parent.service} · ${parent.span.operation}`}
              >
                {parent.span.operation}
              </button>
            </span>
          )}
        </div>
      </div>

      <div className="shrink-0 border-b border-bd-0 px-2 py-1.5">
        <ViewTabs label={t('view.panel.tabs')} value={tab} onValueChange={onTabChange} items={tabs} />
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-3 py-2">
        {tab === 'attributes' && (
          <>
            <TagList entries={attributes} label={t('view.tabs.attributes')} emptyText={t('view.tags.empty')} />
            <SpanMetadata span={span} />
          </>
        )}
        {tab === 'resource' && <TagList entries={resource} label={t('view.tabs.resource')} emptyText={t('view.tags.empty_resource')} />}
        {tab === 'events' && <SpanEvents span={span} startNs={node.startNs} />}
        {tab === 'links' && <SpanLinks traceId={tree.traceId} span={span} window={linkWindow} onOpenTrace={onOpenTrace} />}
        {tab === 'logs' && (
          <SpanLogs traceId={tree.traceId} spanId={span.spanId} window={window} spanLogsHref={logsHref.span} traceLogsHref={logsHref.trace} />
        )}
        {tab === 'metrics' && <SpanMetrics service={node.service} operation={span.operation} startNs={node.startNs} />}
        {tab === 'profile' && <SpanProfile traceId={tree.traceId} node={node} profiles={profiles} loading={profilesLoading} />}
      </div>
    </aside>
  );
}

function Fact({ label, value, title, children }: { label: string; value: string; title?: string | undefined; children?: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="font-sans text-xs text-tx-3">{label}</dt>
      <dd className="truncate font-sans text-sm font-strong tabular-nums text-tx-0" title={title}>
        {value}
        {children}
      </dd>
    </div>
  );
}
