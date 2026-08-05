import { describe, expect, expectTypeOf, it } from 'vitest';

import type { DashboardDefinition } from '../schema';
import invalidAuthoringCompatibility from './generated/fixtures/invalid/authoring-v1-incompatible-visualization.json';
import invalidAuthoringUnknown from './generated/fixtures/invalid/authoring-v1-unknown-field.json';
import invalidAuthoringVersion from './generated/fixtures/invalid/authoring-v2.json';
import invalidDashboardDuplicate from './generated/fixtures/invalid/dashboard-v2-duplicate-id.json';
import invalidDashboardUnknown from './generated/fixtures/invalid/dashboard-v2-unknown-field.json';
import invalidDashboardVersion from './generated/fixtures/invalid/dashboard-v3.json';
import fixtureManifest from './generated/fixtures/manifest.json';
import validAuthoringPromql from './generated/fixtures/valid/authoring-v1-promql.json';
import validAuthoringTyped from './generated/fixtures/valid/authoring-v1-typed-queries.json';
import validDashboardNested from './generated/fixtures/valid/dashboard-v2-nested.json';
import type { DashboardModelV2Contract } from './types';
import {
  validateDashboardAuthoringContract,
  validateDashboardModelContract,
} from './validator';

type SharedModelCore = Pick<
  DashboardDefinition,
  | 'engine'
  | 'schemaVersion'
  | 'uid'
  | 'title'
  | 'tags'
  | 'editable'
  | 'defaultDashboard'
  | 'timeSettings'
  | 'refreshSettings'
  | 'interactionSettings'
  | 'layout'
  | 'elements'
>;
type SchemaModelCore = Pick<DashboardModelV2Contract, keyof SharedModelCore>;

const fixtureValues: Record<string, unknown> = {
  'valid/dashboard-v2-nested.json': validDashboardNested,
  'valid/authoring-v1-promql.json': validAuthoringPromql,
  'valid/authoring-v1-typed-queries.json': validAuthoringTyped,
  'invalid/dashboard-v2-unknown-field.json': invalidDashboardUnknown,
  'invalid/dashboard-v2-duplicate-id.json': invalidDashboardDuplicate,
  'invalid/dashboard-v3.json': invalidDashboardVersion,
  'invalid/authoring-v2.json': invalidAuthoringVersion,
  'invalid/authoring-v1-unknown-field.json': invalidAuthoringUnknown,
  'invalid/authoring-v1-incompatible-visualization.json':
    invalidAuthoringCompatibility,
};

describe('generated Dashboard contracts', () => {
  it('keeps the schema-derived core compatible with the domain type', () => {
    expectTypeOf<SharedModelCore>().toMatchTypeOf<SchemaModelCore>();
    expectTypeOf<SchemaModelCore>().toMatchTypeOf<SharedModelCore>();
  });

  it('validates shared positive fixtures with Ajv 2020', () => {
    expect(validateDashboardModelContract(validDashboardNested).valid).toBe(
      true,
    );
    expect(validateDashboardAuthoringContract(validAuthoringPromql).valid).toBe(
      true,
    );
    expect(fixtureManifest.version).toBe(1);
  });

  it('normalizes shared negative fixture failures', () => {
    const dashboard = validateDashboardModelContract(invalidDashboardUnknown);
    const authoring = validateDashboardAuthoringContract(
      invalidAuthoringVersion,
    );
    expect(dashboard.valid).toBe(false);
    expect(authoring.valid).toBe(false);
    if (!dashboard.valid) {
      expect(dashboard.issues.map((issue) => issue.code)).toContain(
        'CONTRACT_ADDITIONAL_PROPERTY',
      );
    }
    if (!authoring.valid) {
      expect(authoring.issues.map((issue) => issue.code)).toContain(
        'UNSUPPORTED_AUTHORING_VERSION',
      );
    }
  });

  it('runs every shared corpus case through the matching Ajv validator', () => {
    expect(Object.keys(fixtureValues)).toHaveLength(fixtureManifest.cases.length);
    for (const fixture of fixtureManifest.cases) {
      const value = fixtureValues[fixture.path];
      expect(value, `missing generated fixture ${fixture.path}`).toBeDefined();
      const result =
        fixture.contract === 'dashboard-model-v2'
          ? validateDashboardModelContract(value)
          : validateDashboardAuthoringContract(value);
      // Semantic-only failures remain schema-valid in Web and are rejected by the
      // Rust compiler/write boundary before persistence.
      const schemaValid = fixture.valid || ('semantic' in fixture && fixture.semantic);
      expect(result.valid, fixture.path).toBe(schemaValid);
      if (!schemaValid && !result.valid) {
        for (const code of fixture.expectedIssueCodes ?? []) {
          expect(
            result.issues.map((issue) => issue.code),
            fixture.path,
          ).toContain(code);
        }
      }
    }
  });
});
