import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as searchJobsApi from '@/api/searchJobs';
import type { SearchJob } from '@/api/searchJobs';
import { toApiError } from '@/lib/http';
import { canAccessProductPath, useProductAccess } from '@/product/access';
import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { Dot } from '@/shell/chrome';
import { ResultPagination } from '@/shell/ResultPagination';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { toast } from '@/shell/ui/sonner';

import { canUseSearchJobs, seesEverySearchJob } from './access';
import { downloadJobResults } from './download';
import { JobTable } from './JobTable';
import {
  ACTIVE_REFRESH_MS,
  DEFAULT_JOB_PAGE_SIZE,
  JOB_FILTERS,
  JOB_PAGE_SIZES,
  filterTotal,
  hasActiveJobs,
  isActiveState,
  jobFilterFromParam,
  pageCountFor,
  refreshInterval,
  type JobFilter,
} from './model';

const JOBS_KEY = ['search-jobs'] as const;

/**
 * Search jobs: the queries that run in the background, and the backfills of
 * data pipelines, which go through the same queue. A job belongs to whoever
 * submitted it, so a reader is shown their own; an administrator is shown the
 * jobs of everybody in the organization. The backend decides which, the page
 * only says so.
 *
 * The page is for seeing what is under way, so it reads itself again every few
 * seconds while a job waits or runs, and only now and then otherwise. A page
 * nobody is looking at is not read at all.
 */
export function SearchJobs() {
  const { t } = useTranslation('streams');
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const allowed = canUseSearchJobs(access);
  const everyone = seesEverySearchJob(access);
  const [searchParams, setSearchParams] = useSearchParams();
  const filter = jobFilterFromParam(searchParams.get('state'));
  const [page, setPage] = React.useState(1);
  const [pageSize, setPageSize] = React.useState(DEFAULT_JOB_PAGE_SIZE);
  const [cancelling, setCancelling] = React.useState<SearchJob | null>(null);
  const [deleting, setDeleting] = React.useState<SearchJob | null>(null);

  // The counts and the page of jobs are read together and on one clock. Read apart, the counts
  // would say a job has finished while its row went on saying that it runs.
  const query = useQuery({
    queryKey: [...JOBS_KEY, filter, page, pageSize],
    queryFn: async () => {
      const [summary, jobs] = await Promise.all([
        searchJobsApi.summary(),
        searchJobsApi.list({
          states: filter === 'all' ? [] : [filter],
          limit: pageSize,
          offset: (page - 1) * pageSize,
        }),
      ]);
      return { summary, jobs };
    },
    enabled: allowed,
    staleTime: 0,
    // Another state or page keeps the counts on screen while its own jobs are read.
    placeholderData: keepPreviousData,
    refetchOnWindowFocus: true,
    refetchInterval: (current) => refreshInterval(hasActiveJobs(current.state.data?.summary)),
  });
  const summary = query.data?.summary;
  const reading = !query.data || query.isPlaceholderData;
  const jobs = reading ? [] : query.data.jobs;
  const active = hasActiveJobs(summary);
  const total = summary ? filterTotal(filter, summary) : 0;
  const pageCount = pageCountFor(total, pageSize);

  // Deleting the last job of a page leaves that page behind the end of the list.
  React.useEffect(() => {
    if (summary) setPage((current) => Math.min(current, pageCount));
  }, [pageCount, summary]);

  const refresh = () => queryClient.invalidateQueries({ queryKey: JOBS_KEY });
  const onError = (error: unknown) => toast.error(toApiError(error).message);
  const cancel = useMutation({
    mutationFn: (job: SearchJob) => searchJobsApi.cancel(job.job_id),
    onSuccess: () => {
      toast.success(t('jobs.toast.cancelled'));
      setCancelling(null);
      return refresh();
    },
    onError,
  });
  const retry = useMutation({
    mutationFn: (job: SearchJob) => searchJobsApi.retry(job.job_id),
    onSuccess: () => {
      toast.success(t('jobs.toast.retried'));
      return refresh();
    },
    onError,
  });
  const remove = useMutation({
    mutationFn: (job: SearchJob) => searchJobsApi.remove(job.job_id),
    onSuccess: () => {
      toast.success(t('jobs.toast.deleted'));
      setDeleting(null);
      return refresh();
    },
    onError,
  });
  const download = useMutation({
    mutationFn: downloadJobResults,
    onSuccess: (count) => toast.success(t('jobs.toast.downloaded', { count })),
    onError,
  });
  const busyId =
    [cancel, retry, remove, download].find((mutation) => mutation.isPending)?.variables?.job_id ??
    null;

  const changeFilter = (next: JobFilter) => {
    setPage(1);
    setSearchParams(
      (current) => {
        const params = new URLSearchParams(current);
        if (next === 'all') params.delete('state');
        else params.set('state', next);
        return params;
      },
      { replace: true },
    );
  };

  const noJobs = summary?.total === 0;
  const state: ProductStateProps | null = !allowed
    ? {
        variant: 'permission-denied',
        title: t('jobs.denied.title'),
        description: t('jobs.denied.description'),
      }
    : query.isError
      ? { variant: 'error', error: query.error }
      : reading
        ? { variant: 'loading' }
        : noJobs
          ? {
              variant: 'empty',
              title: t(everyone ? 'jobs.empty.title' : 'jobs.empty.title_own'),
              description: <JobSources />,
            }
          : jobs.length === 0 && filter !== 'all'
            ? {
                variant: 'empty',
                compact: true,
                title: t(`jobs.empty.filtered.${filter}`),
                description: t('jobs.empty.filtered_hint'),
              }
            : null;

  return (
    <>
      <ListPage
        appearance="surface"
        title={t('jobs.title')}
        subtitle={t('jobs.subtitle')}
        state={state}
        listHead={
          summary && !noJobs ? (
            <>
              <ViewTabs<JobFilter>
                label={t('jobs.views_label')}
                value={filter}
                onValueChange={changeFilter}
                items={JOB_FILTERS.map((item) => ({
                  key: item,
                  label: t(`jobs.states.${item}`),
                  count: filterTotal(item, summary),
                }))}
              />
              <div className="ml-auto flex items-center gap-x-[12px] whitespace-nowrap font-sans text-xs">
                {!everyone && (
                  <span data-search-jobs-scope="own" className="text-tx-3">
                    {t('jobs.scope_own')}
                  </span>
                )}
                {active && (
                  <span data-search-jobs-live className="flex items-center gap-[6px] text-tx-2">
                    <Dot tone="blue" />
                    {t('jobs.live', { seconds: ACTIVE_REFRESH_MS / 1000 })}
                  </span>
                )}
              </div>
            </>
          ) : undefined
        }
      >
        <JobTable
          jobs={jobs}
          nowMicros={Date.now() * 1000}
          busyId={busyId}
          pipelinesVisible={canAccessProductPath('/pipelines/job', access)}
          showSubmitter={everyone}
          emptyLabel={t('jobs.empty.title')}
          onCancel={setCancelling}
          onRetry={(job) => retry.mutate(job)}
          onDownload={(job) => download.mutate(job)}
          onDelete={setDeleting}
        />
        {total > JOB_PAGE_SIZES[0]! && (
          <ResultPagination
            page={page}
            pageCount={pageCount}
            pageSize={pageSize}
            pageSizeOptions={JOB_PAGE_SIZES}
            pageLabel={t('jobs.pagination.page_summary', { page, pages: pageCount })}
            ariaLabel={t('jobs.pagination.aria')}
            pageSizeAriaLabel={t('jobs.pagination.page_size_aria')}
            firstAriaLabel={t('jobs.pagination.first_aria')}
            previousAriaLabel={t('jobs.pagination.previous_aria')}
            nextAriaLabel={t('jobs.pagination.next_aria')}
            lastAriaLabel={t('jobs.pagination.last_aria')}
            onPageChange={setPage}
            onPageSizeChange={(next) => {
              setPageSize(next);
              setPage(1);
            }}
            className="bg-transparent px-[12px]"
          />
        )}
      </ListPage>

      <ConfirmDialog
        open={cancelling !== null}
        onOpenChange={(open) => !open && setCancelling(null)}
        title={t('jobs.cancel_confirm.title')}
        description={t('jobs.cancel_confirm.description')}
        confirmLabel={t('jobs.cancel_confirm.confirm')}
        cancelLabel={t('jobs.cancel_confirm.keep')}
        busyLabel={t('jobs.cancel_confirm.busy')}
        destructive
        busy={cancel.isPending}
        onConfirm={() => cancelling && cancel.mutate(cancelling)}
      />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={t('jobs.delete_confirm.title')}
        description={
          deleting && isActiveState(deleting.state)
            ? t('jobs.delete_confirm.description_active')
            : t('jobs.delete_confirm.description')
        }
        confirmLabel={t('jobs.delete_confirm.confirm')}
        cancelLabel={t('actions.cancel', { ns: 'common' })}
        busyLabel={t('jobs.delete_confirm.busy')}
        destructive
        busy={remove.isPending}
        onConfirm={() => deleting && remove.mutate(deleting)}
      />
    </>
  );
}

/** Where jobs come from, for a list that has none yet. */
function JobSources() {
  const { t } = useTranslation('streams');

  return (
    <>
      {t('jobs.empty.description')}
      <ul className="mx-auto mt-[8px] w-fit max-w-full list-disc space-y-[4px] pl-[18px] text-left">
        <li>{t('jobs.empty.source_async')}</li>
        <li>{t('jobs.empty.source_backfill')}</li>
        <li>{t('jobs.empty.source_agent')}</li>
      </ul>
    </>
  );
}
