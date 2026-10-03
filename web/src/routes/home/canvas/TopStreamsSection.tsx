import { ChevronRight, Database } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type * as homeApi from '@/api/home';
import type * as streamsApi from '@/api/streams';
import { TableShell, Td, Th, Tr } from '@/shell/chrome';
import { EmptyIllustration } from '@/shell/EmptyIllustration';
import { EmptyState } from '@/shell/EmptyState';
import { cn } from '@/shell/lib/cn';
import { QueryState } from '@/shell/query/State';
import type { queryStateFor } from '@/shell/query/State';
import { StreamTypeTag } from '@/shell/StreamTypeTag';
import { formatRelativeMicros } from '@/time/relative';

import {
  calculateHomeStreamRowCount,
  DEFAULT_HOME_STREAM_ROWS,
  shouldFillHomeStreamViewport,
} from '../streamRows';
import { formatEventRate, streamExplorePath } from './format';
import { TableSkeleton } from './HomeSkeletons';
import { CanvasHeaderAction, CanvasSection } from './layout';
import {
  classifyStreamCondition,
  type StreamCondition,
  toMicros,
} from './streamCondition';
import { ConditionBadge } from './StreamTags';

export function TopStreamsSection({
  overview,
  runtimeOverview,
  state,
  error,
  onOpen,
  onViewAll,
  onConnect,
  riseIndex,
}: {
  overview: homeApi.HomeOverview | undefined;
  runtimeOverview: streamsApi.StreamRuntimeOverview | undefined;
  state: ReturnType<typeof queryStateFor>;
  error: unknown;
  onOpen: (stream: homeApi.HomeStreamOverview) => void;
  onViewAll: () => void;
  onConnect: () => void;
  riseIndex?: number;
}) {
  const { t, i18n } = useTranslation('onboarding');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const streams = overview?.streams ?? [];
  const runtimeById = new Map(
    (runtimeOverview?.streams ?? []).map((stream) => [stream.id, stream]),
  );
  const overviewWindowSecs =
    runtimeOverview?.window_secs ?? overview?.window?.window_secs ?? 1;
  const generatedAtMicros =
    runtimeOverview?.generated_at_micros ?? overview?.generated_at_micros;
  const nowMicros =
    generatedAtMicros != null ? toMicros(generatedAtMicros) : Date.now() * 1000;
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

  const rows = streams.slice(0, visibleRowCount).map((stream) => {
    const runtime = runtimeById.get(stream.id);
    const lastReceived =
      runtime?.last_received_at_micros ?? stream.last_received_at_micros;
    const condition: StreamCondition = classifyStreamCondition({
      status: runtime?.status ?? 'unknown',
      lastReceivedAtMicros: lastReceived,
      nowMicros,
    });
    return { stream, runtime, lastReceived, condition };
  });
  // A column of identical pills says one thing many times; the verdict card
  // already says it once, so the rows fall back to quiet text.
  const quiet =
    rows.length > 1 && rows.every((row) => row.condition === rows[0]?.condition);

  const body = () => {
    if (state === 'loading') return <TableSkeleton />;
    if (state === 'error') {
      return (
        <div className="grid h-full min-h-[254px] place-items-stretch">
          <QueryState state="error" error={error} />
        </div>
      );
    }
    if (state === 'empty') {
      return (
        <EmptyState
          size="compact"
          icon={Database}
          illustration={<EmptyIllustration kind="streams" />}
          title={t('home.streams.empty')}
          primaryAction={{ label: t('home.connect_action'), onClick: onConnect }}
        />
      );
    }
    return (
      <div
        ref={tableViewportRef}
        className={cn(
          'min-h-0 flex-1 overflow-hidden',
          fillTableHeight && '[&>div]:h-full',
        )}
        data-testid="home-top-streams-viewport"
      >
        {/* Rows are sized to fit this viewport, so vertical scroll would only
            ever expose a rounding-error sliver (and draw a second scrollbar). */}
        <TableShell
          className={cn('min-w-[620px]', fillTableHeight && 'h-full')}
          containerClassName="overflow-y-hidden"
        >
          <thead>
            <tr>
              <Th className="pl-[20px]">{t('home.streams.columns.stream')}</Th>
              <Th>{t('home.streams.columns.type')}</Th>
              <Th>{t('home.streams.columns.status')}</Th>
              <Th className="text-right">{t('home.streams.columns.rate')}</Th>
              <Th>{t('home.streams.columns.last_received')}</Th>
              <Th className="w-12 pr-[20px]">
                <span className="sr-only">{t('home.streams.columns.action')}</span>
              </Th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ stream, runtime, lastReceived, condition }) => (
              <Tr
                key={stream.id}
                className="group focus-within:bg-bg-3"
                onClick={(event) => {
                  // The name is a real link (keyboard, middle-click, new tab);
                  // the rest of the row is a larger mouse target for the same place.
                  if ((event.target as HTMLElement).closest('a')) return;
                  onOpen(stream);
                }}
              >
                <Td className="pl-[20px] font-strong text-tx-0">
                  <Link
                    to={streamExplorePath(stream)}
                    className="block truncate rounded-sm"
                  >
                    {stream.name}
                  </Link>
                </Td>
                <Td>
                  <StreamTypeTag type={stream.stream_type} />
                </Td>
                <Td>
                  <ConditionBadge condition={condition} quiet={quiet} />
                </Td>
                <Td className="text-right tabular-nums">
                  {formatEventRate(runtime?.rows ?? stream.rows, overviewWindowSecs)}
                </Td>
                <Td className="text-tx-2">
                  {lastReceived != null ? (
                    <time
                      dateTime={new Date(toMicros(lastReceived) / 1000).toISOString()}
                      title={new Date(toMicros(lastReceived) / 1000).toLocaleString(locale)}
                    >
                      {formatRelativeMicros(lastReceived, locale, generatedAtMicros)}
                    </time>
                  ) : (
                    '—'
                  )}
                </Td>
                <Td className="pr-[20px] text-right">
                  <ChevronRight
                    aria-hidden="true"
                    className="ml-auto h-4 w-4 text-tx-3 transition-[color,transform] duration-fast group-hover:translate-x-0.5 group-hover:text-tx-1"
                  />
                </Td>
              </Tr>
            ))}
          </tbody>
        </TableShell>
      </div>
    );
  };

  return (
    <CanvasSection
      className="home-canvas-streams-section"
      title={t('home.streams.title')}
      actions={<CanvasHeaderAction label={t('home.view_all')} onClick={onViewAll} />}
      {...(riseIndex !== undefined ? { riseIndex } : {})}
    >
      {body()}
    </CanvasSection>
  );
}
