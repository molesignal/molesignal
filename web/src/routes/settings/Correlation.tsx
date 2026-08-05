import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import { DataTable, PageHeader } from '@/admin';
import * as webApi from '@/api/web';
import { ProductState, productStateFor } from '@/product/states';
import { Pill } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';

import { SectionBody } from './_atoms';

export function Correlation() {
  const { t } = useTranslation('settings-admin');
  const providersQuery = useQuery({
    queryKey: ['settings', 'correlation-providers'],
    queryFn: () => webApi.correlationProviders(),
  });
  const state = productStateFor(queryStateFor({
    isLoading: providersQuery.isLoading,
    isError: providersQuery.isError,
    data: providersQuery.data ?? [],
  }), {
    error: providersQuery.error,
    emptyTitle: t('correlation.empty_title'),
    emptyDescription: t('correlation.empty_description'),
  });

  return (
    <>
      <PageHeader title={t('correlation.title')} subtitle={t('correlation.subtitle') as string} />
      <SectionBody>
        {state ? (
          <ProductState {...state} />
        ) : (
          <DataTable
            rows={providersQuery.data ?? []}
            rowKey={(row) => row.id}
            columns={[
              {
                key: 'label',
                header: t('correlation.columns.provider'),
                cell: (row) => <span className="text-tx-0">{row.label}</span>,
              },
              {
                key: 'from',
                header: t('correlation.columns.from'),
                cell: (row) => row.from_kind,
                width: 140,
              },
              {
                key: 'to',
                header: t('correlation.columns.to'),
                cell: (row) => row.to_kind,
                width: 140,
              },
              {
                key: 'enabled',
                header: t('correlation.columns.enabled'),
                cell: (row) => (
                  <Pill tone={row.enabled ? 'green' : 'dim'}>
                    {row.enabled ? t('correlation.enabled') : t('correlation.disabled')}
                  </Pill>
                ),
                width: 140,
              },
            ]}
          />
        )}
      </SectionBody>
    </>
  );
}
