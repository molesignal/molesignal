import { useQueries, useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';

import * as alertsApi from '@/api/alerts';
import * as businessServicesApi from '@/api/businessServices';
import type { AutomationSourceKind, ComponentStatus } from '@/api/statusPages';
import * as syntheticsApi from '@/api/synthetics';
import { FormField, FormSelect, type FormSelectOption } from '@/shell/FormDrawer';

import {
  alertRuleResources,
  type AutomationSourceResource,
  businessServiceResources,
  syntheticMonitorResources,
} from './sourceResources';
import { componentStatusLabel } from '../../../model';

export function SourceResourceField({
  sourceKind,
  value,
  onChange,
  allowAll = true,
  required = false,
  enabled = true,
}: {
  sourceKind: AutomationSourceKind;
  value: string;
  onChange: (value: string) => void;
  allowAll?: boolean;
  required?: boolean;
  enabled?: boolean;
}) {
  const { t } = useTranslation('status-pages');
  const source = useSourceResources(sourceKind, enabled);
  const resourceNoun = sourceKind === 'alert_incident'
    ? 'alert_rule'
    : sourceKind === 'business_service'
      ? 'business_service'
      : 'synthetic_monitor';
  const options: FormSelectOption[] = [];

  if (allowAll) {
    options.push({
      value: '',
      label: t(`automation.source_picker.all_${resourceNoun}`),
    });
  }
  if (value && !source.resources.some((resource) => resource.id === value)) {
    options.push({
      value,
      label: t('automation.source_picker.unavailable', { id: value }),
    });
  }
  options.push(...source.resources.map((resource) => ({
    value: resource.id,
    label: resourceLabel(resource, sourceKind, t),
  })));
  if (!source.pending && !source.error && source.resources.length === 0) {
    options.push({
      value: '__no_resources__',
      label: t(`automation.source_picker.empty_${resourceNoun}`),
      disabled: true,
    });
  }

  const unavailableReason = source.pending
    ? t('automation.source_picker.loading')
    : source.error
      ? t('automation.source_picker.load_error')
      : undefined;
  const placeholder = source.pending
    ? t('automation.source_picker.loading')
    : source.error
      ? t('automation.source_picker.load_error')
      : source.resources.length === 0
        ? t(`automation.source_picker.empty_${resourceNoun}`)
        : t(`automation.source_picker.select_${resourceNoun}`);

  return (
    <FormField
      label={t(`automation.source_picker.${resourceNoun}`)}
      hint={source.error
        ? t('automation.source_picker.load_error')
        : t(`automation.source_picker.${resourceNoun}_${allowAll ? 'hint' : 'required_hint'}`)}
      required={required}
    >
      <FormSelect
        value={value}
        onChange={onChange}
        options={options}
        placeholder={placeholder}
        disabled={source.pending || Boolean(source.error)}
        disabledReason={unavailableReason}
      />
    </FormField>
  );
}

function useSourceResources(sourceKind: AutomationSourceKind, enabled: boolean) {
  const isAlert = sourceKind === 'alert_incident';
  const isService = sourceKind === 'business_service';
  const isMonitor = sourceKind === 'synthetic_monitor';
  const alertRules = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: alertsApi.list,
    enabled: enabled && isAlert,
    staleTime: 30_000,
  });
  const services = useQuery({
    queryKey: ['business-services'],
    queryFn: businessServicesApi.list,
    enabled: enabled && isService,
    staleTime: 30_000,
  });
  const monitors = useQuery({
    queryKey: ['synthetics', 'monitors'],
    queryFn: syntheticsApi.listMonitors,
    enabled: enabled && isMonitor,
    staleTime: 30_000,
  });
  const selectableMonitors = (monitors.data ?? []).filter((monitor) =>
    Boolean(monitor.active_revision_id)
    && (monitor.lifecycle === 'active' || monitor.lifecycle === 'paused'));
  const monitorDetails = useQueries({
    queries: !isMonitor ? [] : selectableMonitors.map((monitor) => ({
      queryKey: ['synthetics', 'monitor', monitor.id],
      queryFn: () => syntheticsApi.getMonitor(monitor.id),
      enabled,
      staleTime: 30_000,
    })),
  });

  if (isAlert) {
    return {
      resources: alertRuleResources(alertRules.data ?? []),
      pending: enabled && alertRules.isPending,
      error: alertRules.error,
    };
  }
  if (isService) {
    return {
      resources: businessServiceResources(services.data ?? []),
      pending: enabled && services.isPending,
      error: services.error,
    };
  }
  return {
    resources: syntheticMonitorResources(
      selectableMonitors,
      monitorDetails.flatMap((query) => query.data ? [query.data] : []),
    ),
    pending: enabled && (
      monitors.isPending || monitorDetails.some((query) => query.isPending)
    ),
    error: monitors.error ?? monitorDetails.find((query) => query.error)?.error,
  };
}

function resourceLabel(
  resource: AutomationSourceResource,
  sourceKind: AutomationSourceKind,
  t: TFunction<'status-pages'>,
): string {
  // A business service is told apart by how it is doing, not by a lifecycle.
  if (sourceKind === 'business_service') {
    return [resource.name, componentStatusLabel(t, resource.target as ComponentStatus)].join(' · ');
  }
  const state = sourceKind === 'alert_incident'
    ? t(`automation.source_picker.${resource.state}`)
    : t(`automation.lifecycle.${resource.state}`);
  return [resource.name, compact(resource.target), state].filter(Boolean).join(' · ');
}

function compact(value: string, maxLength = 80): string {
  const characters = Array.from(value);
  return characters.length <= maxLength
    ? value
    : `${characters.slice(0, maxLength - 1).join('')}…`;
}
