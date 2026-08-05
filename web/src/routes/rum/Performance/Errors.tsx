import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import { ProductState, productStateFor } from '@/product/states';
import { Card, CardBody, CardHeader, TimeRangeChip } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { useAuthStore } from '@/stores/auth';
import { useTimeStore, formatWindowSummary } from '@/stores/useTimeStore';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { windowToMicros } from '../_helpers';
import { RumListPage } from '../RumLayout';

export function PerformanceErrors() {
  const { t } = useTranslation('rum');
  const orgId = useAuthStore((s) => s.ctx?.org_id ?? '');
  const window = useTimeStore((s) => s.window);
  const range = React.useMemo(() => windowToMicros(window), [window]);

  const q = useQuery({
    queryKey: ['rum', 'error-rate', orgId, range.from_micros, range.to_micros],
    queryFn: () => rumApi.errorRateSeries({ org_id: orgId, ...range }),
    enabled: !!orgId,
  });

  const data = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data });
  const chartState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('performance.no_data'),
  });

  return (
    <RumListPage
      title={t('performance.errors')}
      toolbar={<TimeRangeChip value={formatWindowSummary(window)} />}
      performance
    >
      <Card>
        <CardHeader title={t('performance.errors')} />
        <CardBody>
          {chartState ? (
            <ProductState {...chartState} compact />
          ) : (
            <TimeSeriesChart
              height={240}
              showLegend={false}
              xDomain={[range.from_micros, range.to_micros]}
              options={{ drawStyle: 'area', fillOpacity: 0.14 }}
              series={[
                {
                  id: 'rum-errors',
                  name: t('performance.errors'),
                  color: 'var(--red)',
                  data: data.map((point) => point.count),
                  timestamps: data.map((point) => point.ts_micros),
                },
              ]}
            />
          )}
        </CardBody>
      </Card>
    </RumListPage>
  );
}
