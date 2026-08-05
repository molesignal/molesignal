import { describe, expect, it } from 'vitest';

import {
  createEmptyDashboardDefinition,
  dashboardDefinitionFromApi,
  dashboardDefinitionFromStoredModel,
  dashboardDefinitionToModel,
  parseDashboardDefinitionJson,
  validateDashboardDefinition,
} from '../model';
import { createDashboardGroup, createDashboardPanel } from '../factories';

describe('dashboard model', () => {
  it('round-trips the current schema without compatibility fields', () => {
    const definition = createEmptyDashboardDefinition('Current dashboard');
    definition.elements = [createDashboardPanel()];
    const model = dashboardDefinitionToModel(definition);
    const parsed = parseDashboardDefinitionJson(JSON.stringify(model));

    expect(parsed.engine).toBe('molesignal-dashboard');
    expect(parsed.schemaVersion).toBe(2);
    expect(parsed.elements).toHaveLength(1);
    expect(parsed.interactionSettings).toEqual({
      cursorSync: 'off',
    });
    expect(model).not.toHaveProperty('panels');
    expect(validateDashboardDefinition(parsed)).toEqual({
      valid: true,
      issues: [],
    });
  });

  it('rejects unsupported dashboard JSON', () => {
    expect(() =>
      parseDashboardDefinitionJson(
        JSON.stringify({
          title: 'Legacy',
          schemaVersion: 39,
          panels: [],
        }),
      ),
    ).toThrow(/engine must be molesignal-dashboard/);
  });

  it('upgrades a persisted Grafana-style dashboard without weakening JSON imports', () => {
    const definition = dashboardDefinitionFromApi({
      id: 'dashboard-legacy',
      org_id: 'org-1',
      uid: 'legacy-overview',
      title: 'Legacy overview',
      tags: ['legacy'],
      version: 7,
      created_at: 1_785_062_832_000_000,
      updated_at: 1_785_062_833_000_000,
      model: {
        schemaVersion: 39,
        time: { from: 'now-1h', to: 'now' },
        refresh: '1m',
        templating: {
          list: [
            {
              name: 'service',
              type: 'query',
              query: 'label_values(http_requests_total, service)',
              current: { value: 'checkout' },
            },
          ],
        },
        panels: [
          {
            id: 4,
            title: 'Request rate',
            type: 'graph',
            gridPos: { x: 0, y: 0, w: 12, h: 8 },
            targets: [
              {
                refId: 'A',
                expr: 'rate(http_requests_total[5m])',
                datasource: { type: 'prometheus' },
              },
            ],
          },
        ],
      },
    });

    expect(validateDashboardDefinition(definition)).toEqual({
      valid: true,
      issues: [],
    });
    expect(definition).toMatchObject({
      engine: 'molesignal-dashboard',
      schemaVersion: 2,
      id: 'dashboard-legacy',
      uid: 'legacy-overview',
      title: 'Legacy overview',
      version: 7,
      timeSettings: { defaultFrom: 'now-1h', defaultTo: 'now' },
      refreshSettings: {
        enabled: true,
        mode: 'interval',
        defaultInterval: '1m',
      },
    });
    expect(definition.variables[0]).toMatchObject({
      name: 'service',
      currentValue: 'checkout',
    });
    expect(definition.elements[0]).toMatchObject({
      kind: 'panel',
      title: 'Request rate',
      visualization: { type: 'time_series' },
      queries: [
        {
          refId: 'A',
          dataSourceType: 'metrics',
          query: {
            language: 'promql',
            expression: 'rate(http_requests_total[5m])',
          },
        },
      ],
    });
  });

  it('repairs a persisted partial v2 envelope with safe defaults', () => {
    const definition = dashboardDefinitionFromApi({
      id: 'dashboard-partial',
      org_id: 'org-1',
      uid: 'partial',
      title: 'Partial dashboard',
      tags: [],
      version: 1,
      created_at: 0,
      updated_at: 0,
      model: {
        title: 'Partial dashboard',
        elements: [],
      },
    });

    expect(validateDashboardDefinition(definition)).toEqual({
      valid: true,
      issues: [],
    });
    expect(definition).toMatchObject({
      engine: 'molesignal-dashboard',
      schemaVersion: 2,
      uid: 'partial',
      interactionSettings: { cursorSync: 'off' },
      variables: [],
      annotations: [],
      links: [],
      elements: [],
    });
  });

  it('upgrades a scrubbed public legacy model without query text', () => {
    const definition = dashboardDefinitionFromStoredModel(
      {
        title: 'Public legacy dashboard',
        uid: 'public-share-1',
        panels: [
          {
            id: 4,
            title: 'Request rate',
            type: 'graph',
            targets: [
              {
                refId: 'A',
                expr: '__molesignal_public_query__',
                language: 'promql',
                stream_type: 'metrics',
              },
            ],
          },
        ],
      },
      'Fallback title',
      'fallback-uid',
    );

    expect(validateDashboardDefinition(definition)).toEqual({
      valid: true,
      issues: [],
    });
    expect(definition).toMatchObject({
      engine: 'molesignal-dashboard',
      schemaVersion: 2,
      title: 'Public legacy dashboard',
      uid: 'public-share-1',
    });
    expect(definition.elements[0]).toMatchObject({
      id: 'legacy-4-1',
      kind: 'panel',
      queries: [
        {
          refId: 'A',
          dataSourceType: 'metrics',
          query: {
            expression: '__molesignal_public_query__',
          },
        },
      ],
    });
  });

  it('requires the current refresh model without inferring old defaults', () => {
    const definition = createEmptyDashboardDefinition('Strict dashboard');
    const value = JSON.parse(JSON.stringify(definition)) as Record<
      string,
      unknown
    >;
    const refreshSettings = value.refreshSettings as Record<string, unknown>;
    delete refreshSettings.mode;

    expect(validateDashboardDefinition(value)).toEqual({
      valid: false,
      issues: expect.arrayContaining([
        'refreshSettings.mode must be off, interval or live',
      ]),
    });
  });

  it('keeps older v2 models valid while rejecting unknown cursor sync modes', () => {
    const definition = createEmptyDashboardDefinition('Cursor sync');
    const legacyValue = JSON.parse(JSON.stringify(definition)) as Record<
      string,
      unknown
    >;
    delete legacyValue.interactionSettings;

    expect(validateDashboardDefinition(legacyValue)).toEqual({
      valid: true,
      issues: [],
    });

    const invalidValue = JSON.parse(JSON.stringify(definition)) as Record<
      string,
      unknown
    >;
    invalidValue.interactionSettings = { cursorSync: 'shared_tooltip' };
    expect(validateDashboardDefinition(invalidValue)).toEqual({
      valid: false,
      issues: expect.arrayContaining([
        'interactionSettings.cursorSync must be off or shared_crosshair',
      ]),
    });
  });

  it('validates nested element positions against the configured grid', () => {
    const definition = createEmptyDashboardDefinition('Nested dashboard');
    const group = createDashboardGroup();
    const panel = createDashboardPanel();
    panel.gridPos = { ...panel.gridPos, x: 20, w: 12 };
    group.elements = [panel];
    definition.elements = [group];

    expect(validateDashboardDefinition(definition)).toEqual({
      valid: false,
      issues: expect.arrayContaining([
        expect.stringContaining('exceeds the configured grid'),
      ]),
    });
  });
});
