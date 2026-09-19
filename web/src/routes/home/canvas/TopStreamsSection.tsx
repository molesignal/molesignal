import { ChevronRight } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type * as homeApi from '@/api/home';
import type * as streamsApi from '@/api/streams';
import { runtimeStatusToHealthStatus } from '@/investigation/streamHealth';
import {
  Dot,
  Pill,
  type PillTone,
  TableShell,
  Td,
  Th,
  Tr,
} from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { QueryState } from '@/shell/query/State';
import type { queryStateFor } from '@/shell/query/State';
import { formatRelativeMicros } from '@/time/relative';

import {
  calculateHomeStreamRowCount,
  DEFAULT_HOME_STREAM_ROWS,
  shouldFillHomeStreamViewport,
} from '../streamRows';
import { formatEventRate } from './format';
import { CanvasHeaderAction, CanvasSection } from './layout';

const STATUS_TONE: Record<homeApi.HomeHealthStatus, PillTone> = {
  healthy: 'green',
  degraded: 'red',
  delayed: 'yellow',
  no_data: 'dim',
  unknown: 'dim',
};

const STATUS_DOT: Record<
  homeApi.HomeHealthStatus,
  'green' | 'red' | 'yellow' | 'dim'
> = {
  healthy: 'green',
  degraded: 'red',
  delayed: 'yellow',
  no_data: 'dim',
  unknown: 'dim',
};

const STREAM_TONE: Record<homeApi.HomeStreamOverview['stream_type'], PillTone> = {
  logs: 'orange',
  metrics: 'blue',
  traces: 'green',
  profiles: 'purple',
};

export function TopStreamsSection({
  overview,
  runtimeOverview,
  state,
  error,
  onOpen,
  onViewAll,
}: {
  overview: homeApi.HomeOverview | undefined;
  runtimeOverview: streamsApi.StreamRuntimeOverview | undefined;
  state: ReturnType<typeof queryStateFor>;
  error: unknown;
  onOpen: (stream: homeApi.HomeStreamOverview) => void;
  onViewAll: () => void;
}) {
  const { t, i18n } = useTranslation('onboarding');
  const streams = overview?.streams ?? [];
  const runtimeById = new Map(
    (runtimeOverview?.streams ?? []).map((stream) => [stream.id, stream]),
  );
  const overviewWindowSecs =
    runtimeOverview?.window_secs ?? overview?.window?.window_secs ?? 1;
  const generatedAtMicros =
    runtimeOverview?.generated_at_micros ?? overview?.generated_at_micros;
  const tableViewportRef = React.useRef<HTMLDivElement>(null);
  const [visibleRowCount, setVisibleRowCount] = React.useState(() =>
    Math.min(streams.length, DEFAULT_HOME_STREAM_ROWS),
  );
  const [fillTableHeight, setFillTableHeight] = React.useState(false);

  React.useLayoutEffect(() => {
    const viewport = tableViewportRef.current;
    setVisibleRowCount((current) => {
      const next = Math.min(streams.length, current || DEFAULT_HOME_STREAM_ROWS);
      return current === next ? current : next;
    });
    if (!viewport || streams.length === 0) {
      setFillTableHeight(false);
      return;
    }

    let animationFrame = 0;
    const measure = () => {
      window.cancelAnimationFrame(animationFrame);
      animationFrame = window.requestAnimationFrame(() => {
        const header = viewport.querySelector<HTMLTableSectionElement>('thead');
        const firstRow = viewport.querySelector<HTMLTableRowElement>('tbody tr');
        const headerHeight = header?.getBoundingClientRect().height ?? 0;
        const rowHeight = firstRow?.getBoundingClientRect().height ?? 0;
        const nextCount = calculateHomeStreamRowCount({
          viewportHeight: viewport.clientHeight,
          headerHeight,
          rowHeight,
          totalRows: streams.length,
        });
        setVisibleRowCount((current) => (current === nextCount ? current : nextCount));
        const shouldFill = shouldFillHomeStreamViewport({
          viewportHeight: viewport.clientHeight,
          headerHeight,
          rowHeight,
          visibleRows: nextCount,
        });
        setFillTableHeight((current) => (current === shouldFill ? current : shouldFill));
      });
    };

    measure();
    if (typeof ResizeObserver === 'undefined') {
      return () => window.cancelAnimationFrame(animationFrame);
    }

    const observer = new ResizeObserver(measure);
    observer.observe(viewport);
    const header = viewport.querySelector<HTMLTableSectionElement>('thead');
    const firstRow = viewport.querySelector<HTMLTableRowElement>('tbody tr');
    if (header) observer.observe(header);
    if (firstRow) observer.observe(firstRow);

    return () => {
      observer.disconnect();
      window.cancelAnimationFrame(animationFrame);
    };
  }, [streams.length]);

  return (
    <CanvasSection
      className="home-canvas-detail-section"
      title={t('home.streams.title')}
      actions={<CanvasHeaderAction label={t('home.view_all')} onClick={onViewAll} />}
    >
      {state ? (
        <div className="grid h-full min-h-[254px] place-items-stretch">
          <QueryState state={state} error={error} emptyLabel={t('home.streams.empty')} />
        </div>
      ) : (
        <div
          ref={tableViewportRef}
          className={cn(
            'min-h-0 flex-1 overflow-hidden',
            fillTableHeight && '[&>div]:h-full',
          )}
          data-testid="home-top-streams-viewport"
        >
          <TableShell className={cn('min-w-[620px]', fillTableHeight && 'h-full')}>
            <thead>
              <tr>
                <Th>{t('home.streams.columns.stream')}</Th>
                <Th>{t('home.streams.columns.type')}</Th>
                <Th>{t('home.streams.columns.status')}</Th>
                <Th>{t('home.streams.columns.rate')}</Th>
                <Th>{t('home.streams.columns.last_received')}</Th>
                <Th className="w-16 whitespace-nowrap text-right">
                  {t('home.streams.columns.action')}
                </Th>
              </tr>
            </thead>
            <tbody>
              {streams.slice(0, visibleRowCount).map((stream) => {
                const runtime = runtimeById.get(stream.id);
                const status = runtime
                  ? runtimeStatusToHealthStatus(runtime.status)
                  : 'unknown';
                return (
                  <Tr key={stream.id} onClick={() => onOpen(stream)}>
                    <Td className="font-strong text-tx-0">{stream.name}</Td>
                    <Td>
                      <Pill tone={STREAM_TONE[stream.stream_type]}>
                        {stream.stream_type}
                      </Pill>
                    </Td>
                    <Td>
                      <Pill tone={STATUS_TONE[status]}>
                        <Dot tone={STATUS_DOT[status]} />
                        {t(`home.status.${status}`)}
                      </Pill>
                    </Td>
                    <Td className="font-mono text-xs">
                      {formatEventRate(runtime?.rows ?? stream.rows, overviewWindowSecs)}
                    </Td>
                    <Td className="text-tx-2">
                      {formatRelativeMicros(
                        runtime?.last_received_at_micros ?? stream.last_received_at_micros,
                        i18n.resolvedLanguage ?? i18n.language,
                        generatedAtMicros,
                      )}
                    </Td>
                    <Td className="text-right">
                      <ChevronRight className="ml-auto h-4 w-4 text-tx-3" />
                    </Td>
                  </Tr>
                );
              })}
            </tbody>
          </TableShell>
        </div>
      )}
    </CanvasSection>
  );
}
