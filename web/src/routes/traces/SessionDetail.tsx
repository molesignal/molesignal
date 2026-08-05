import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useParams } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as queryApi from '@/api/query';
import * as streamsApi from '@/api/streams';
import { type ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { queryStateFor } from '@/shell/query/State';
import { SignalReference } from '@/shell/SignalReference';
import { useAuthStore } from '@/stores/auth';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

interface SessionTraceRow {
  trace_id: string;
  service: string;
  operation: string;
  start_ns: number;
  duration_ms: number;
}

export function TraceSessionDetail() {
  const { t } = useTranslation('traces');
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const orgId = useAuthStore((s) => s.ctx?.org_id ?? '');

  // Use the existing /query endpoint to fetch traces tagged with this session id.
  const q = useQuery({
    queryKey: ['trace-session', id],
    queryFn: async () => {
      const now = Date.now() * 1000;
      const sevenDays = 7 * 24 * 3600 * 1_000_000;
      if (!id) return [];
      const streams = await streamsApi.list(500);
      const traceStream =
        streams.find(
          (stream) =>
            stream.type === 'traces' && stream.name === 'default' && streamsApi.isQueryable(stream),
        ) ?? streams.find((stream) => stream.type === 'traces' && streamsApi.isQueryable(stream));
      if (!traceStream) return [];
      if (!traceStream.schema.fields.some((field) => field.name === 'session.id')) return [];

      const sql = `SELECT trace_id, MIN("service.name") AS service, MIN("name") AS operation,
        MIN(start_time_unix_nano) AS start_ns,
        (MAX(end_time_unix_nano) - MIN(start_time_unix_nano)) / 1000000.0 AS duration_ms
        FROM ${sqlIdentifier(traceStream.name)}
        WHERE "session.id" = ${sqlLiteral(id)}
        GROUP BY trace_id
        ORDER BY start_ns ASC`;
      const result = await queryApi.runQuery({
        org_id: orgId,
        language: 'sql',
        statement: sql,
        time_range: { start: now - sevenDays, end: now },
        stream: { name: traceStream.name, stream_type: 'traces' },
      });
      const idx = (col: string) => result.columns.indexOf(col);
      const cols = {
        trace_id: idx('trace_id'),
        service: idx('service'),
        operation: idx('operation'),
        start_ns: idx('start_ns'),
        duration_ms: idx('duration_ms'),
      };
      return result.rows.map((r): SessionTraceRow => ({
        trace_id: String(r[cols.trace_id] ?? ''),
        service: String(r[cols.service] ?? ''),
        operation: String(r[cols.operation] ?? ''),
        start_ns: Number(r[cols.start_ns] ?? 0),
        duration_ms: Number(r[cols.duration_ms] ?? 0),
      }));
    },
    enabled: !!id,
  });

  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState: ProductStateProps | null =
    !id
      ? {
          variant: 'empty',
          title: t('session.empty_title'),
          description: t('detail.missing_description'),
        }
      : state === 'loading'
        ? { variant: 'loading' }
        : state === 'error'
          ? { variant: 'error', error: q.error }
          : state === 'empty'
            ? {
                variant: 'empty',
                title: t('session.empty_title'),
                description: t('session.empty_description'),
              }
            : null;

  return (
    <DetailPage
      title={t('session.title')}
      metadata={[
        { label: t('session.back'), value: <Link to="/traces" className="text-indigo-soft hover:underline">{t('session.back')}</Link> },
        ...(id
          ? [{
              label: t('session.session_id'),
              // Session id isn't a true trace_id, but it's a copyable
              // handle SREs hand off to support workflows.
              value: <SignalReference type="trace_id" value={id}>{id}</SignalReference>,
            }]
          : []),
        { label: t('session.result_count', { count: rows.length }), value: String(rows.length) },
      ]}
      state={pageState}
    >
      <DataTable
        rows={rows}
        rowKey={(row) => row.trace_id}
        onRowClick={(row) => navigate(`/traces/${encodeURIComponent(row.trace_id)}`)}
        columns={[
          {
            key: 'trace_id',
            header: t('session.columns.trace_id'),
            cell: (row) => (
              <SignalReference type="trace_id" value={row.trace_id}>
                {row.trace_id.slice(0, 16)}
              </SignalReference>
            ),
          },
          {
            key: 'service',
            header: t('session.columns.service'),
            cell: (row) => row.service ? (
              <SignalReference type="service" value={row.service}>
                {row.service}
              </SignalReference>
            ) : '—',
          },
          {
            key: 'operation',
            header: t('session.columns.operation'),
            cell: (row) => <TraceOperationName operation={row.operation} />,
          },
          {
            key: 'duration_ms',
            header: t('session.columns.duration_ms'),
            cell: (row) => row.duration_ms.toFixed(1),
            className: 'text-right',
            headerClassName: 'text-right',
            width: 160,
          },
        ]}
      />
    </DetailPage>
  );
}

function sqlIdentifier(value: string): string {
  return `"${value.replace(/"/g, '""')}"`;
}

function sqlLiteral(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}
