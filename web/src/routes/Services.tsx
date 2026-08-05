import { useQuery } from '@tanstack/react-query';
import { ArrowDownLeft, ArrowUpRight } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import { DataTable, type MetadataStripItem } from '@/admin';
import * as serviceGraphApi from '@/api/serviceGraph';
import type { TopologyEdge, TopologyNode } from '@/api/serviceGraph';
import { productStateFor, type ProductStateProps } from '@/product/states';
import { DetailPage, ListPage } from '@/product/templates';
import { Pill, type PillTone } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { SignalReference } from '@/shell/SignalReference';
import { resolveWindow, useTimeStore } from '@/stores/useTimeStore';

/* ─── formatting + shared time window ─── */

function fmtRps(v: number): string {
  return v >= 100 ? v.toFixed(0) : v.toFixed(1);
}
function fmtMs(v: number): string {
  return `${v.toFixed(0)}ms`;
}
function fmtPct(fraction: number): string {
  return `${(fraction * 100).toFixed(2)}%`;
}
/** Error-rate severity: green < 1% < yellow < 5% < red. */
function errorTone(fraction: number): PillTone {
  if (fraction >= 0.05) return 'red';
  if (fraction >= 0.01) return 'yellow';
  return 'green';
}

function useRange(): { from: string; to: string } {
  const window = useTimeStore((s) => s.window);
  return React.useMemo(() => {
    const r = resolveWindow(window);
    return { from: r.from.toISOString(), to: r.to.toISOString() };
  }, [window]);
}

function useTopology(range: { from: string; to: string }) {
  return useQuery({
    queryKey: ['web', 'topology', range.from, range.to],
    queryFn: () => serviceGraphApi.get(range.from, range.to),
  });
}

/* ─── /services — service catalog ─── */

export function Services() {
  const { t } = useTranslation('services');
  const navigate = useNavigate();
  const range = useRange();
  const q = useTopology(range);

  const nodes = React.useMemo(
    () => [...(q.data?.nodes ?? [])].sort((a, b) => b.span_count - a.span_count),
    [q.data],
  );

  const tableState = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: nodes });
  const listState: ProductStateProps | null =
    tableState === 'loading'
      ? { variant: 'loading' }
      : tableState === 'error'
        ? { variant: 'error', error: q.error }
        : tableState === 'empty'
          ? {
              variant: 'empty',
              title: t('states.empty_title', { defaultValue: 'No services seen' }),
              description: t('states.empty_description', {
                defaultValue: 'Services appear once traced spans arrive in the selected time range.',
              }),
            }
          : null;

  const totalRps = nodes.reduce((sum, n) => sum + n.rps, 0);
  const degraded = nodes.filter((n) => n.error_rate >= 0.01).length;

  return (
    <ListPage
      title={t('title', { defaultValue: 'Services' })}
      subtitle={t('subtitle', { defaultValue: 'Service catalog derived from trace topology.' })}
      kpis={[
        { label: t('kpis.services', { defaultValue: 'Services' }), value: String(nodes.length) },
        { label: t('kpis.rps', { defaultValue: 'Total RPS' }), value: fmtRps(totalRps) },
        {
          label: t('kpis.degraded', { defaultValue: 'Degraded (≥1% err)' }),
          value: String(degraded),
          tone: degraded > 0 ? 'warn' : 'neutral',
        },
      ]}
      kpiLayout="inline"
      state={listState}
    >
      <DataTable
        rows={nodes}
        rowKey={(n) => n.id}
        onRowClick={(n) => navigate(`/services/${encodeURIComponent(n.name)}`)}
        columns={[
          {
            key: 'name',
            header: t('list.columns.name', { defaultValue: 'Service' }),
            cell: (n) => <span className="font-strong text-tx-0">{n.name}</span>,
          },
          {
            key: 'rps',
            header: t('list.columns.rps', { defaultValue: 'RPS' }),
            cell: (n) => <span className="tabular-nums text-tx-1">{fmtRps(n.rps)}</span>,
            width: 100,
          },
          {
            key: 'error_rate',
            header: t('list.columns.error_rate', { defaultValue: 'Error rate' }),
            cell: (n) => <Pill tone={errorTone(n.error_rate)}>{fmtPct(n.error_rate)}</Pill>,
            width: 130,
          },
          {
            key: 'p95',
            header: t('list.columns.p95', { defaultValue: 'p95' }),
            cell: (n) => <span className="tabular-nums text-blue-soft">{fmtMs(n.p95_ms)}</span>,
            width: 110,
          },
          {
            key: 'spans',
            header: t('list.columns.spans', { defaultValue: 'Spans' }),
            cell: (n) => <span className="tabular-nums text-tx-3">{n.span_count.toLocaleString()}</span>,
            width: 120,
          },
        ]}
      />
    </ListPage>
  );
}

/* ─── /services/:service — service detail ─── */

export function ServiceDetail() {
  const { t } = useTranslation('services');
  const { service = '' } = useParams<{ service: string }>();
  const range = useRange();
  const q = useTopology(range);

  const node = React.useMemo<TopologyNode | undefined>(
    () => q.data?.nodes.find((n) => n.name === service),
    [q.data, service],
  );
  const upstream = React.useMemo(
    () => (q.data?.edges ?? []).filter((e) => e.target === service),
    [q.data, service],
  );
  const downstream = React.useMemo(
    () => (q.data?.edges ?? []).filter((e) => e.source === service),
    [q.data, service],
  );

  // Distinguish "topology loaded but this service isn't in it" from a real error.
  const notFound = !q.isLoading && !q.isError && !node;
  const queryState = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: node ?? null });
  const pageState: ProductStateProps | null = notFound
    ? {
        variant: 'empty',
        title: t('detail.not_found_title', { defaultValue: 'No traffic for this service' }),
        description: t('detail.not_found_description', {
          defaultValue: 'This service has no traced spans in the selected time range.',
        }),
      }
    : productStateFor(queryState, { error: q.error });

  const metadata: MetadataStripItem[] | undefined = node
    ? [
        { label: t('list.columns.rps', { defaultValue: 'RPS' }), value: <span className="tabular-nums">{fmtRps(node.rps)}</span> },
        {
          label: t('list.columns.error_rate', { defaultValue: 'Error rate' }),
          value: <Pill tone={errorTone(node.error_rate)}>{fmtPct(node.error_rate)}</Pill>,
        },
        { label: t('list.columns.p95', { defaultValue: 'p95' }), value: <span className="tabular-nums">{fmtMs(node.p95_ms)}</span> },
        { label: t('list.columns.spans', { defaultValue: 'Spans' }), value: <span className="tabular-nums">{node.span_count.toLocaleString()}</span> },
      ]
    : undefined;

  return (
    <DetailPage title={service} metadata={metadata} state={pageState}>
      {node && (
        <div className="flex flex-col gap-5">
          <DepSection
            title={t('detail.upstream', { defaultValue: 'Upstream (callers)' })}
            icon={<ArrowDownLeft className="h-3.5 w-3.5" />}
            edges={upstream}
            peer={(e) => e.source}
            time={range}
            emptyLabel={t('detail.no_upstream', { defaultValue: 'No callers in range.' })}
          />
          <DepSection
            title={t('detail.downstream', { defaultValue: 'Downstream (dependencies)' })}
            icon={<ArrowUpRight className="h-3.5 w-3.5" />}
            edges={downstream}
            peer={(e) => e.target}
            time={range}
            emptyLabel={t('detail.no_downstream', { defaultValue: 'No dependencies in range.' })}
          />
        </div>
      )}
    </DetailPage>
  );
}

function DepSection({
  title,
  icon,
  edges,
  peer,
  time,
  emptyLabel,
}: {
  title: string;
  icon: React.ReactNode;
  edges: TopologyEdge[];
  peer: (e: TopologyEdge) => string;
  time: { from: string; to: string };
  emptyLabel: string;
}) {
  const { t } = useTranslation('services');
  return (
    <section className="flex flex-col gap-2">
      <h3 className="flex items-center gap-1.5 font-sans text-xs font-strong uppercase tracking-wider text-tx-3">
        {icon}
        {title}
      </h3>
      {edges.length === 0 ? (
        <p className="font-sans text-xs text-tx-3">{emptyLabel}</p>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {edges.map((e) => (
            <li
              key={`${e.source}->${e.target}`}
              className="flex items-center gap-3 rounded border border-bd-0 bg-bg-2 px-3 py-1.5"
            >
              <span className="min-w-0 flex-1">
                <SignalReference type="service" value={peer(e)} time={time}>
                  {peer(e)}
                </SignalReference>
              </span>
              <span className="tabular-nums text-xs text-tx-2">
                {t('detail.edge_rps', { defaultValue: '{{rps}} rps', rps: fmtRps(e.rps) })}
              </span>
              <Pill tone={errorTone(e.err_rate)}>{fmtPct(e.err_rate)}</Pill>
              <span className="tabular-nums text-xs text-blue-soft">{fmtMs(e.p95_ms)}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
