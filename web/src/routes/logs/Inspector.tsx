import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Link, useSearchParams } from 'react-router-dom';

import * as searchJobsApi from '@/api/searchJobs';
import { formatMicrosActive } from '@/lib/time';
import { type ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { queryStateFor } from '@/shell/query/State';

function KvRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[200px_1fr] gap-3 border-b border-bd-0 py-3 last:border-b-0">
      <div className="font-sans text-xs font-strong text-tx-1">{label}</div>
      <div className="font-sans text-xs text-tx-0">{children}</div>
    </div>
  );
}

export function LogsInspector() {
  const { t } = useTranslation('logs');
  const [params] = useSearchParams();
  const id = params.get('id');
  const q = useQuery({
    queryKey: ['search-job', id],
    queryFn: () => searchJobsApi.get(id ?? ''),
    enabled: !!id,
  });
  const data = q.data;
  const state = id ? queryStateFor({ isLoading: q.isLoading, isError: q.isError, data }) : null;
  const pageState: ProductStateProps | null =
    !id
      ? {
          variant: 'empty',
          title: t('inspector.pick_title'),
          description: t('inspector.pick_description'),
          action: <Link to="/logs" className="text-blue-soft hover:underline">{t('inspector.back')}</Link>,
        }
      : state === 'loading'
        ? { variant: 'loading' }
        : state === 'error'
          ? { variant: 'error', error: q.error }
          : state === 'empty'
            ? {
                variant: 'empty',
                title: t('inspector.not_found_title'),
                description: t('inspector.not_found_description'),
              }
            : null;

  return (
    <DetailPage
      title={t('inspector.title')}
      subtitle={t('inspector.subtitle')}
      metadata={[
        { label: t('inspector.back'), value: <Link to="/logs" className="text-blue-soft hover:underline">{t('inspector.back')}</Link> },
        ...(id ? [{ label: t('inspector.fields.job_id'), value: id }] : []),
        ...(data ? [{ label: t('inspector.fields.state'), value: data.state }] : []),
      ]}
      state={pageState}
    >
      {data && (
        <div className="rounded-md border border-bd-0 bg-bg-1 px-3">
          <KvRow label={t('inspector.fields.job_id')}>{data.job_id}</KvRow>
          <KvRow label={t('inspector.fields.state')}>{data.state}</KvRow>
          <KvRow label={t('inspector.fields.submitted_at')}>{formatMicrosActive(data.submitted_at_micros)}</KvRow>
          <KvRow label={t('inspector.fields.started_at')}>{formatMicrosActive(data.started_at_micros)}</KvRow>
          <KvRow label={t('inspector.fields.finished_at')}>{formatMicrosActive(data.finished_at_micros)}</KvRow>
          <KvRow label={t('inspector.fields.result_rows')}>{data.result_rows ?? '-'}</KvRow>
          <KvRow label={t('inspector.fields.result_object_key')}>{data.result_object_key ?? '-'}</KvRow>
          <KvRow label={t('inspector.fields.error')}>{data.error ?? '-'}</KvRow>
          <KvRow label={t('inspector.fields.expires_at')}>{formatMicrosActive(data.expires_at_micros)}</KvRow>
        </div>
      )}
    </DetailPage>
  );
}
