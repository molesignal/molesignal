import { useQuery } from '@tanstack/react-query';
import { ArrowDown, ArrowUp, Minus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import {
  apmApi,
  apmQueryKeys,
  type ErrorSummary,
  type TransactionSummary,
  type VersionCompareResponse,
  type VersionCompareParams,
} from '@/api/apm';
import { ProductState } from '@/product/states';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

import { ApmPageFrame, QueryBoundary, Section } from './components';
import { ApmFilters } from './Filters';
import { formatCount, formatDuration, formatRate, formatSigned } from './format';
import { useApmFilters } from './useApmFilters';

const EMPTY_VERSION_VALUE = '__empty_version__';

export function ApmDeployments() {
  const { t } = useTranslation('apm');
  const [searchParams, setSearchParams] = useSearchParams();
  const { orgId, filters, params, setFilter, clearFilters } = useApmFilters();
  const baseline = searchParams.get('baseline') ?? '';
  const candidate = searchParams.get('candidate') ?? '';
  const service = filters.service;
  const serviceQuery = useQuery({
    queryKey: apmQueryKeys.service(orgId, service, params),
    queryFn: () => apmApi.service(service, params),
    enabled: Boolean(orgId && service),
    staleTime: 30_000,
  });
  const compareParams: VersionCompareParams = {
    ...params,
    service,
    baseline,
    candidate,
  };
  const ready = Boolean(orgId && service && baseline && candidate && baseline !== candidate);
  const query = useQuery({
    queryKey: apmQueryKeys.compare(orgId, compareParams),
    queryFn: () => apmApi.compareVersions(compareParams),
    enabled: ready,
    staleTime: 30_000,
  });
  const versions = serviceQuery.data?.versions.map((item) => item.version) ?? [];
  const setCompare = React.useCallback(
    (key: 'baseline' | 'candidate', value: string) => {
      const next = new URLSearchParams(searchParams);
      if (value) next.set(key, value);
      else next.delete(key);
      setSearchParams(next, { replace: true });
    },
    [searchParams, setSearchParams],
  );

  return (
    <ApmPageFrame
      title={t('deployments.title')}
      subtitle={t('deployments.subtitle')}
      meta={query.data?.meta ?? serviceQuery.data?.meta}
      toolbar={
        <ApmFilters
          filters={filters}
          setFilter={setFilter}
          clearFilters={clearFilters}
          showSearch={false}
        />
      }
    >
      <Section
        title={t('versions.selection')}
        description={t('versions.selection_description')}
      >
        <VersionSelectors
          versions={versions}
          baseline={baseline}
          candidate={candidate}
          onBaseline={(value) => setCompare('baseline', value)}
          onCandidate={(value) => setCompare('candidate', value)}
        />
      </Section>
      {!ready ? (
        <QueryBoundary
          pending={Boolean(service) && serviceQuery.isPending}
          error={service ? serviceQuery.error : null}
          empty={false}
          onRetry={() => void serviceQuery.refetch()}
        >
          <ProductState
            variant="empty"
            title={t('versions.configure_title')}
            description={t('versions.configure_description')}
          />
        </QueryBoundary>
      ) : (
        <QueryBoundary
          pending={query.isPending}
          error={query.error}
          empty={false}
          refetching={query.isFetching && Boolean(query.data)}
          onRetry={() => void query.refetch()}
        >
          {query.data && <Comparison detail={query.data} />}
        </QueryBoundary>
      )}
    </ApmPageFrame>
  );
}

function VersionSelectors({
  versions,
  baseline,
  candidate,
  onBaseline,
  onCandidate,
}: {
  versions: string[];
  baseline: string;
  candidate: string;
  onBaseline: (value: string) => void;
  onCandidate: (value: string) => void;
}) {
  const { t } = useTranslation('apm');
  const options = Array.from(new Set([baseline, candidate, ...versions].filter(Boolean)));
  return (
    <div className="flex flex-wrap items-end gap-4 p-4">
      <VersionSelect
        label={t('versions.baseline')}
        placeholder={t('versions.choose_version')}
        value={baseline}
        options={options}
        onChange={onBaseline}
      />
      <span className="pb-2 text-tx-3">→</span>
      <VersionSelect
        label={t('versions.candidate')}
        placeholder={t('versions.choose_version')}
        value={candidate}
        options={options}
        onChange={onCandidate}
      />
      {versions.length === 0 && (
        <span className="pb-2 text-xs text-tx-3">{t('versions.type_service_hint')}</span>
      )}
    </div>
  );
}

function VersionSelect({
  label,
  placeholder,
  value,
  options,
  onChange,
}: {
  label: string;
  placeholder: string;
  value: string;
  options: string[];
  onChange: (value: string) => void;
}) {
  return (
    <label className="grid gap-1">
      <span className="text-xs font-strong text-tx-3">{label}</span>
      <Select
        value={value || EMPTY_VERSION_VALUE}
        onValueChange={(next) => onChange(next === EMPTY_VERSION_VALUE ? '' : next)}
      >
        <SelectTrigger className="h-9 min-w-48 bg-bg-1 font-mono text-xs" aria-label={label}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent align="start">
          <SelectItem value={EMPTY_VERSION_VALUE} className="font-mono text-xs">
            {placeholder}
          </SelectItem>
          {options.map((version) => (
            <SelectItem key={version} value={version} className="font-mono text-xs">
              {version}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </label>
  );
}

function Comparison({ detail }: { detail: VersionCompareResponse }) {
  const { t } = useTranslation('apm');
  return (
    <div className="space-y-5">
      <Status detail={detail} />
      <div className="grid gap-px overflow-hidden rounded-lg border border-bd-0 bg-bd-0 md:grid-cols-2">
        <VersionSnapshot
          label={t('versions.baseline')}
          version={detail.baseline.version}
          sampleCount={detail.baseline.sample_count}
          red={detail.baseline.red}
        />
        <VersionSnapshot
          label={t('versions.candidate')}
          version={detail.candidate.version}
          sampleCount={detail.candidate.sample_count}
          red={detail.candidate.red}
        />
      </div>
      <Section title={t('versions.deltas')}>
        <div className="grid gap-px bg-bd-0 sm:grid-cols-2 xl:grid-cols-4">
          <Delta
            label={t('metrics.requests')}
            absolute={formatSigned(detail.delta.request_count_absolute)}
            relative={formatOptionalRate(detail.delta.request_count_relative)}
          />
          <Delta
            label={t('metrics.error_rate')}
            absolute={formatSigned(detail.delta.error_rate_absolute, true)}
            relative={formatOptionalRate(detail.delta.error_rate_relative)}
          />
          <Delta
            label={t('metrics.p95')}
            absolute={formatSigned(detail.delta.p95_absolute_micros, false, true)}
            relative={formatOptionalRate(detail.delta.p95_relative)}
          />
          <Delta
            label={t('versions.data_confidence')}
            absolute={
              detail.sufficient_data
                ? t('versions.sufficient')
                : t('versions.insufficient')
            }
          />
        </div>
      </Section>
      <Section title={t('versions.regressed_transactions')}>
        <DataTable
          rows={detail.regressed_transactions}
          columns={transactionColumns(t)}
          rowKey={(row) => `${row.version ?? ''}:${row.transaction.name}`}
          emptyLabel={t('versions.no_transaction_regressions')}
        />
      </Section>
      <Section title={t('versions.regressed_errors')}>
        <DataTable
          rows={detail.regressed_errors}
          columns={errorColumns(t)}
          rowKey={(row) => row.error.fingerprint}
          emptyLabel={t('versions.no_error_regressions')}
        />
      </Section>
    </div>
  );
}

function Status({ detail }: { detail: VersionCompareResponse }) {
  const { t } = useTranslation('apm');
  const tone = versionComparisonTone(detail.status);
  const Icon = tone === 'danger' ? ArrowUp : tone === 'good' ? ArrowDown : Minus;
  return (
    <div
      className={`flex items-center gap-3 rounded-lg border px-4 py-3 ${
        tone === 'danger'
          ? 'border-red/25 bg-red-dim text-red-soft'
          : tone === 'good'
            ? 'border-green/25 bg-green-dim text-green-soft'
            : 'border-bd-0 bg-bg-1 text-tx-2'
      }`}
    >
      <Icon aria-hidden className="h-4 w-4" />
      <div>
        <div className="text-sm font-strong">{t(`versions.status.${detail.status}`)}</div>
        <div className="mt-0.5 text-xs opacity-80">
          {detail.sufficient_data
            ? t('versions.status_sufficient')
            : t('versions.status_insufficient')}
        </div>
      </div>
    </div>
  );
}

export function versionComparisonTone(
  status: VersionCompareResponse['status'],
): 'danger' | 'good' | 'neutral' {
  if (status === 'regressed') return 'danger';
  if (status === 'improved') return 'good';
  return 'neutral';
}

function VersionSnapshot({
  label,
  version,
  sampleCount,
  red,
}: {
  label: string;
  version: string;
  sampleCount: number;
  red: VersionCompareResponse['baseline']['red'];
}) {
  const { t } = useTranslation('apm');
  return (
    <div className="bg-bg-1 p-4">
      <span className="text-xs font-strong uppercase tracking-wide text-tx-3">{label}</span>
      <div className="mt-1 font-mono text-sm font-strong text-tx-0">{version}</div>
      <div className="mt-4 grid grid-cols-3 gap-3 text-xs">
        <Metric label={t('metrics.requests')} value={formatCount(red.request_count)} />
        <Metric label={t('metrics.error_rate')} value={formatRate(red.error_rate)} />
        <Metric label={t('metrics.p95')} value={formatDuration(red.p95_micros)} />
      </div>
      <div className="mt-3 text-xs text-tx-3">
        {t('versions.sample_count', { count: sampleCount })}
      </div>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <span>
      <span className="block text-tx-3">{label}</span>
      <span className="mt-0.5 block font-mono font-strong text-tx-0">{value}</span>
    </span>
  );
}

function Delta({
  label,
  absolute,
  relative,
}: {
  label: string;
  absolute: string;
  relative?: string | undefined;
}) {
  return (
    <div className="bg-bg-1 p-4">
      <span className="block text-xs text-tx-3">{label}</span>
      <span className="mt-1 block font-mono text-base font-strong text-tx-0">{absolute}</span>
      {relative && <span className="mt-0.5 block text-xs text-tx-2">{relative}</span>}
    </div>
  );
}

function formatOptionalRate(value?: number): string | undefined {
  return value === undefined ? undefined : formatSigned(value, true);
}

function transactionColumns(
  t: (key: string) => string,
): DataTableColumn<TransactionSummary>[] {
  return [
    { key: 'name', header: t('columns.transaction'), cell: (row) => row.transaction.name },
    { key: 'requests', header: t('columns.requests'), cell: (row) => formatCount(row.red.request_count) },
    { key: 'errors', header: t('columns.error_rate'), cell: (row) => formatRate(row.red.error_rate) },
    { key: 'p95', header: t('columns.p95'), cell: (row) => formatDuration(row.red.p95_micros) },
  ];
}

function errorColumns(t: (key: string) => string): DataTableColumn<ErrorSummary>[] {
  return [
    { key: 'error', header: t('columns.error'), cell: (row) => row.error.error_type },
    { key: 'count', header: t('columns.occurrences'), cell: (row) => formatCount(row.occurrence_count) },
    { key: 'service', header: t('columns.service'), cell: (row) => row.service.name },
  ];
}
