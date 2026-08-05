import { describe, expect, it } from 'vitest';

import { createDashboardPanel } from '../factories';
import {
  expandRepeatedElements,
  interpolateVariables,
} from '../variables';

describe('dashboard variables', () => {
  it('supports scalar and formatted multi-value interpolation', () => {
    expect(
      interpolateVariables(
        '$service ${regions:csv} ${regions:pipe} ${regions:regex}',
        {
          service: 'api',
          regions: ['us.east', 'eu-west'],
        },
      ),
    ).toBe('api us.east,eu-west us.east|eu-west us\\.east|eu-west');
  });

  it('creates repeat copies only at runtime', () => {
    const source = {
      ...createDashboardPanel(),
      id: 'source',
      title: '$service latency',
      gridPos: { x: 0, y: 0, w: 8, h: 6 },
      repeat: {
        variable: 'service',
        direction: 'horizontal' as const,
      },
    };
    const runtime = expandRepeatedElements(
      [source],
      { service: ['api', 'worker', 'edge'] },
      24,
    );
    expect(runtime.map((item) => item.element.title)).toEqual([
      'api latency',
      'worker latency',
      'edge latency',
    ]);
    expect(runtime.map((item) => item.element.gridPos.x)).toEqual([0, 8, 16]);
    expect(source.id).toBe('source');
  });
});
