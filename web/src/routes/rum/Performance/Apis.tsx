import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as rumApi from '@/api/rum';
import { productStateFor } from '@/product/states';
import { Pill, TimeRangeChip } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { useAuthStore } from '@/stores/auth';
import { useTimeStore, formatWindowSummary } from '@/stores/useTimeStore';

import { windowToMicros } from '../_helpers';
import { RumListPage } from '../RumLayout';

export function Apis() {
  const { t } = useTranslation('rum');
  const orgId = useAuthStore((s) => s.ctx?.org_id ?? '');
  const window = useTimeStore((s) => s.window);
  const range = React.useMemo(() => windowToMicros(window), [window]);

  const q = useQuery({
    queryKey: ['rum', 'apis', orgId, range.from_micros, range.to_micros],
    queryFn: () => rumApi.apiPerformance({ org_id: orgId, ...range }),
    enabled: !!orgId,
  });

  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('performance.no_data'),
  });

  return (
    <RumListPage
      title={t('performance.apis')}
      toolbar={<TimeRangeChip value={formatWindowSummary(window)} />}
      performance
      state={pageState}
    >
      <DataTable
        rows={rows}
        rowKey={(r) => r.url}
        columns={[
          { key: 'url', header: t('performance.columns.url'), cell: (r) => r.url },
          { key: 'count', header: t('performance.columns.count'), cell: (r) => r.count, width: 90 },
          { key: 'p50', header: t('performance.columns.p50'), cell: (r) => Math.round(r.p50_ms), width: 90 },
          { key: 'p95', header: t('performance.columns.p95'), cell: (r) => Math.round(r.p95_ms), width: 90 },
          {
            key: 'err',
            header: t('performance.columns.errors'),
            cell: (r) => {
              const pct = (r.err_rate * 100).toFixed(1);
              return r.err_rate > 0.01 ? (
                <Pill tone="red">{pct}%</Pill>
              ) : (
                <span className="text-tx-3">{pct}%</span>
              );
            },
            width: 100,
          },
        ]}
      />
    </RumListPage>
  );
}
