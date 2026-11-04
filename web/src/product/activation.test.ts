import { describe, expect, it } from 'vitest';

import type { ProductAccess } from './access';
import {
  type ActivationInputs,
  canReadActivationStep,
  deriveActivationState,
} from './activation';
import type { PermissionKey } from './permissions';

const EMPTY_ORG: ActivationInputs = {
  streams: [],
  dashboardsCount: 0,
  alertsCount: 0,
  schedulesCount: 0,
  notifyConnectors: [],
  modelProviders: [],
  pipelinesCount: 0,
  sampleDataAvailable: false,
};

const OWN_STREAM = { name: 'app' };
const SAMPLE_STREAMS = [
  { name: 'sample_app_logs' },
  { name: 'sample_app_metrics' },
  { name: 'sample_app_traces' },
];

const EVERY_READ: PermissionKey[] = [
  'streams.read',
  'streams.query',
  'dashboards.read',
  'alerts.rules.read',
  'schedules.read',
  'notify.connectors.read',
  'agent.models.read',
  'pipelines.read',
];

const EVERY_CONFIGURE: PermissionKey[] = [
  'logs.write',
  'dashboards.create',
  'alerts.rules.manage',
  'schedules.create',
  'notify.connectors.manage',
  'agent.models.manage',
  'pipelines.create',
];

function access(
  permissions: readonly PermissionKey[],
  closedPaths: readonly string[] = [],
  status: ProductAccess['status'] = 'ready',
): ProductAccess {
  return {
    organizationId: 'acme',
    role: 'Viewer',
    scope: 'organization',
    permissions: new Set(permissions),
    version: 1,
    routeCatalogVersion: 1,
    routes: closedPaths.map((pathPattern) => ({
      id: pathPattern,
      pathPattern: pathPattern,
      allowed: false,
    })),
    status,
  };
}

const FULL_ACCESS = access([...EVERY_READ, ...EVERY_CONFIGURE]);

function stepIds(state: ReturnType<typeof deriveActivationState>) {
  return state.steps.map((step) => step.id);
}

function isDone(input: Partial<ActivationInputs>, id: string) {
  return deriveActivationState({ ...EMPTY_ORG, ...input }, FULL_ACCESS).steps.find(
    (step) => step.id === id,
  )?.completed;
}

describe('deriveActivationState', () => {
  it('marks an empty org as not ready, with all eight steps to do', () => {
    const state = deriveActivationState(EMPTY_ORG, FULL_ACCESS);

    expect(state.ready).toBe(false);
    expect(state.completedCount).toBe(0);
    expect(state.totalCount).toBe(8);
  });

  it('reads as a path from data coming in to answering an alert', () => {
    expect(stepIds(deriveActivationState(EMPTY_ORG, FULL_ACCESS))).toEqual([
      'datasource',
      'sample-data',
      'dashboard',
      'alert',
      'schedule',
      'notification',
      'ai',
      'pipeline',
    ]);
  });

  it('sends each new step to the page that sets it up', () => {
    const to = Object.fromEntries(
      deriveActivationState(EMPTY_ORG, FULL_ACCESS).steps.map((step) => [step.id, step.to]),
    );

    expect(to.schedule).toBe('/alerts/schedules');
    expect(to.notification).toBe('/settings/notify/connectors');
    expect(to.ai).toBe('/agent/settings/models');
  });

  it('marks active orgs ready once core workflows exist', () => {
    const state = deriveActivationState(
      { ...EMPTY_ORG, streams: [OWN_STREAM, { name: 'web' }], dashboardsCount: 1, alertsCount: 1 },
      FULL_ACCESS,
    );

    expect(state.ready).toBe(true);
    expect(state.completedCount).toBe(3);
    expect(state.totalCount).toBe(7);
  });

  it('does not take ready back from an org that had three of the original five', () => {
    const originalFive = [
      { streams: [OWN_STREAM], dashboardsCount: 1, alertsCount: 1 },
      { streams: [OWN_STREAM], alertsCount: 1, pipelinesCount: 1 },
      { streams: SAMPLE_STREAMS, sampleDataAvailable: true, dashboardsCount: 1 },
      { dashboardsCount: 1, pipelinesCount: 1, sampleDataAvailable: true },
    ];

    for (const org of originalFive) {
      expect(deriveActivationState({ ...EMPTY_ORG, ...org }, FULL_ACCESS).ready).toBe(true);
    }
  });

  it('completes the datasource step with a stream the platform did not write', () => {
    for (const name of ['_audit', '_synthetic_results', '_agent_model_traces', '_agent_score_traces']) {
      expect(isDone({ streams: [{ name }] }, 'datasource')).toBe(false);
    }

    expect(isDone({ streams: undefined }, 'datasource')).toBe(false);
    expect(isDone({ streams: [OWN_STREAM] }, 'datasource')).toBe(true);
    expect(isDone({ streams: [{ name: '_audit' }, OWN_STREAM] }, 'datasource')).toBe(true);
    expect(isDone({ streams: SAMPLE_STREAMS }, 'datasource')).toBe(true);
    // Only the platform's exact names are reserved: an org may send to any other.
    expect(isDone({ streams: [{ name: '_my_app' }] }, 'datasource')).toBe(true);
    // The platform organization's telemetry is the data it has.
    expect(isDone({ streams: [{ name: '_molesignal' }] }, 'datasource')).toBe(true);
  });

  it('does not call an org ready on a stream the platform wrote', () => {
    const state = deriveActivationState(
      { ...EMPTY_ORG, streams: [{ name: '_audit' }], dashboardsCount: 1, alertsCount: 1 },
      FULL_ACCESS,
    );

    expect(state.completedCount).toBe(2);
    expect(state.ready).toBe(false);
  });

  it('completes the schedule step with any schedule', () => {
    expect(isDone({}, 'schedule')).toBe(false);
    expect(isDone({ schedulesCount: 1 }, 'schedule')).toBe(true);
  });

  it('completes the notification step only with a connector the engine would send through', () => {
    const connector = { enabled: true, status: 'connected' } as const;

    expect(isDone({ notifyConnectors: [] }, 'notification')).toBe(false);
    expect(isDone({ notifyConnectors: [connector] }, 'notification')).toBe(true);
    expect(
      isDone({ notifyConnectors: [{ ...connector, status: 'unknown' }] }, 'notification'),
    ).toBe(true);
    expect(
      isDone({ notifyConnectors: [{ ...connector, enabled: false }] }, 'notification'),
    ).toBe(false);
    expect(
      isDone({ notifyConnectors: [{ ...connector, status: 'error' }] }, 'notification'),
    ).toBe(false);
    expect(
      isDone(
        { notifyConnectors: [{ ...connector, status: 'error' }, connector] },
        'notification',
      ),
    ).toBe(true);
  });

  it('completes the AI step only with an enabled provider that has a key', () => {
    const provider = { enabled: true, keySet: true };

    expect(isDone({ modelProviders: [] }, 'ai')).toBe(false);
    expect(isDone({ modelProviders: [provider] }, 'ai')).toBe(true);
    expect(isDone({ modelProviders: [{ ...provider, enabled: false }] }, 'ai')).toBe(false);
    expect(isDone({ modelProviders: [{ ...provider, keySet: false }] }, 'ai')).toBe(false);
  });

  it('leaves out a step whose list the reader may not read', () => {
    const permissions = [...EVERY_READ, ...EVERY_CONFIGURE].filter(
      (permission) => permission !== 'notify.connectors.read' && permission !== 'agent.models.read',
    );
    const state = deriveActivationState(EMPTY_ORG, access(permissions));

    expect(state.totalCount).toBe(6);
    expect(stepIds(state)).not.toContain('notification');
    expect(stepIds(state)).not.toContain('ai');
  });

  it('leaves out a step left to do that the reader cannot do, and keeps it once it is done', () => {
    const readOnly = access(EVERY_READ);

    expect(deriveActivationState(EMPTY_ORG, readOnly).totalCount).toBe(0);

    const state = deriveActivationState({ ...EMPTY_ORG, schedulesCount: 1 }, readOnly);

    expect(stepIds(state)).toEqual(['schedule']);
    expect(state.completedCount).toBe(1);
    expect(state.totalCount).toBe(1);
  });

  it('offers the AI step on the permission that adds a provider, whatever else the reader holds', () => {
    const modelsOnly = access(['agent.use', 'agent.models.read', 'agent.models.manage']);
    const profilesInstead = access(['agent.use', 'agent.models.read', 'agent.profiles.manage']);

    expect(stepIds(deriveActivationState(EMPTY_ORG, modelsOnly))).toEqual(['ai']);
    expect(stepIds(deriveActivationState(EMPTY_ORG, profilesInstead))).toEqual([]);
  });

  it('accepts any of the writes a source can send with for the datasource step', () => {
    const metricsOnly = access(['streams.read', 'metrics.write']);

    expect(stepIds(deriveActivationState(EMPTY_ORG, metricsOnly))).toEqual(['datasource']);
  });

  it('leaves out a step whose page the catalog closes, done or not', () => {
    const closed = access(
      [...EVERY_READ, ...EVERY_CONFIGURE],
      ['/agent/:section/:id', '/dashboards/new/edit'],
    );
    const state = deriveActivationState({ ...EMPTY_ORG, dashboardsCount: 1 }, closed);

    expect(stepIds(state)).not.toContain('ai');
    expect(stepIds(state)).not.toContain('dashboard');
  });

  it('keeps an org ready for a reader who cannot open a step that is done', () => {
    const closed = access(
      [...EVERY_READ, ...EVERY_CONFIGURE],
      ['/dashboards/new/edit'],
    );
    const state = deriveActivationState(
      { ...EMPTY_ORG, streams: [OWN_STREAM], dashboardsCount: 1, alertsCount: 1 },
      closed,
    );

    expect(state.completedCount).toBe(2);
    expect(state.ready).toBe(true);
  });

  it('does not hold a reader who can see fewer than three steps to three', () => {
    const narrow = access(['streams.read', 'logs.write', 'schedules.read', 'schedules.create']);

    expect(deriveActivationState(EMPTY_ORG, narrow).ready).toBe(false);
    expect(
      deriveActivationState({ ...EMPTY_ORG, streams: [OWN_STREAM], schedulesCount: 1 }, narrow).ready,
    ).toBe(true);
  });

  it('offers nothing until what the reader holds is known', () => {
    const loading = access([...EVERY_READ, ...EVERY_CONFIGURE], [], 'loading');

    expect(deriveActivationState(EMPTY_ORG, loading).totalCount).toBe(0);
    expect(deriveActivationState(EMPTY_ORG, null).totalCount).toBe(0);
  });
});

describe('the sample data step', () => {
  const OTHER_SEVEN_DONE: Partial<ActivationInputs> = {
    streams: [OWN_STREAM],
    dashboardsCount: 1,
    alertsCount: 1,
    schedulesCount: 1,
    notifyConnectors: [{ enabled: true, status: 'connected' }],
    modelProviders: [{ enabled: true, keySet: true }],
    pipelinesCount: 1,
  };

  it('leaves nothing to do for an org with streams of its own once the other seven are done', () => {
    const state = deriveActivationState({ ...EMPTY_ORG, ...OTHER_SEVEN_DONE }, FULL_ACCESS);

    expect(stepIds(state)).not.toContain('sample-data');
    expect(state.completedCount).toBe(7);
    expect(state.totalCount).toBe(7);
  });

  it('is still offered to an org whose only streams are the ones the platform writes', () => {
    for (const name of ['_audit', '_synthetic_results', '_agent_model_traces', '_agent_score_traces']) {
      const state = deriveActivationState({ ...EMPTY_ORG, streams: [{ name }] }, FULL_ACCESS);

      expect(stepIds(state)).toContain('sample-data');
    }
  });

  it('leaves the strip on connecting a datasource while there is one to connect', () => {
    for (const streams of [[], [{ name: '_audit' }]]) {
      const state = deriveActivationState({ ...EMPTY_ORG, streams }, FULL_ACCESS);

      expect(state.steps.find((step) => !step.completed)?.id).toBe('datasource');
    }
  });

  it('is still offered after a load that stopped short, which the status calls not loaded', () => {
    const state = deriveActivationState(
      { ...EMPTY_ORG, streams: [{ name: 'sample_app_traces' }] },
      FULL_ACCESS,
    );

    expect(state.steps.find((step) => !step.completed)?.id).toBe('sample-data');
  });

  it('shows as done once loaded, with streams of the org\'s own beside it or not', () => {
    expect(isDone({ sampleDataAvailable: true, streams: SAMPLE_STREAMS }, 'sample-data')).toBe(true);
    expect(
      isDone({ sampleDataAvailable: true, streams: [...SAMPLE_STREAMS, OWN_STREAM] }, 'sample-data'),
    ).toBe(true);
  });

  it('stays done for a reader who can no longer tell what streams there are', () => {
    expect(isDone({ sampleDataAvailable: true, streams: undefined }, 'sample-data')).toBe(true);
  });

  it('is not offered until the list has answered, nor to a reader who may not read it', () => {
    const state = deriveActivationState(
      { ...EMPTY_ORG, streams: undefined },
      access(['streams.query', 'logs.write']),
    );

    expect(stepIds(state)).toEqual([]);
  });

  it('does not move ready by not being offered, for a narrow reader either', () => {
    const narrow = access(['streams.read', 'streams.query', 'logs.write']);

    // The two steps this reader can see set the bar at two, offered or not: one done is not enough.
    expect(deriveActivationState({ ...EMPTY_ORG, streams: [OWN_STREAM] }, narrow).ready).toBe(false);

    for (const reader of [FULL_ACCESS, narrow]) {
      for (const org of [{}, { dashboardsCount: 1 }, { dashboardsCount: 1, alertsCount: 1 }]) {
        // Either way the datasource step is done and the sample step is not.
        const offered = deriveActivationState({ ...EMPTY_ORG, ...org, streams: [{ name: 'sample_app_traces' }] }, reader);
        const leftOut = deriveActivationState({ ...EMPTY_ORG, ...org, streams: [OWN_STREAM] }, reader);

        expect(stepIds(offered)).toContain('sample-data');
        expect(stepIds(leftOut)).not.toContain('sample-data');
        expect(leftOut.ready).toBe(offered.ready);
      }
    }
  });
});

describe('canReadActivationStep', () => {
  it('asks for what the route behind the list asks for', () => {
    expect(canReadActivationStep('notification', access(['notify.connectors.read']))).toBe(true);
    expect(canReadActivationStep('notification', access(['notify.connectors.manage']))).toBe(false);
    expect(canReadActivationStep('ai', access(['agent.models.read']))).toBe(true);
    expect(canReadActivationStep('schedule', access(['schedules.read']))).toBe(true);
  });

  it('keeps recent dashboards for a platform session, whose read is its own', () => {
    expect(canReadActivationStep('dashboard', access(['sys.dashboards.read']))).toBe(true);
  });

  it('asks for nothing until what the reader holds is known', () => {
    expect(
      canReadActivationStep('schedule', access(['schedules.read'], [], 'loading')),
    ).toBe(false);
    expect(canReadActivationStep('schedule', null)).toBe(false);
  });
});
