import { describe, expect, it } from 'vitest';

import { deriveActivationState } from './activation';

describe('deriveActivationState', () => {
  it('marks an empty org as not ready and exposes sample data as backend pending', () => {
    const state = deriveActivationState({
      streamsCount: 0,
      dashboardsCount: 0,
      alertsCount: 0,
      pipelinesCount: 0,
      sampleDataAvailable: false,
    });

    expect(state.ready).toBe(false);
    expect(state.completedCount).toBe(0);
    expect(state.steps.find((step) => step.id === 'sample-data')?.backendPending).toBe(true);
  });

  it('marks active orgs ready once core workflows exist', () => {
    const state = deriveActivationState({
      streamsCount: 2,
      dashboardsCount: 1,
      alertsCount: 1,
      pipelinesCount: 0,
      sampleDataAvailable: false,
    });

    expect(state.ready).toBe(true);
    expect(state.completedCount).toBe(3);
  });
});
