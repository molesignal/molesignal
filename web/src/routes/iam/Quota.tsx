import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as quotaApi from '@/api/quota';
import { productStateFor } from '@/product/states';
import { Card, CardBody, CardHeader, Pill, uiLabelClass } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';

import { IamListPage } from './IamLayout';
import { formatMicros } from '../rum/_helpers';

type QuotaItem = {
  key: 'datasource' | 'dashboards' | 'alerts';
  label: string;
  used: number | null;
  limit: number | null;
  kind: 'bytes' | 'count';
};

export function Quota() {
  const { t } = useTranslation('iam');
  const q = useQuery({
    queryKey: ['iam', 'quota'],
    queryFn: () => quotaApi.get(),
    retry: false,
  });
  const data = q.data;
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('quota.empty_title'),
    emptyDescription: t('quota.empty_description'),
  });

  const items: QuotaItem[] = data
    ? [
        {
          key: 'datasource',
          label: t('quota.labels.datasource'),
          used: data.ingest_bytes,
          limit: data.ingest_limit_bytes,
          kind: 'bytes',
        },
        {
          key: 'dashboards',
          label: t('quota.labels.dashboards'),
          used: data.dashboards,
          limit: data.dashboards_limit,
          kind: 'count',
        },
        {
          key: 'alerts',
          label: t('quota.labels.alerts'),
          used: data.alerts,
          limit: data.alerts_limit,
          kind: 'count',
        },
      ]
    : [];

  return (
    <IamListPage
      title={t('quota.title')}
      subtitle={t('quota.subtitle') as string}
      state={pageState}
    >
      {data ? (
        <div className="space-y-3">
          <Card>
            <CardHeader
              title={t('quota.source_license')}
              actions={<Pill tone="blue">{data.edition}</Pill>}
            />
            <CardBody>
              <div className="grid gap-3 md:grid-cols-3">
                {items.map((item) => (
                  <QuotaCard key={item.key} item={item} />
                ))}
              </div>
              <div className="mt-4 rounded-md border border-bd-0 bg-bg-2 px-3 py-2">
                <div className={uiLabelClass}>{t('quota.labels.reset')}</div>
                <div className="mt-1 font-sans text-sm font-semibold text-tx-0">
                  {data.reset_at_micros ? formatMicros(data.reset_at_micros) : t('quota.not_reported')}
                </div>
              </div>
            </CardBody>
          </Card>
        </div>
      ) : null}
    </IamListPage>
  );
}

function QuotaCard({ item }: { item: QuotaItem }) {
  const { t } = useTranslation('iam');
  const limit = formatValue(item.limit, item.kind);
  const used = item.used === null ? t('quota.usage_unknown') : formatValue(item.used, item.kind);
  const value = item.limit === null ? t('quota.not_reported') : limit;

  return (
    <div className="rounded-md border border-bd-0 bg-bg-1 px-3 py-3">
      <div className={uiLabelClass}>{item.label}</div>
      <div className="mt-2 font-sans text-2xl font-display-strong leading-none text-tx-0">
        {value}
      </div>
      <div className="mt-2 font-sans text-xs font-semibold text-tx-2">
        {used} / {item.limit === null ? t('quota.limit_unknown') : limit}
      </div>
    </div>
  );
}

function formatValue(value: number | null, kind: QuotaItem['kind']): string {
  if (value === null) return '—';
  if (kind === 'count') return new Intl.NumberFormat().format(value);
  return formatBytes(value);
}

function formatBytes(value: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];
  let size = value;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  const precision = size >= 10 || unitIndex === 0 ? 0 : 1;
  return `${size.toFixed(precision)} ${units[unitIndex]}`;
}
