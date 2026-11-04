import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as limitsApi from '@/api/pipelines/limits';
import { ProductState } from '@/product/states';

import { MIB, quotaLimit } from './model';
import type { PipelineGraphModel, TransformStep } from '../PipelineGraph/model';
import { PipelineSection, pipelineFlatTableClassName } from '../Surfaces';

/** One bucket of a limit step today, with what the step's settings say about it. */
interface UsageRow {
  key: string;
  step: TransformStep;
  bucket: string;
  usage: Omit<limitsApi.LimitBucket, 'step' | 'bucket'>;
}

const NOTHING = { admittedEvents: 0, admittedBytes: 0, overEvents: 0, overBytes: 0 };

/**
 * What a realtime pipeline's quotas and throttles let through and held back
 * today (UTC), for every node together. Shown only where there is one.
 */
export function PipelineLimits({ pipelineId, graph }: { pipelineId: string; graph: PipelineGraphModel }) {
  const { t, i18n } = useTranslation('pipelines');
  const limits =
    graph.mode === 'realtime' ? graph.transforms.filter((step) => step.kind === 'quota' || step.kind === 'throttle') : [];
  const query = useQuery({
    queryKey: ['pipeline-limits', pipelineId],
    queryFn: () => limitsApi.today(pipelineId),
    enabled: limits.length > 0,
    refetchInterval: 30_000,
  });

  if (limits.length === 0) return null;

  const rows: UsageRow[] = limits.flatMap((step) => {
    const stored = (query.data?.buckets ?? []).filter((bucket) => bucket.step === step.name.trim());
    const buckets = stored.length > 0 ? stored : [{ step: step.name, bucket: '', ...NOTHING }];

    return buckets.map((item) => ({
      key: `${step.name}\u0000${item.bucket}`,
      step,
      bucket: item.bucket,
      usage: {
        admittedEvents: item.admittedEvents,
        admittedBytes: item.admittedBytes,
        overEvents: item.overEvents,
        overBytes: item.overBytes,
      },
    }));
  });
  const number = (value: number) => value.toLocaleString(i18n.language);
  const size = (bytes: number) => t('counting.usage.mib', { value: (bytes / MIB).toLocaleString(i18n.language, { maximumFractionDigits: 1 }) });

  return (
    <PipelineSection
      title={t('counting.usage.title')}
      description={t('counting.usage.description')}
      bodyClassName="pt-0"
    >
      {query.isError ? (
        <ProductState variant="error" error={query.error} compact className={pipelineFlatTableClassName} />
      ) : (
        <DataTable
          className={pipelineFlatTableClassName}
          loading={query.isLoading}
          rows={rows}
          rowKey={(row) => row.key}
          columns={[
            {
              key: 'step',
              header: t('counting.usage.step'),
              width: 200,
              cell: (row) => (
                <div>
                  <div className="font-strong text-tx-1">{row.step.name}</div>
                  <div className="mt-0.5 font-sans text-xs text-tx-3">{t(`steps.types.${row.step.kind}`)}</div>
                </div>
              ),
            },
            {
              key: 'bucket',
              header: t('counting.usage.bucket'),
              width: 220,
              cell: (row) => (
                <span className="font-mono text-xs text-tx-2">
                  {bucketLabel(row.bucket, row.step.quota?.partitionBy ?? [], t)}
                </span>
              ),
            },
            {
              key: 'admitted',
              header: t('counting.usage.admitted'),
              width: 220,
              cell: (row) => {
                const quota = row.step.quota;
                const bytes = quota?.unit === 'bytes';
                const used = bytes ? size(row.usage.admittedBytes) : number(row.usage.admittedEvents);
                const limit = limitOf(row);

                return (
                  <span className="font-sans tabular-nums text-tx-1">
                    {limit === null ? used : t('counting.usage.of_limit', { used, limit: bytes ? size(limit) : number(limit) })}
                  </span>
                );
              },
            },
            {
              key: 'over',
              header: t('counting.usage.over'),
              width: 220,
              cell: (row) => {
                const bytes = row.step.quota?.unit === 'bytes';
                const over = bytes ? size(row.usage.overBytes) : number(row.usage.overEvents);
                const action = row.step.quota?.action ?? row.step.throttle?.action ?? 'drop';

                return (
                  <span className={row.usage.overEvents > 0 ? 'font-sans tabular-nums text-yellow' : 'font-sans tabular-nums text-tx-3'}>
                    {over}
                    <span className="text-tx-3"> · {t(`counting.over_actions.${action}`)}</span>
                  </span>
                );
              },
            },
          ]}
        />
      )}
    </PipelineSection>
  );
}

/** A quota's limit for the bucket in its own unit (bytes for a quota in bytes); a throttle has none a day. */
function limitOf(row: UsageRow): number | null {
  const quota = row.step.quota;
  if (row.step.kind !== 'quota' || !quota) return null;

  const fields = quota.partitionBy.map((field) => field.trim()).filter(Boolean);
  const own = quota.overrides.find(
    (override) => JSON.stringify(fields.map((field) => override.values[field] ?? null)) === row.bucket,
  );

  return quotaLimit(own?.limit ?? quota.limit, quota.unit);
}

type Translate = TFunction<'pipelines'>;

/** A bucket as people read it: `service=cart · env=prod`, the rest past the cap, or the whole step. */
function bucketLabel(bucket: string, fields: readonly string[], t: Translate): string {
  if (bucket === '') return t('counting.usage.whole');
  if (bucket === '*') return t('counting.usage.others');

  const named = fields.map((field) => field.trim()).filter(Boolean);

  try {
    const values: unknown = JSON.parse(bucket);
    if (Array.isArray(values)) {
      return values
        .map((value, index) => `${named[index] ?? index + 1}=${value === null ? '∅' : String(value)}`)
        .join(' · ');
    }
  } catch {
    // A bucket that is not a list is shown as stored.
  }

  return bucket;
}
