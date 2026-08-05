import { describe, expect, it } from 'vitest';

import { createDashboardPanel } from '../factories';
import {
  DEFAULT_QUERY_LEGEND_TEMPLATE,
  QUERY_LEGEND_AUTO,
  queryLegendValueForMode,
  resolveQueryLegendMode,
} from './legend';

describe('Grafana query Legend modes', () => {
  it('resolves Auto, legacy Verbose, and Custom persisted values', () => {
    expect(resolveQueryLegendMode(QUERY_LEGEND_AUTO)).toBe('auto');
    expect(resolveQueryLegendMode(undefined)).toBe('verbose');
    expect(resolveQueryLegendMode('')).toBe('verbose');
    expect(resolveQueryLegendMode('__verbose')).toBe('verbose');
    expect(resolveQueryLegendMode('{{service}}')).toBe('custom');
  });

  it('maps mode selections to Grafana-compatible values', () => {
    expect(queryLegendValueForMode('auto')).toBe(QUERY_LEGEND_AUTO);
    expect(queryLegendValueForMode('verbose')).toBeUndefined();
    expect(queryLegendValueForMode('custom')).toBe(
      DEFAULT_QUERY_LEGEND_TEMPLATE,
    );
  });

  it('defaults new metrics panels to Auto', () => {
    expect(createDashboardPanel().queries[0]?.legend).toBe(
      QUERY_LEGEND_AUTO,
    );
  });
});
