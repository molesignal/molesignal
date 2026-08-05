export type ActivationStepId = 'datasource' | 'dashboard' | 'alert' | 'pipeline' | 'sample-data';

export interface ActivationInputs {
  streamsCount: number;
  dashboardsCount: number;
  alertsCount: number;
  pipelinesCount: number;
  sampleDataAvailable: boolean;
}

export interface ActivationStep {
  id: ActivationStepId;
  labelKey: string;
  descriptionKey: string;
  completed: boolean;
  to: string;
  backendPending?: boolean;
}

export interface ActivationState {
  completedCount: number;
  totalCount: number;
  ready: boolean;
  steps: ActivationStep[];
}

export function deriveActivationState(input: ActivationInputs): ActivationState {
  const steps: ActivationStep[] = [
    {
      id: 'datasource',
      labelKey: 'steps.datasource.title',
      descriptionKey: 'steps.datasource.description',
      completed: input.streamsCount > 0,
      to: '/datasource',
    },
    {
      id: 'dashboard',
      labelKey: 'steps.dashboard.title',
      descriptionKey: 'steps.dashboard.description',
      completed: input.dashboardsCount > 0,
      to: '/dashboards/new/edit',
    },
    {
      id: 'alert',
      labelKey: 'steps.alert.title',
      descriptionKey: 'steps.alert.description',
      completed: input.alertsCount > 0,
      to: '/alerts',
    },
    {
      id: 'pipeline',
      labelKey: 'steps.pipeline.title',
      descriptionKey: 'steps.pipeline.description',
      completed: input.pipelinesCount > 0,
      to: '/pipelines',
    },
    {
      id: 'sample-data',
      labelKey: 'steps.sample_data.title',
      descriptionKey: 'steps.sample_data.description',
      completed: input.sampleDataAvailable,
      to: '/datasource/recommended/http-json',
      backendPending: !input.sampleDataAvailable,
    },
  ];
  const completedCount = steps.filter((step) => step.completed).length;
  return {
    completedCount,
    totalCount: steps.length,
    ready: completedCount >= 3,
    steps,
  };
}
