import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { apmApi, apmQueryKeys } from '@/api/apm';
import { FormField, FormSelect } from '@/shell/FormDrawer';
import { useAuthStore } from '@/stores/auth';
import type { ApmAlertMetric, ApmAlertQuery } from '@/types/alerting';

import { APM_METRICS, APM_TRANSACTION_KINDS, apmMetricUnit } from './apmSource';

const DAY_MICROS = 86_400_000_000;
const SEPARATOR = '\u241f';

/**
 * Which APM figure a rule watches: a service in an environment, the whole of
 * it or one endpoint, and the figure, said with the unit its thresholds are
 * set in. Services and endpoints are the ones APM saw over the last day; the
 * one a rule already names stays a choice even when it was not seen.
 */
export function ApmSourceFields({
  draft,
  onChange,
  metrics = APM_METRICS,
}: {
  draft: ApmAlertQuery;
  onChange: (next: ApmAlertQuery) => void;
  /** The figures offered; an anomaly rule watches fewer. */
  metrics?: readonly ApmAlertMetric[];
}) {
  const { t } = useTranslation('alerts');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const [range] = React.useState(() => {
    const to = Date.now() * 1_000;
    return { from: to - DAY_MICROS, to };
  });
  const params = { ...range, limit: 200, sort: 'request_count', direction: 'desc' as const };

  const services = useQuery({
    queryKey: apmQueryKeys.services(orgId, params),
    queryFn: () => apmApi.services(params),
    enabled: Boolean(orgId),
  });
  const detailParams = {
    ...range,
    ...(draft.namespace ? { namespace: draft.namespace } : {}),
    ...(draft.environment ? { environment: draft.environment } : {}),
  };
  const detail = useQuery({
    queryKey: apmQueryKeys.service(orgId, draft.service, detailParams),
    queryFn: () => apmApi.service(draft.service, detailParams),
    enabled: Boolean(orgId && draft.service && draft.environment),
  });

  const listed = services.data?.items ?? [];
  const serviceValue = `${draft.namespace ?? ''}${SEPARATOR}${draft.service}`;
  const serviceOptions = unique([
    ...(draft.service ? [{ value: serviceValue, label: label(draft.namespace, draft.service) }] : []),
    ...listed.map((item) => ({
      value: `${item.service.namespace}${SEPARATOR}${item.service.name}`,
      label: label(item.service.namespace, item.service.name),
    })),
  ]);
  const environments = unique([
    ...(draft.environment ? [draft.environment] : []),
    ...listed
      .filter((item) => item.service.name === draft.service && (!draft.namespace || item.service.namespace === draft.namespace))
      .map((item) => item.service.environment),
  ].map((value) => ({ value, label: value })));
  const endpointValue = draft.transaction ? `${draft.transaction.kind}${SEPARATOR}${draft.transaction.name}` : '';
  const endpointOptions = unique([
    { value: '', label: t('workbench.apm.whole_service') },
    ...(draft.transaction ? [{ value: endpointValue, label: draft.transaction.name }] : []),
    ...(detail.data?.transactions ?? []).map((item) => ({
      value: `${item.transaction.kind}${SEPARATOR}${item.transaction.name}`,
      label: item.transaction.name,
    })),
  ]);

  const chooseService = (value: string) => {
    const [namespace = '', service = ''] = value.split(SEPARATOR);
    const environment =
      listed.find((item) => item.service.name === service && item.service.namespace === namespace)?.service.environment ?? null;
    onChange({ ...draft, namespace: namespace || null, service, environment, transaction: null });
  };

  const chooseEndpoint = (value: string) => {
    if (!value) {
      onChange({ ...draft, transaction: null });
      return;
    }
    const [kind = 'http', name = ''] = value.split(SEPARATOR);
    const known = APM_TRANSACTION_KINDS.find((candidate) => candidate === kind) ?? 'http';
    onChange({ ...draft, transaction: { name, kind: known } });
  };

  return (
    <div data-alert-apm-source="" className="grid grid-cols-1 gap-4 md:grid-cols-2">
      <FormField label={t('workbench.apm.service')} required>
        <FormSelect
          ariaLabel={t('workbench.apm.service')}
          value={draft.service ? serviceValue : ''}
          onChange={chooseService}
          options={serviceOptions}
          placeholder={t('workbench.apm.service_placeholder')}
        />
      </FormField>
      <FormField label={t('workbench.apm.environment')}>
        <FormSelect
          ariaLabel={t('workbench.apm.environment')}
          value={draft.environment ?? ''}
          onChange={(value) => onChange({ ...draft, environment: value || null, transaction: null })}
          options={[{ value: '', label: t('workbench.apm.every_environment') }, ...environments]}
        />
      </FormField>
      <FormField label={t('workbench.apm.endpoint')}>
        <FormSelect ariaLabel={t('workbench.apm.endpoint')} value={endpointValue} onChange={chooseEndpoint} options={endpointOptions} />
      </FormField>
      <FormField label={t('workbench.apm.metric')} hint={t(`workbench.apm.unit.${apmMetricUnit(draft.metric)}`)} required>
        <FormSelect
          ariaLabel={t('workbench.apm.metric')}
          value={draft.metric}
          onChange={(value) => onChange({ ...draft, metric: value as ApmAlertMetric })}
          options={(draft.metric === 'slo_burn_rate' ? ['slo_burn_rate' as const] : metrics).map((metric) => ({
            value: metric,
            label: t(`workbench.apm.metrics.${metric}`),
          }))}
        />
      </FormField>
    </div>
  );
}

function label(namespace: string | null | undefined, name: string): string {
  return namespace && namespace !== name ? `${name} · ${namespace}` : name;
}

function unique<T extends { value: string }>(options: readonly T[]): T[] {
  const seen = new Set<string>();
  return options.filter((option) => {
    if (seen.has(option.value)) return false;
    seen.add(option.value);
    return true;
  });
}
