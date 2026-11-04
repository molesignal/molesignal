import type { ModelProvider } from '@/api/agent/modelProviders';
import type { NotifyConnector } from '@/api/notify';
import type { StreamSummary } from '@/api/streams';
import {
  hasPermission,
  isProductPathDenied,
  type ProductAccess,
} from '@/product/access';
import type { PermissionKey } from '@/product/permissions';
import { isPlatformStream } from '@/product/platformStreams';

export type ActivationStepId =
  | 'datasource'
  | 'dashboard'
  | 'alert'
  | 'schedule'
  | 'notification'
  | 'ai'
  | 'pipeline'
  | 'sample-data';

export interface ActivationInputs {
  /** Missing until the list has answered, and for a reader who may not read it. */
  streams: ReadonlyArray<Pick<StreamSummary, 'name'>> | undefined;
  dashboardsCount: number;
  alertsCount: number;
  schedulesCount: number;
  notifyConnectors: ReadonlyArray<Pick<NotifyConnector, 'enabled' | 'status'>>;
  modelProviders: ReadonlyArray<Pick<ModelProvider, 'enabled' | 'keySet'>>;
  pipelinesCount: number;
  sampleDataAvailable: boolean;
}

export interface ActivationStep {
  id: ActivationStepId;
  labelKey: string;
  descriptionKey: string;
  completed: boolean;
  to: string;
}

export interface ActivationState {
  completedCount: number;
  totalCount: number;
  ready: boolean;
  steps: ActivationStep[];
}

/**
 * A number rather than a share of the list, so that a step added to the list later cannot take
 * this back from a workspace that already reached it.
 */
const READY_STEP_COUNT = 3;

/** What loading sample data writes (`onboarding.rs`). */
const SAMPLE_STREAMS = new Set(['sample_app_logs', 'sample_app_metrics', 'sample_app_traces']);

/** Whether a stream holds data the org sent itself, not what the platform or a sample load wrote. */
function isOwnStream({ name }: Pick<StreamSummary, 'name'>): boolean {
  return !isPlatformStream(name) && !SAMPLE_STREAMS.has(name);
}

type AccessRule = (access: ProductAccess | null) => boolean;

const anyOf =
  (...permissions: PermissionKey[]): AccessRule =>
  (access) =>
    permissions.some((permission) => hasPermission(permission, access));

/**
 * `read` is what the route behind the step's list asks for, `configure` what doing the step takes.
 */
const STEP_ACCESS: Record<ActivationStepId, { read: AccessRule; configure: AccessRule }> = {
  datasource: {
    read: anyOf('streams.read', 'sys.telemetry.read'),
    // An intake token carries the writes its holder has, and any one of them lets a source send.
    configure: anyOf('logs.write', 'metrics.write', 'traces.write', 'profiles.write'),
  },
  dashboard: {
    read: anyOf('dashboards.read', 'sys.dashboards.read'),
    configure: anyOf('dashboards.create'),
  },
  alert: {
    read: anyOf('alerts.rules.read'),
    configure: anyOf('alerts.rules.manage'),
  },
  schedule: {
    read: anyOf('schedules.read'),
    configure: anyOf('schedules.create'),
  },
  notification: {
    read: anyOf('notify.connectors.read'),
    configure: anyOf('notify.connectors.manage'),
  },
  ai: {
    read: anyOf('agent.models.read'),
    configure: anyOf('agent.models.manage'),
  },
  pipeline: {
    read: anyOf('pipelines.read'),
    configure: anyOf('pipelines.create'),
  },
  'sample-data': {
    read: anyOf('streams.query', 'sys.telemetry.read'),
    configure: anyOf('logs.write'),
  },
};

/** Whether the reader may ask for the list that says if this step is done. */
export function canReadActivationStep(
  id: ActivationStepId,
  access: ProductAccess | null,
): boolean {
  return STEP_ACCESS[id].read(access);
}

/**
 * The steps run from data coming in to someone answering an alert with help at hand. Sample data
 * is a second way for data to come in, so it follows the datasource step. The strip names the first
 * step left, which stays on connecting a datasource: sample data comes up there only when a load
 * stopped short.
 */
export function deriveActivationState(
  input: ActivationInputs,
  access: ProductAccess | null,
): ActivationState {
  const allSteps: ActivationStep[] = [
    {
      id: 'datasource',
      labelKey: 'steps.datasource.title',
      descriptionKey: 'steps.datasource.description',
      // A platform stream says nothing about data coming in: an audited action alone leaves `_audit`.
      completed: Boolean(input.streams?.some((stream) => !isPlatformStream(stream.name))),
      to: '/datasource',
    },
    {
      id: 'sample-data',
      labelKey: 'steps.sample_data.title',
      descriptionKey: 'steps.sample_data.description',
      completed: input.sampleDataAvailable,
      to: '/datasource/recommended/http-json',
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
      id: 'schedule',
      labelKey: 'steps.schedule.title',
      descriptionKey: 'steps.schedule.description',
      completed: input.schedulesCount > 0,
      to: '/alerts/schedules',
    },
    {
      id: 'notification',
      labelKey: 'steps.notification.title',
      descriptionKey: 'steps.notification.description',
      // What the notify engine would send through: it skips a disabled connector and one whose
      // test failed, while one nobody has tested yet is used.
      completed: input.notifyConnectors.some(
        (connector) => connector.enabled && connector.status !== 'error',
      ),
      to: '/settings/notify/connectors',
    },
    {
      id: 'ai',
      labelKey: 'steps.ai.title',
      descriptionKey: 'steps.ai.description',
      // A provider without a key is refused at the first call.
      completed: input.modelProviders.some(
        (provider) => provider.enabled && provider.keySet,
      ),
      to: '/agent/settings/models',
    },
    {
      id: 'pipeline',
      labelKey: 'steps.pipeline.title',
      descriptionKey: 'steps.pipeline.description',
      completed: input.pipelinesCount > 0,
      to: '/pipelines',
    },
  ];
  const known = allSteps.filter((step) => canReadActivationStep(step.id, access));
  // Sample data offered to an org with data of its own would stay on the strip for good, and loading
  // it would write sample streams beside that data. Without the list nobody can tell, so it waits.
  const wantsSampleData = input.streams !== undefined && !input.streams.some(isOwnStream);
  // A step that could only lead to "no permission", or wait for someone else, is not offered.
  const steps = known.filter(
    (step) =>
      !isProductPathDenied(step.to, access) &&
      (step.completed ||
        (STEP_ACCESS[step.id].configure(access) &&
          (step.id !== 'sample-data' || wantsSampleData))),
  );
  const completedCount = steps.filter((step) => step.completed).length;
  // Whether the workspace is active, so it counts what the reader may see done but not open.
  const knownCompletedCount = known.filter((step) => step.completed).length;

  return {
    completedCount,
    totalCount: steps.length,
    ready: knownCompletedCount >= Math.min(READY_STEP_COUNT, known.length),
    steps,
  };
}
