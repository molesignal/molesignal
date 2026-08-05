import { useQuery } from '@tanstack/react-query';
import { ChevronLeft, PlayCircle } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useParams } from 'react-router-dom';

import * as rumApi from '@/api/rum';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { useAuthStore } from '@/stores/auth';
import { useTimeStore } from '@/stores/useTimeStore';

import { formatMicros, windowToMicros } from './_helpers';
import { RumDetailPage, RumSectionHeader, useRumBasePath } from './RumLayout';

export function ErrorDetail() {
  const { t } = useTranslation('rum');
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const basePath = useRumBasePath();
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const window = useTimeStore((state) => state.window);
  const range = React.useMemo(() => windowToMicros(window), [window]);

  const query = useQuery({
    queryKey: ['rum', 'error', orgId, id, range.from_micros, range.to_micros],
    queryFn: () => rumApi.getError({ org_id: orgId, fingerprint: id, ...range }),
    enabled: !!orgId && !!id,
  });

  const detail = query.data;
  const state = queryStateFor({
    isLoading: query.isLoading,
    isError: query.isError,
    data: detail,
  });
  const pageState = productStateFor(state, {
    error: query.error,
    emptyTitle: t('error_detail.no_stack'),
  });

  return (
    <RumDetailPage
      title={detail?.message || t('error_detail.title')}
      subtitle={id}
      toolbar={
        <ChromeButton onClick={() => navigate(`${basePath}/errors`)}>
          <ChevronLeft className="h-4 w-4" />
          {t('error_detail.back')}
        </ChromeButton>
      }
      state={pageState}
    >
      {detail && (
        <div className="grid gap-6 xl:grid-cols-12">
          <section className="xl:col-span-12">
            <div className="grid border-y border-bd-0 sm:grid-cols-2 xl:grid-cols-4">
              <DetailMetric label={t('error_detail.occurrences')} value={detail.count} tone="red" />
              <DetailMetric label={t('error_detail.users')} value={detail.users} />
              <DetailMetric label={t('error_detail.first_seen')} value={formatMicros(detail.first_seen_micros)} small />
              <DetailMetric label={t('error_detail.last_seen')} value={formatMicros(detail.last_seen_micros)} small />
            </div>
          </section>

          <section className="min-w-0 xl:col-span-8">
            <RumSectionHeader
              title={t('error_detail.stack')}
              description={t('error_detail.stack_description')}
            />
            {detail.stack.length === 0 ? (
              <div className="grid min-h-44 place-items-center text-sm text-tx-3">
                {t('error_detail.no_stack')}
              </div>
            ) : (
              <div className="mt-4 overflow-auto rounded-md border border-bd-0 bg-bg-2">
                {detail.stack.map((frame, index) => {
                  const file = frame.original_file ?? frame.file ?? '<anonymous>';
                  const fn = frame.original_function ?? frame.function ?? '<anonymous>';
                  const line = frame.original_line ?? frame.line ?? 0;
                  const column = frame.original_column ?? frame.column ?? 0;
                  const restored = !!frame.original_file;
                  return (
                    <div
                      key={`${file}-${line}-${column}-${index}`}
                      className="grid min-h-[54px] grid-cols-[32px_minmax(0,1fr)_auto] items-center gap-3 border-b border-bd-0 px-4 py-2.5 last:border-b-0"
                    >
                      <span className="font-mono text-xs text-tx-3">{index + 1}</span>
                      <span className="min-w-0">
                        <span className="block truncate font-mono text-xs font-strong text-tx-0">
                          {fn}
                        </span>
                        <span className="mt-1 block truncate font-mono text-xs text-tx-3">
                          {file}:{line}:{column}
                        </span>
                      </span>
                      <Pill tone={restored ? 'green' : 'dim'}>
                        {restored
                          ? t('error_detail.source_restored')
                          : t('error_detail.minified')}
                      </Pill>
                    </div>
                  );
                })}
              </div>
            )}
          </section>

          <aside className="min-w-0 xl:col-span-4">
            <RumSectionHeader title={t('error_detail.affected_context')} />
            <dl className="m-0 mt-4 grid grid-cols-[90px_minmax(0,1fr)] gap-x-3 gap-y-3 text-xs">
              <dt className="text-tx-3">{t('error_detail.pages')}</dt>
              <dd className="m-0 flex min-w-0 flex-wrap gap-1.5">
                {detail.pages.length > 0
                  ? detail.pages.map((page) => (
                      <Pill key={page} tone="neutral">{page}</Pill>
                    ))
                  : '—'}
              </dd>
              <dt className="text-tx-3">{t('error_detail.versions')}</dt>
              <dd className="m-0 flex min-w-0 flex-wrap gap-1.5">
                {detail.versions.length > 0
                  ? detail.versions.map((version) => (
                      <Pill key={version} tone="blue">{version}</Pill>
                    ))
                  : '—'}
              </dd>
            </dl>
          </aside>

          <section className="min-w-0 xl:col-span-12">
            <RumSectionHeader
              title={t('error_detail.sessions')}
              description={t('error_detail.sessions_description')}
            />
            {detail.recent_sessions.length === 0 ? (
              <div className="grid min-h-32 place-items-center text-sm text-tx-3">—</div>
            ) : (
              <div className="grid gap-x-6 sm:grid-cols-2 xl:grid-cols-3">
                {detail.recent_sessions.map((session) => (
                  <Link
                    key={session}
                    to={`${basePath}/sessions/view/${encodeURIComponent(session)}`}
                    className="group flex min-h-[68px] items-center gap-3 border-b border-bd-0 py-3 outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
                  >
                    <span className="grid h-9 w-9 place-items-center rounded-md bg-indigo-dim text-indigo-soft">
                      <PlayCircle className="h-4 w-4" />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-mono text-xs font-strong text-tx-0">
                        {session}
                      </span>
                      <span className="mt-1 block text-xs text-tx-3">
                        {t('error_detail.open_replay')}
                      </span>
                    </span>
                    <span className="text-tx-3 transition-transform group-hover:translate-x-0.5 group-hover:text-tx-1">→</span>
                  </Link>
                ))}
              </div>
            )}
          </section>
        </div>
      )}
    </RumDetailPage>
  );
}

function DetailMetric({
  label,
  value,
  tone,
  small,
}: {
  label: string;
  value: React.ReactNode;
  tone?: 'red';
  small?: boolean;
}) {
  return (
    <div className="min-h-[104px] border-b border-bd-0 p-4 sm:[&:nth-child(odd)]:border-r xl:border-b-0 xl:border-r xl:last:border-r-0">
      <div className="text-xs font-strong text-tx-3">{label}</div>
      <div
        className={`${small ? 'mt-3 text-sm' : 'mt-2 text-3xl'} font-display-strong ${
          tone === 'red' ? 'text-red-soft' : 'text-tx-0'
        }`}
      >
        {value}
      </div>
    </div>
  );
}
