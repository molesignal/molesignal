import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as modelProvidersApi from '@/api/agent/modelProviders';
import * as alertsApi from '@/api/alerts';
import * as dashboardsApi from '@/api/dashboards';
import * as notifyApi from '@/api/notify';
import * as onboardingApi from '@/api/onboarding';
import * as pipelinesApi from '@/api/pipelines';
import * as schedulesApi from '@/api/schedules';
import * as streamsApi from '@/api/streams';
import { toApiError } from '@/lib/http';
import { useProductAccess } from '@/product/access';
import { canReadActivationStep, deriveActivationState } from '@/product/activation';
import { toast } from '@/shell/ui/sonner';

/**
 * How far setting up has gone, from every list the reader may read, and loading the sample data.
 * The steps left can only be counted once each of those lists has answered.
 */
export function useActivation() {
  const { t } = useTranslation('onboarding');
  const qc = useQueryClient();
  const access = useProductAccess();
  const can = (step: Parameters<typeof canReadActivationStep>[0]) => canReadActivationStep(step, access);

  const sampleStatus = useQuery({
    queryKey: ['onboarding', 'sample-data'],
    queryFn: onboardingApi.getSampleDataStatus,
    enabled: can('sample-data'),
  });
  const streams = useQuery({
    queryKey: ['streams', 'list'],
    queryFn: () => streamsApi.list(200),
    enabled: can('datasource'),
  });
  const dashboards = useQuery({
    queryKey: ['dashboards', 'list'],
    queryFn: () => dashboardsApi.list(),
    enabled: can('dashboard'),
  });
  const rules = useQuery({ queryKey: ['alerts', 'rules'], queryFn: () => alertsApi.list(), enabled: can('alert') });
  const pipelines = useQuery({
    queryKey: ['pipelines', 'list'],
    queryFn: () => pipelinesApi.list(),
    enabled: can('pipeline'),
  });
  const connectors = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: notifyApi.listConnectors,
    enabled: can('notification'),
  });
  const modelProviders = useQuery({
    queryKey: ['agent', 'model-providers'],
    queryFn: modelProvidersApi.list,
    enabled: can('ai'),
  });
  const schedules = useQuery({
    queryKey: ['schedules'],
    queryFn: schedulesApi.list,
    enabled: can('schedule'),
  });

  const state = deriveActivationState(
    {
      streams: streams.data,
      dashboardsCount: dashboards.data?.length ?? 0,
      alertsCount: rules.data?.length ?? 0,
      schedulesCount: schedules.data?.length ?? 0,
      notifyConnectors: connectors.data ?? [],
      modelProviders: modelProviders.data ?? [],
      pipelinesCount: pipelines.data?.length ?? 0,
      sampleDataAvailable: sampleStatus.data?.loaded ?? false,
    },
    access,
  );
  const known =
    access?.status === 'ready' &&
    [streams, dashboards, rules, schedules, connectors, modelProviders, pipelines, sampleStatus].every(
      (query) => !query.isEnabled || query.isSuccess,
    );
  const next = state.steps.find((step) => !step.completed);

  const loadSample = useMutation({
    mutationFn: onboardingApi.loadSampleData,
    onSuccess: (result) => {
      toast.success(t('activation.load_success', { rows: result.totalRows }));
      void qc.invalidateQueries({ queryKey: ['onboarding', 'sample-data'] });
      void qc.invalidateQueries({ queryKey: ['streams'] });
      void qc.invalidateQueries({ queryKey: ['dashboards'] });
      void qc.invalidateQueries({ queryKey: ['saved-views'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return {
    state,
    /** The steps left and the next one's name, once they can be counted. */
    todo: known
      ? { remaining: state.totalCount - state.completedCount, nextLabel: next ? t(`activation.${next.labelKey}`) : null }
      : null,
    loadSample,
  };
}
