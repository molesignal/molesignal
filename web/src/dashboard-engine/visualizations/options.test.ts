import { describe, expect, it } from 'vitest';

import {
  resolveVisualizationOptions,
  transitionVisualizationOptions,
} from './options';

describe('visualization option integration', () => {
  it('fills sparse persisted options and preserves stored overrides', () => {
    expect(
      resolveVisualizationOptions(
        {
          orientation: 'vertical',
          calculation: 'last',
          showValues: 'auto',
        },
        { orientation: 'horizontal', custom: 'preserved' },
      ),
    ).toEqual({
      orientation: 'horizontal',
      calculation: 'last',
      showValues: 'auto',
      custom: 'preserved',
    });
  });

  it('carries only target-supported option names across a type change', () => {
    expect(
      transitionVisualizationOptions(
        {
          orientation: 'horizontal',
          calculation: 'last',
          displayMode: 'basic',
        },
        {
          orientation: 'vertical',
          calculation: 'avg',
          lineWidth: 4,
          graphMode: 'area',
        },
      ),
    ).toEqual({
      orientation: 'vertical',
      calculation: 'avg',
      displayMode: 'basic',
    });
  });
});
