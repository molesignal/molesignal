import { useQuery } from '@tanstack/react-query';
import { Combine, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { apmApi, apmQueryKeys, type ErrorIssueFilter } from '@/api/apm';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { ApmNavigation } from '../frame/Navigation';
import { ApmCard, ApmPage, ApmQueryState } from '../frame/Page';
import { ApmQualityChip } from '../frame/QualityChip';
import { ApmTimeControls } from '../frame/TimeControls';
import { TraceDrawerHost } from '../frame/TraceDrawerHost';
import { useAddressedDrawer, useApmScope } from '../frame/useApmScope';
import { apmQueryParams } from '../scope';
import { ErrorBreakdownCard } from './Breakdown';
import { ErrorDrawer } from './ErrorDrawer';
import { type ErrorRulesView, ErrorRulesSheet } from './ErrorRules';
import { ErrorTable } from './ErrorTable';
import { emptyRule } from './rules';

const STATUSES: readonly ErrorIssueFilter[] = ['unresolved', 'investigating', 'resolved', 'ignored', 'all'];

/**
 * Error groups as a list of issues, the module-wide view or one service's:
 * those still open by default (regressions and those being investigated with
 * them), the ones resolved or ignored a choice away, and where the listed
 * groups happened above them. A group opens in a drawer; the rules that
 * group errors open in a sheet.
 */
export function ErrorList({ service }: { service?: { namespace: string; name: string; version: string } }) {
  const { t } = useTranslation('apm');
  const { scope, setScope, range, orgId } = useApmScope();
  const drawer = useAddressedDrawer('error');
  const [rulesView, setRulesView] = React.useState<ErrorRulesView | null>(null);
  const params = apmQueryParams(scope, range, {
    status: scope.status,
    limit: 200,
    sort: 'last_seen',
    direction: 'desc',
    ...(service
      ? {
          service: service.name,
          namespace: service.namespace,
          ...(service.version ? { version: service.version } : {}),
        }
      : {}),
  });

  const errors = useQuery({
    queryKey: apmQueryKeys.errors(orgId, params),
    queryFn: () => apmApi.errors(params),
    enabled: Boolean(orgId),
    refetchInterval: 60_000,
  });

  const needle = scope.search.trim().toLocaleLowerCase();
  const rows = React.useMemo(
    () =>
      (errors.data?.items ?? []).filter(
        (group) =>
          !needle ||
          group.error.errorType.toLocaleLowerCase().includes(needle) ||
          (group.representativeMessage ?? '').toLocaleLowerCase().includes(needle) ||
          group.service.name.toLocaleLowerCase().includes(needle),
      ),
    [errors.data, needle],
  );

  const breakdown = errors.data?.breakdown;

  return (
    <div className="space-y-[12px]">
      {breakdown && (errors.data?.items.length ?? 0) > 0 && (
        <ErrorBreakdownCard breakdown={breakdown} description={t('errors.breakdown.listed')} testId="errors-breakdown" />
      )}
      <ApmCard
        testId="errors"
        title={
          <span className="flex items-center gap-2">
            <label className="relative">
              <span className="sr-only">{t('errors.search')}</span>
              <Search aria-hidden className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-tx-3" />
              <input
                value={scope.search}
                onChange={(event) => setScope({ search: event.target.value })}
                placeholder={t('errors.search')}
                className="h-8 w-60 rounded-md border-0 bg-[var(--control-surface)] pl-8 pr-2.5 font-sans text-xs font-body text-tx-1 outline-none placeholder:text-tx-3 hover:bg-bg-3 focus-visible:bg-bg-3"
              />
            </label>
            <FittedSelect
              aria-label={t('errors.status_filter')}
              data-apm-error-status=""
              value={scope.status}
              onValueChange={(status) => setScope({ status: status as ErrorIssueFilter })}
              options={STATUSES.map((value) => ({
                value,
                label: t(`errors.filter.${value}`),
              }))}
              maxWidth={160}
              className="h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 hover:bg-bg-3"
            />
            <span className="font-sans text-xs text-tx-3">{t('errors.count', { count: rows.length })}</span>
          </span>
        }
        actions={
          <span className="flex items-center gap-2">
            <ApmQualityChip meta={errors.data?.meta} />
            <button
              type="button"
              onClick={() => setRulesView({ mode: 'list' })}
              data-apm-error-rules-open=""
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
            >
              <Combine aria-hidden className="h-3.5 w-3.5 text-[var(--blue-ink)]" />
              {t('errors.rules.open')}
            </button>
          </span>
        }
      >
        <ApmQueryState
          query={errors}
          empty={rows.length === 0}
          emptyTitle={t(scope.status === 'unresolved' ? 'errors.none_open' : 'errors.none')}
          emptyDescription={t('errors.none_description')}
        >
          <ErrorTable errors={rows} showService={!service} onOpen={(group) => drawer.open(group.error.fingerprint)} />
        </ApmQueryState>
      </ApmCard>
      <ErrorRulesSheet view={rulesView} onView={setRulesView} groups={errors.data?.items ?? []} newRule={() => emptyRule(service)} />
    </div>
  );
}

/** The module's error tab: every service's error groups. */
export function ApmErrors() {
  const { t } = useTranslation('apm');
  const { scope, setScope } = useApmScope();

  return (
    <TraceDrawerHost>
      <ApmPage
        page="errors"
        title={t('title')}
        subtitle={t('subtitle')}
        breadcrumbs={null}
        navigation={<ApmNavigation end={<ApmTimeControls scope={scope} onScopeChange={setScope} />} />}
      >
        <ErrorList />
        <ErrorDrawer />
      </ApmPage>
    </TraceDrawerHost>
  );
}
